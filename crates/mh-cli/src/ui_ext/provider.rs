//! Framed, operator-owned read provider channel.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

#[cfg(unix)]
use std::process::Command;

use serde_json::{json, Map, Value};

use super::broker::{validate_bounded_text, ReadOperation};

pub(super) const WIRE_SCHEMA: &str = "mh-ui-read-provider.v1";
pub(super) const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct ProviderClient {
    #[cfg(unix)]
    stream: std::os::unix::net::UnixStream,
    generation: String,
    failed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProviderResponse {
    pub(super) generation: String,
    pub(super) payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProviderReadError {
    Rejected { generation: String, code: String },
    Channel(String),
}

impl ProviderClient {
    pub(super) fn connect(path: &Path) -> Result<Self, String> {
        #[cfg(unix)]
        {
            let before = validate_socket_path(path)?;
            let mut stream = std::os::unix::net::UnixStream::connect(path)
                .map_err(|error| format!("provider socket connect failed: {error}"))?;
            let after = validate_socket_path(path)?;
            if before != after {
                return Err("provider socket changed during connect".to_string());
            }
            stream
                .set_read_timeout(Some(PROVIDER_TIMEOUT))
                .map_err(|error| format!("provider socket timeout setup failed: {error}"))?;
            stream
                .set_write_timeout(Some(PROVIDER_TIMEOUT))
                .map_err(|error| format!("provider socket timeout setup failed: {error}"))?;
            let hello = read_frame(&mut stream)?;
            let generation = validate_hello(&hello)?;
            Ok(Self {
                stream,
                generation,
                failed: false,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err("the trusted UI provider requires Unix domain sockets".to_string())
        }
    }

    pub(super) fn read(
        &mut self,
        request_id: &str,
        operation: ReadOperation,
        payload: Value,
    ) -> Result<ProviderResponse, ProviderReadError> {
        if self.failed {
            return Err(ProviderReadError::Channel(
                "provider channel has failed; restart mh-ui-ext".to_string(),
            ));
        }
        if let Err(error) = validate_bounded_text(request_id, 64, "request_id") {
            self.failed = true;
            return Err(ProviderReadError::Channel(error));
        }
        if !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            self.failed = true;
            return Err(ProviderReadError::Channel(
                "request_id must match [A-Za-z0-9_-]{1,64}".to_string(),
            ));
        }
        let request = json!({
            "schema": WIRE_SCHEMA,
            "type": "read",
            "request_id": request_id,
            "generation": self.generation,
            "operation": operation.as_str(),
            "arguments": payload,
        });
        if let Err(error) = write_frame(&mut self.stream, &request) {
            self.failed = true;
            return Err(ProviderReadError::Channel(error));
        }
        let response = match read_frame(&mut self.stream) {
            Ok(response) => response,
            Err(error) => {
                self.failed = true;
                return Err(ProviderReadError::Channel(error));
            }
        };
        match validate_result(&response, request_id, &self.generation) {
            Ok(ValidatedResult::Success(response)) => Ok(response),
            Ok(ValidatedResult::Rejected(code)) => Err(ProviderReadError::Rejected {
                generation: self.generation.clone(),
                code,
            }),
            Err(error) => {
                self.failed = true;
                Err(ProviderReadError::Channel(error))
            }
        }
    }
}

pub(super) fn write_frame<W: Write>(writer: &mut W, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("provider JSON encode failed: {error}"))?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err("provider frame exceeds the 8 MiB limit".to_string());
    }
    let length = u32::try_from(bytes.len())
        .map_err(|_| "provider frame length does not fit in u32".to_string())?;
    writer
        .write_all(&length.to_be_bytes())
        .and_then(|_| writer.write_all(&bytes))
        .and_then(|()| writer.flush())
        .map_err(|error| format!("provider frame write failed: {error}"))
}

pub(super) fn read_frame<R: Read>(reader: &mut R) -> Result<Value, String> {
    let mut length_bytes = [0u8; 4];
    reader
        .read_exact(&mut length_bytes)
        .map_err(|error| format!("provider frame header read failed: {error}"))?;
    let length = u32::from_be_bytes(length_bytes) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err("provider frame length is invalid".to_string());
    }
    let mut bytes = vec![0u8; length];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| format!("provider frame body read failed: {error}"))?;
    let text = String::from_utf8(bytes).map_err(|_| "provider frame is not UTF-8".to_string())?;
    serde_json::from_str(&text).map_err(|error| format!("provider frame JSON is invalid: {error}"))
}

