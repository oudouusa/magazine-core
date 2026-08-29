//! The fixed browser-to-shell broker contract.

use std::collections::BTreeSet;

use serde_json::Value;

pub(super) const BROKER_CHANNEL: &str = "mh-ui-read-broker.v1";
pub(super) const MAX_BROWSER_MESSAGE_BYTES: usize = 16 * 1024;
pub(super) const MAX_IFRAME_CONCURRENT_REQUESTS: usize = 8;
pub(super) const MAX_REQUEST_ID_BYTES: usize = 64;
pub(super) const MAX_WORK_KEY_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct BrowserReadRequest {
    pub(super) request_id: String,
    pub(super) op: ReadOperation,
    pub(super) payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReadOperation {
    GalleryList,
    GraphDetail,
}

impl ReadOperation {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::GalleryList => "gallery.list",
            Self::GraphDetail => "graph.detail",
        }
    }
}

/// Parse the object posted by an extension. The extension name is deliberately
/// absent: the caller resolves it from the registered iframe window.
pub(super) fn parse_browser_request(value: &Value) -> Result<BrowserReadRequest, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "broker message must be a JSON object".to_string())?;
    require_exact_keys(object, &["channel", "type", "request_id", "op", "payload"])?;
    if object.get("channel").and_then(Value::as_str) != Some(BROKER_CHANNEL) {
        return Err("broker channel mismatch".to_string());
    }
    if object.get("type").and_then(Value::as_str) != Some("read") {
        return Err("broker message type is not read".to_string());
    }
    let request_id = object
        .get("request_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "broker request_id must be a string".to_string())?;
    validate_bounded_text(request_id, MAX_REQUEST_ID_BYTES, "request_id")?;
    if !request_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("request_id must match [A-Za-z0-9_-]{1,64}".to_string());
    }
    let op = match object.get("op").and_then(Value::as_str) {
        Some("gallery.list") => ReadOperation::GalleryList,
        Some("graph.detail") => ReadOperation::GraphDetail,
        _ => return Err("broker operation is not allowed".to_string()),
    };
    let payload = object
        .get("payload")
        .ok_or_else(|| "broker payload is required".to_string())?
        .clone();
    validate_payload(op, &payload)?;
    let encoded =
        serde_json::to_vec(value).map_err(|_| "broker message is not JSON".to_string())?;
    if encoded.len() > MAX_BROWSER_MESSAGE_BYTES {
        return Err("broker message exceeds 16 KiB".to_string());
    }
    Ok(BrowserReadRequest {
        request_id: request_id.to_string(),
        op,
        payload,
    })
}

pub(super) fn validate_payload(op: ReadOperation, payload: &Value) -> Result<(), String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "broker payload must be a JSON object".to_string())?;
    match op {
        ReadOperation::GalleryList => {
            require_exact_keys(object, &["limit", "offset"])?;
            let limit = object
                .get("limit")
                .and_then(Value::as_i64)
                .ok_or_else(|| "gallery.list limit must be an integer".to_string())?;
            if !(1..=200).contains(&limit) {
                return Err("gallery.list limit must be between 1 and 200".to_string());
            }
            let offset = object
                .get("offset")
                .and_then(Value::as_i64)
                .ok_or_else(|| "gallery.list offset must be a signed 64-bit integer".to_string())?;
            if offset < 0 {
                return Err("gallery.list offset must not be negative".to_string());
            }
        }
        ReadOperation::GraphDetail => {
            require_exact_keys(object, &["work_key"])?;
            let work_key = object
                .get("work_key")
                .and_then(Value::as_str)
                .ok_or_else(|| "graph.detail work_key must be a string".to_string())?;
            validate_bounded_text(work_key, MAX_WORK_KEY_BYTES, "work_key")?;
        }
    }
    Ok(())
}

pub(super) fn validate_bounded_text(
    value: &str,
    max_bytes: usize,
    field: &str,
) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    if value.len() > max_bytes {
        return Err(format!("{field} exceeds {max_bytes} UTF-8 bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} contains a control character"));
    }
    Ok(())
}

fn require_exact_keys(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<(), String> {
    let allowed = allowed.iter().copied().collect::<BTreeSet<_>>();
    if object.keys().any(|key| !allowed.contains(key.as_str()))
        || allowed.iter().any(|key| !object.contains_key(*key))
    {
        return Err("broker message contains missing or unknown fields".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(op: &str, payload: Value) -> Value {
        json!({
            "channel": BROKER_CHANNEL,
            "type": "read",
            "request_id": "req-1",
            "op": op,
            "payload": payload,
        })
    }

    #[test]
    fn broker_accepts_only_the_two_bounded_operations() {
        assert_eq!(
            parse_browser_request(&request("gallery.list", json!({"limit": 200, "offset": 0})))
                .unwrap()
                .op,
            ReadOperation::GalleryList
        );
        assert_eq!(
            parse_browser_request(&request("graph.detail", json!({"work_key": "opaque-key"})))
                .unwrap()
                .op,
            ReadOperation::GraphDetail
        );
        for value in [
            request("gallery.list", json!({"limit": 0, "offset": 0})),
            request("gallery.list", json!({"limit": 201, "offset": 0})),
            request("gallery.list", json!({"limit": 1, "offset": -1})),
            request("gallery.list", json!({"limit": 1, "offset": 1, "sql": "x"})),
            request("graph.detail", json!({"work_key": "   "})),
            request("graph.detail", json!({"work_key": "x", "sql": "x"})),
            request("other", json!({})),
        ] {
            assert!(parse_browser_request(&value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn broker_rejects_large_or_control_text() {
        let large = request("graph.detail", json!({"work_key": "a".repeat(4097)}));
        assert!(parse_browser_request(&large).is_err());
        assert!(validate_bounded_text("a\n", 4096, "work_key").is_err());
        let bad_request_id = json!({
            "channel": BROKER_CHANNEL,
            "type": "read",
            "request_id": "not allowed",
            "op": "gallery.list",
            "payload": {"limit": 1, "offset": 0},
        });
        assert!(parse_browser_request(&bad_request_id).is_err());
        let oversized = request("graph.detail", json!({"work_key": "a".repeat(4096)}));
        assert!(
            serde_json::to_vec(&oversized).unwrap().len() <= MAX_BROWSER_MESSAGE_BYTES,
            "fixture should isolate the field limit from the message limit"
        );
        assert!(parse_browser_request(&oversized).is_ok());
    }
}
