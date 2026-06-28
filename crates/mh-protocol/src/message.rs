//! JSON-RPC 2.0 message builders and helpers used by host and plugins.
//!
//! Transport framing lives in [`crate::framing`]; this module is only about
//! message *semantics*. Protocol and record-schema versions are kept separate
//! so they can evolve independently.

use serde_json::{json, Value};

/// Wire protocol version negotiated in `initialize`.
pub const PROTOCOL_VERSION: i64 = 1;

/// Version of the `SourceRecord` shape carried in `record` notifications.
pub const RECORD_SCHEMA_VERSION: i64 = 1;

/// Maximum in-flight plugin->host requests the host will service at once.
pub const MAX_PENDING_REQUESTS: usize = 16;

/// Maximum records a single `record` batch may carry.
pub const MAX_RECORD_BATCH: usize = 100;

/// Methods exchanged over the protocol. `host->plugin` and `plugin->host`
/// directions share the JSON-RPC envelope; IDs are namespaced by the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Initialize,
    Discover,
    Cancel,
    FetchRequest,
    StateQuery,
    Record,
    Log,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Initialize => "initialize",
            Method::Discover => "discover",
            Method::Cancel => "cancel",
            Method::FetchRequest => "fetch_request",
            Method::StateQuery => "state_query",
            Method::Record => "record",
            Method::Log => "log",
        }
    }

    pub fn parse(s: &str) -> Option<Method> {
        Some(match s {
            "initialize" => Method::Initialize,
            "discover" => Method::Discover,
            "cancel" => Method::Cancel,
            "fetch_request" => Method::FetchRequest,
            "state_query" => Method::StateQuery,
            "record" => Method::Record,
            "log" => Method::Log,
            _ => return None,
        })
    }
}

/// A JSON-RPC request with an id.
pub fn request(id: &str, method: Method, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method.as_str(), "params": params})
}

/// A JSON-RPC notification (no id, no response expected).
pub fn notification(method: Method, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method.as_str(), "params": params})
}

/// A successful JSON-RPC response.
pub fn response_ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// An error JSON-RPC response.
pub fn response_err(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_round_trip() {
        for m in [
            Method::Initialize,
            Method::Discover,
            Method::Cancel,
            Method::FetchRequest,
            Method::StateQuery,
            Method::Record,
            Method::Log,
        ] {
            assert_eq!(Method::parse(m.as_str()), Some(m));
        }
        assert_eq!(Method::parse("nope"), None);
    }

    #[test]
    fn builders_shape() {
        let r = request("h-1", Method::Initialize, json!({"protocol_version": 1}));
        assert_eq!(r["jsonrpc"], "2.0");
        assert_eq!(r["id"], "h-1");
        assert_eq!(r["method"], "initialize");
        let n = notification(Method::Record, json!({}));
        assert!(n.get("id").is_none());
    }
}
