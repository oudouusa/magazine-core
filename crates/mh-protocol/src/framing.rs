//! stdio frame codec: `4-byte unsigned big-endian length` + `UTF-8 JSON payload`.
//!
//! [`canonical_json`] is the **wire/golden encoding rule** (sorted keys, no
//! whitespace) used to produce byte-identical golden fixtures. It is NOT a
//! receiving requirement: a conforming receiver MUST accept any well-formed
//! JSON regardless of key order or whitespace. Byte-level equality across
//! languages is only guaranteed for the pinned golden vectors; in general,
//! number formatting (e.g. floats) may differ between implementations.

use std::io::{ErrorKind, Read, Write};

/// Hard cap on a single frame payload (8 MiB). Frames larger than this are
/// rejected on both read and write.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// Error reading or writing a frame.
#[derive(Debug)]
pub enum FrameError {
    /// Clean end of stream at a frame boundary (peer closed, zero bytes read).
    Eof,
    /// Stream ended mid-frame (partial length prefix or partial body).
    Truncated,
    /// Declared or actual payload length exceeds [`MAX_FRAME`].
    TooLarge(usize),
    /// Underlying I/O error.
    Io(std::io::Error),
    /// Payload was not valid UTF-8.
    Utf8,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Eof => write!(f, "eof at frame boundary"),
            FrameError::Truncated => write!(f, "truncated frame"),
            FrameError::TooLarge(n) => write!(f, "frame too large: {n} > {MAX_FRAME}"),
            FrameError::Io(e) => write!(f, "io error: {e}"),
            FrameError::Utf8 => write!(f, "invalid utf-8 payload"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Read exactly `buf.len()` bytes, distinguishing a clean boundary EOF (zero
/// bytes available) from a truncated read (some bytes, then EOF).
fn read_exact_or_eof<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<(), FrameError> {
    let mut filled = 0usize;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => {
                return Err(if filled == 0 {
                    FrameError::Eof
                } else {
                    FrameError::Truncated
                });
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(FrameError::Io(e)),
        }
    }
    Ok(())
}

/// Read one frame: a 4-byte big-endian length prefix followed by that many
/// UTF-8 bytes.
///
/// Returns [`FrameError::Eof`] only when the stream ends exactly at a frame
/// boundary (zero bytes of a new length prefix). A partial length prefix or a
/// short body is [`FrameError::Truncated`] (fail-closed), never a clean EOF.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<String, FrameError> {
    let mut len_buf = [0u8; 4];
    read_exact_or_eof(reader, &mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(len));
    }
    let mut buf = vec![0u8; len];
    read_exact_or_eof(reader, &mut buf)?;
    String::from_utf8(buf).map_err(|_| FrameError::Utf8)
}

/// Write one frame: length prefix + payload, then flush. Rejects payloads
/// larger than [`MAX_FRAME`] before casting the length to `u32`.
pub fn write_frame<W: Write>(writer: &mut W, payload: &str) -> Result<(), FrameError> {
    let bytes = payload.as_bytes();
    if bytes.len() > MAX_FRAME {
        return Err(FrameError::TooLarge(bytes.len()));
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .map_err(FrameError::Io)?;
    writer.write_all(bytes).map_err(FrameError::Io)?;
    writer.flush().map_err(FrameError::Io)
}

/// Canonical JSON: object keys sorted recursively, no insignificant
/// whitespace, UTF-8. This is the wire/golden **encoding** rule; see the module
/// docs for why it is not a receiving requirement.
pub fn canonical_json(value: &serde_json::Value) -> String {
    fn sort(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for k in keys {
                    out.insert(k.clone(), sort(&map[k]));
                }
                serde_json::Value::Object(out)
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(sort).collect())
            }
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(value)).expect("serde_json::Value always serialises")
}

/// Canonicalise `value` and return its framed bytes (length prefix + payload).
/// Returns [`FrameError::TooLarge`] if the canonical payload exceeds
/// [`MAX_FRAME`].
pub fn frame_bytes(value: &serde_json::Value) -> Result<Vec<u8>, FrameError> {
    let payload = canonical_json(value);
    if payload.len() > MAX_FRAME {
        return Err(FrameError::TooLarge(payload.len()));
    }
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload.as_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, "{\"a\":1}").unwrap();
        let mut cur = Cursor::new(buf);
        assert_eq!(read_frame(&mut cur).unwrap(), "{\"a\":1}");
        assert!(matches!(read_frame(&mut cur), Err(FrameError::Eof)));
    }

    #[test]
    fn canonical_sorts_keys_no_whitespace() {
        let v = serde_json::json!({"b": 1, "a": {"d": 2, "c": 3}});
        assert_eq!(canonical_json(&v), "{\"a\":{\"c\":3,\"d\":2},\"b\":1}");
    }

    #[test]
    fn oversized_length_rejected_on_read() {
        let mut bytes = (MAX_FRAME as u32 + 1).to_be_bytes().to_vec();
        bytes.push(b'x');
        let mut cur = Cursor::new(bytes);
        assert!(matches!(read_frame(&mut cur), Err(FrameError::TooLarge(_))));
    }

    #[test]
    fn partial_length_prefix_is_truncated_not_eof() {
        // 2 of 4 length bytes, then EOF.
        let mut cur = Cursor::new(vec![0u8, 0u8]);
        assert!(matches!(read_frame(&mut cur), Err(FrameError::Truncated)));
    }

    #[test]
    fn short_body_is_truncated() {
        // length says 5, only 2 body bytes follow.
        let mut bytes = 5u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"ab");
        let mut cur = Cursor::new(bytes);
        assert!(matches!(read_frame(&mut cur), Err(FrameError::Truncated)));
    }
}