fn validate_hello(value: &Value) -> Result<String, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "provider hello must be a JSON object".to_string())?;
    require_exact_keys(object, &["schema", "type", "generation", "operations"])?;
    if object.get("schema").and_then(Value::as_str) != Some(WIRE_SCHEMA)
        || object.get("type").and_then(Value::as_str) != Some("hello")
    {
        return Err("provider hello schema or type mismatch".to_string());
    }
    let generation = object
        .get("generation")
        .and_then(Value::as_str)
        .ok_or_else(|| "provider hello generation must be a string".to_string())?;
    validate_bounded_text(generation, 4096, "generation")?;
    let operations = object
        .get("operations")
        .and_then(Value::as_array)
        .ok_or_else(|| "provider hello operations must be an array".to_string())?;
    let expected = [
        Value::String("gallery.list".to_string()),
        Value::String("graph.detail".to_string()),
    ];
    if operations != &expected {
        return Err(
            "provider hello operations must be exactly gallery.list and graph.detail".to_string(),
        );
    }
    Ok(generation.to_string())
}

#[derive(Debug, Clone, PartialEq)]
enum ValidatedResult {
    Success(ProviderResponse),
    Rejected(String),
}

fn validate_result(
    value: &Value,
    request_id: &str,
    generation: &str,
) -> Result<ValidatedResult, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "provider result must be a JSON object".to_string())?;
    if object.get("schema").and_then(Value::as_str) != Some(WIRE_SCHEMA)
        || object.get("type").and_then(Value::as_str) != Some("read-result")
    {
        return Err("provider result schema or type mismatch".to_string());
    }
    if object.get("request_id").and_then(Value::as_str) != Some(request_id) {
        return Err("provider result request_id mismatch".to_string());
    }
    if object.get("generation").and_then(Value::as_str) != Some(generation) {
        return Err("provider result generation mismatch".to_string());
    }
    match object.get("ok").and_then(Value::as_bool) {
        Some(true) => {
            require_exact_keys(
                object,
                &[
                    "schema",
                    "type",
                    "request_id",
                    "generation",
                    "ok",
                    "payload",
                ],
            )?;
            Ok(ValidatedResult::Success(ProviderResponse {
                generation: generation.to_string(),
                payload: object
                    .get("payload")
                    .cloned()
                    .ok_or_else(|| "provider result payload is required".to_string())?,
            }))
        }
        Some(false) => {
            require_exact_keys(
                object,
                &["schema", "type", "request_id", "generation", "ok", "error"],
            )?;
            let error = object
                .get("error")
                .and_then(Value::as_object)
                .ok_or_else(|| "provider result error must be an object".to_string())?;
            require_exact_keys(error, &["code"])?;
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .ok_or_else(|| "provider result error code must be a string".to_string())?;
            validate_bounded_text(code, 64, "provider error code")?;
            Ok(ValidatedResult::Rejected(code.to_string()))
        }
        None => Err("provider result ok must be a boolean".to_string()),
    }
}

fn require_exact_keys(object: &Map<String, Value>, required: &[&str]) -> Result<(), String> {
    if object.keys().any(|key| !required.contains(&key.as_str()))
        || required.iter().any(|key| !object.contains_key(*key))
    {
        return Err("provider message contains missing or unknown fields".to_string());
    }
    Ok(())
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SocketIdentity {
    dev: u64,
    ino: u64,
    uid: u32,
    mode: u32,
}

#[cfg(unix)]
fn validate_socket_path(path: &Path) -> Result<SocketIdentity, String> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

    if !path.is_absolute() {
        return Err("provider socket path must be absolute".to_string());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "provider socket must have a parent directory".to_string())?;
    let parent_link_metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("provider socket parent metadata failed: {error}"))?;
    if parent_link_metadata.file_type().is_symlink() || !parent_link_metadata.is_dir() {
        return Err("provider socket parent must be a real directory".to_string());
    }
    let parent_canonical = fs::canonicalize(parent)
        .map_err(|error| format!("provider socket parent cannot be resolved: {error}"))?;
    let parent_metadata = fs::metadata(&parent_canonical)
        .map_err(|error| format!("provider socket parent metadata failed: {error}"))?;
    let uid = current_euid()?;
    let parent_mode = parent_metadata.permissions().mode() & 0o777;
    if parent_metadata.uid() != uid || parent_mode & 0o077 != 0 {
        return Err("provider socket parent is not owner-controlled".to_string());
    }

    let link_metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("provider socket metadata failed: {error}"))?;
    if link_metadata.file_type().is_symlink() || !link_metadata.file_type().is_socket() {
        return Err("provider socket must be a Unix socket, not a symlink".to_string());
    }
    let mode = link_metadata.permissions().mode() & 0o777;
    if link_metadata.uid() != uid || mode & 0o077 != 0 {
        return Err("provider socket must be owner-only".to_string());
    }
    let canonical_socket = fs::canonicalize(path)
        .map_err(|error| format!("provider socket cannot be resolved: {error}"))?;
    if !canonical_socket.starts_with(&parent_canonical) {
        return Err("provider socket escapes its parent".to_string());
    }
    Ok(SocketIdentity {
        dev: link_metadata.dev(),
        ino: link_metadata.ino(),
        uid: link_metadata.uid(),
        mode,
    })
}

#[cfg(unix)]
fn current_euid() -> Result<u32, String> {
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        if let Some(uid) = status
            .lines()
            .find(|line| line.starts_with("Uid:"))
            .and_then(|line| line.split_whitespace().nth(2))
            .and_then(|value| value.parse::<u32>().ok())
        {
            return Ok(uid);
        }
    }
    let output = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .map_err(|error| format!("cannot determine effective uid: {error}"))?;
    if !output.status.success() {
        return Err("cannot determine effective uid".to_string());
    }
    String::from_utf8(output.stdout)
        .map_err(|_| "effective uid output is not UTF-8".to_string())?
        .trim()
        .parse::<u32>()
        .map_err(|_| "effective uid output is invalid".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn framing_is_big_endian_utf8_and_bounded() {
        let value = json!({"schema": WIRE_SCHEMA, "text": "日本語"});
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &value).unwrap();
        assert_eq!(
            u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize,
            bytes.len() - 4
        );
        assert_eq!(read_frame(&mut Cursor::new(bytes)).unwrap(), value);
        assert!(read_frame(&mut Cursor::new(vec![0, 0, 0, 0])).is_err());
        assert!(read_frame(&mut Cursor::new(vec![0, 0, 0, 4, b'{'])).is_err());
    }

    #[test]
    fn hello_and_result_are_generation_and_operation_bound() {
        let hello = json!({
            "schema": WIRE_SCHEMA,
            "type": "hello",
            "generation": "generation-1",
            "operations": ["gallery.list", "graph.detail"],
        });
        assert_eq!(validate_hello(&hello).unwrap(), "generation-1");
        let result = json!({
            "schema": WIRE_SCHEMA,
            "type": "read-result",
            "request_id": "request-1",
            "generation": "generation-1",
            "ok": true,
            "payload": {"opaque": true},
        });
        assert_eq!(
            validate_result(&result, "request-1", "generation-1").unwrap(),
            ValidatedResult::Success(ProviderResponse {
                generation: "generation-1".to_string(),
                payload: json!({"opaque": true}),
            })
        );
        let mut mismatched = result.clone();
        mismatched["generation"] = json!("generation-2");
        assert!(validate_result(&mismatched, "request-1", "generation-1").is_err());

        let rejected = json!({
            "schema": WIRE_SCHEMA,
            "type": "read-result",
            "request_id": "request-1",
            "generation": "generation-1",
            "ok": false,
            "error": {"code": "not_found"},
        });
        assert_eq!(
            validate_result(&rejected, "request-1", "generation-1").unwrap(),
            ValidatedResult::Rejected("not_found".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn provider_connects_only_to_owner_only_socket_and_uses_one_framed_channel() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixListener;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("mh-ui-ext-provider-{stamp}"));
        fs::create_dir_all(&directory).expect("mkdir");
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).expect("chmod dir");
        let socket_path = directory.join("provider.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind");
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600)).expect("chmod socket");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            write_frame(
                &mut stream,
                &json!({
                    "schema": WIRE_SCHEMA,
                    "type": "hello",
                    "generation": "generation-1",
                    "operations": ["gallery.list", "graph.detail"],
                }),
            )
            .expect("hello");
            let request = read_frame(&mut stream).expect("request");
            assert_eq!(request["type"], "read");
            assert_eq!(request["operation"], "gallery.list");
            assert_eq!(request["arguments"], json!({"limit": 1, "offset": 0}));
            assert_eq!(request["generation"], "generation-1");
            write_frame(
                &mut stream,
                &json!({
                    "schema": WIRE_SCHEMA,
                    "type": "read-result",
                    "request_id": request["request_id"],
                    "generation": "generation-1",
                    "ok": true,
                    "payload": {"opaque": ["value"]},
                }),
            )
            .expect("result");
        });
        let mut client = ProviderClient::connect(&socket_path).expect("connect");
        let response = client
            .read(
                "request-1",
                ReadOperation::GalleryList,
                json!({"limit": 1, "offset": 0}),
            )
            .expect("read");
        assert_eq!(response.generation, "generation-1");
        assert_eq!(response.payload, json!({"opaque": ["value"]}));
        server.join().expect("server");
    }

    #[cfg(unix)]
    #[test]
    fn provider_parent_and_socket_permissions_are_fail_closed() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixListener;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("mh-ui-ext-provider-mode-{stamp}"));
        fs::create_dir_all(&directory).expect("mkdir");
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).expect("chmod dir");
        let socket_path = directory.join("provider.sock");
        let _listener = UnixListener::bind(&socket_path).expect("bind");
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o666)).expect("chmod socket");
        assert!(ProviderClient::connect(&socket_path).is_err());
        let _ = fs::remove_file(&socket_path);
        let _ = fs::remove_dir(&directory);
    }
}
