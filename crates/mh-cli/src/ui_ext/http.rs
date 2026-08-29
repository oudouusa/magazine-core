use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::broker::parse_browser_request;
use super::provider::{ProviderReadError, MAX_FRAME_BYTES};
use super::registry::RegistryOpenedAsset as OpenedAsset;
use super::shell::{read_route_prefix, render_shell};
use super::{is_loopback_authority, Options};

const MAX_REQUEST_HEADER_BYTES: usize = 64 * 1024;
const REQUEST_DEADLINE: Duration = Duration::from_secs(15);
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONCURRENT_CONNECTIONS: usize = 64;
const PERMISSIONS_POLICY: &str =
    "camera=(), microphone=(), geolocation=(), display-capture=(), fullscreen=(), payment=(), usb=(), serial=(), hid=()";
const ASSET_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'none'; img-src 'none'; font-src 'none'; media-src 'none'; object-src 'none'; worker-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerRole {
    Shell,
    Assets,
}

pub(super) fn serve(listener: TcpListener, options: Arc<Options>, role: ServerRole) {
    let active = Arc::new(AtomicUsize::new(0));
    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else {
            continue;
        };
        if active.load(Ordering::Acquire) >= MAX_CONCURRENT_CONNECTIONS {
            let _ =
                json_response(503, json!({"error": "too many connections"})).write_to(&mut stream);
            continue;
        }
        active.fetch_add(1, Ordering::AcqRel);
        let slot = ConnectionSlot(Arc::clone(&active));
        let options = Arc::clone(&options);
        let spawned = thread::Builder::new()
            .name("mh-ui-ext-http".to_string())
            .spawn(move || {
                let _slot = slot;
                if let Err(error) = serve_connection(&options, role, &mut stream) {
                    eprintln!("mh-ui-ext request error: {error}");
                }
            });
        if spawned.is_err() {
            // The slot is also released by the moved guard if thread creation
            // fails; there is no work to retry.
        }
    }
}

struct ConnectionSlot(Arc<AtomicUsize>);

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn serve_connection(options: &Options, role: ServerRole, stream: &mut TcpStream) -> io::Result<()> {
    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    let request = read_http_request(stream)?;
    let response = match request {
        Some(request) => handle_request(options, role, &request),
        None => json_response(400, json!({"error": "bad request"})),
    };
    response.write_to(stream)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpRequest {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
}

fn read_http_request(stream: &mut TcpStream) -> io::Result<Option<HttpRequest>> {
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        stream.set_read_timeout(Some(remaining.min(SOCKET_TIMEOUT)))?;
        let read = match stream.read(&mut chunk) {
            Ok(read) => read,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_REQUEST_HEADER_BYTES {
            return Ok(None);
        }
    };
    let header = std::str::from_utf8(&buffer[..header_end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request headers are not UTF-8"))?;
    let mut lines = header.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request line"))?;
    let fields = request_line.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 3 || !matches!(fields[2], "HTTP/1.0" | "HTTP/1.1") {
        return Ok(None);
    }
    let method = fields[0].to_string();
    let target = fields[1].to_string();
    if target.is_empty() || !target.starts_with('/') || target.contains('#') {
        return Ok(None);
    }
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Ok(None);
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value.chars().any(char::is_control)
            || headers.insert(name.clone(), value.to_string()).is_some()
        {
            return Ok(None);
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Ok(None);
    }
    if headers
        .get("content-length")
        .is_some_and(|value| value != "0")
    {
        return Ok(None);
    }
    Ok(Some(HttpRequest {
        method,
        target,
        headers,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpResponse {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
    headers: Vec<(&'static str, String)>,
}

impl HttpResponse {
    fn write_to(&self, stream: &mut TcpStream) -> io::Result<()> {
        write!(
            stream,
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n",
            self.status,
            self.reason(),
            self.content_type,
            self.body.len()
        )?;
        for (name, value) in &self.headers {
            write!(stream, "{name}: {value}\r\n")?;
        }
        write!(stream, "\r\n")?;
        stream.write_all(&self.body)
    }

    fn reason(&self) -> &'static str {
        match self.status {
            200 => "OK",
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Payload Too Large",
            500 => "Internal Server Error",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Error",
        }
    }

    fn without_body(mut self) -> Self {
        self.body.clear();
        self
    }

    fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

fn json_response(status: u16, value: Value) -> HttpResponse {
    HttpResponse {
        status,
        reason: "",
        content_type: "application/json; charset=utf-8",
        body: serde_json::to_vec(&value).unwrap_or_else(|_| b"{\"error\":\"json\"}".to_vec()),
        headers: Vec::new(),
    }
}

fn handle_request(options: &Options, role: ServerRole, request: &HttpRequest) -> HttpResponse {
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return json_response(405, json!({"error": "method not allowed"}))
            .header("Allow", "GET, HEAD");
    }
    let Some(host) = request.headers.get("host") else {
        return json_response(400, json!({"error": "Host header is required"}));
    };
    let port = match role {
        ServerRole::Shell => options.shell_port,
        ServerRole::Assets => options.asset_port,
    };
    if !is_loopback_authority(host, port) {
        return json_response(403, json!({"error": "invalid Host header"}));
    }
    let (path, query) = split_target(&request.target);
    let mut response = match role {
        ServerRole::Shell => handle_shell(options, path, query, request.method == "HEAD"),
        ServerRole::Assets => handle_asset(options, path, request.method == "HEAD"),
    };
    if request.method == "HEAD" {
        response = response.without_body();
    }
    response
}

fn handle_shell(options: &Options, path: &str, query: Option<&str>, head: bool) -> HttpResponse {
    if path == "/" {
        return match render_shell(
            &options.registry,
            &options.shell_origin(),
            &options.asset_origin(),
        ) {
            Ok(page) => HttpResponse {
                status: 200,
                reason: "",
                content_type: "text/html; charset=utf-8",
                body: page.bytes,
                headers: vec![
                    ("Content-Security-Policy", page.csp),
                    ("Permissions-Policy", PERMISSIONS_POLICY.to_string()),
                    ("Referrer-Policy", "no-referrer".to_string()),
                    ("X-DNS-Prefetch-Control", "off".to_string()),
                    ("Cross-Origin-Resource-Policy", "same-origin".to_string()),
                ],
            },
            Err(_) => json_response(500, json!({"error": "shell render failed"})),
        };
    }
    if let Some(name) = path.strip_prefix(read_route_prefix()) {
        return read_response(options, name, query, head);
    }
    json_response(404, json!({"error": "not found"}))
}

fn handle_asset(options: &Options, path: &str, head: bool) -> HttpResponse {
    let Some(rest) = path.strip_prefix("/extensions/") else {
        return json_response(404, json!({"error": "not found"}));
    };
    let Some((name, requested)) = rest.split_once('/') else {
        return json_response(400, json!({"error": "asset path is incomplete"}));
    };
    if name.is_empty() || name.contains('%') || name.contains('.') || name.contains('\\') {
        return json_response(400, json!({"error": "invalid extension name"}));
    }
    let asset = match options.registry.asset(name, requested) {
        Ok(asset) => asset,
        Err(error) if error.contains("size limit") => {
            return json_response(413, json!({"error": "asset too large"}));
        }
        Err(_) => return json_response(404, json!({"error": "asset not found"})),
    };
    asset_response(options, asset, head)
}

fn asset_response(options: &Options, asset: OpenedAsset, _head: bool) -> HttpResponse {
    HttpResponse {
        status: 200,
        reason: "",
        content_type: asset.content_type,
        body: asset.bytes,
        headers: vec![
            ("Permissions-Policy", PERMISSIONS_POLICY.to_string()),
            ("Referrer-Policy", "no-referrer".to_string()),
            ("X-DNS-Prefetch-Control", "off".to_string()),
            ("Cross-Origin-Resource-Policy", "cross-origin".to_string()),
            (
                "Content-Security-Policy",
                format!("{ASSET_CSP}; frame-ancestors {}", options.shell_origin()),
            ),
        ],
    }
}

fn read_response(options: &Options, name: &str, query: Option<&str>, head: bool) -> HttpResponse {
    if name.is_empty() || name.contains('/') || name.contains('%') {
        return json_response(400, json!({"error": "invalid extension name"}));
    }
    if options.registry.get(name).is_none() {
        return json_response(404, json!({"error": "unknown extension"}));
    }
    let Some(query) = query else {
        return json_response(400, json!({"error": "read query is required"}));
    };
    let values = match parse_query(query) {
        Ok(values) => values,
        Err(error) => return json_response(400, json!({"error": error})),
    };
    let Some(request_id) = values.get("request_id") else {
        return json_response(400, json!({"error": "request_id is required"}));
    };
    let Some(op) = values.get("op") else {
        return json_response(400, json!({"error": "op is required"}));
    };
    let Some(payload_text) = values.get("payload") else {
        return json_response(400, json!({"error": "payload is required"}));
    };
    let payload: Value = match serde_json::from_str(payload_text) {
        Ok(payload) => payload,
        Err(_) => return json_response(400, json!({"error": "payload is not JSON"})),
    };
    let message = json!({
        "channel": super::broker::BROKER_CHANNEL,
        "type": "read",
        "request_id": request_id,
        "op": op,
        "payload": payload,
    });
    let request = match parse_browser_request(&message) {
        Ok(request) => request,
        Err(error) => return json_response(400, json!({"error": error})),
    };
    if head {
        // HEAD is a route probe and must not consume the one-shot provider
        // channel. The GET route carries the actual opaque payload.
        return json_response(
            200,
            json!({
                "request_id": request.request_id,
                "op": request.op.as_str(),
                "ok": false,
            }),
        );
    }
    let response = match options.provider.lock() {
        Ok(mut provider) => provider.read(&request.request_id, request.op, request.payload),
        Err(_) => Err(ProviderReadError::Channel(
            "provider lock failed".to_string(),
        )),
    };
    let response = match response {
        Ok(response) => response,
        Err(ProviderReadError::Rejected { generation, code }) => {
            return json_response(
                200,
                json!({
                    "request_id": request.request_id,
                    "op": request.op.as_str(),
                    "generation": generation,
                    "ok": false,
                    "error": {"code": code},
                }),
            );
        }
        Err(ProviderReadError::Channel(_)) => {
            return json_response(502, json!({"error": "provider read failed"}));
        }
    };
    let result = json!({
        "request_id": request.request_id,
        "op": request.op.as_str(),
        "generation": response.generation,
        "ok": true,
        "payload": response.payload,
    });
    let bytes = serde_json::to_vec(&result).unwrap_or_default();
    if bytes.len() > MAX_FRAME_BYTES {
        return json_response(
            502,
            json!({
                "request_id": request.request_id,
                "op": request.op.as_str(),
                "ok": false,
                "error": "response exceeds 8 MiB",
            }),
        );
    }
    HttpResponse {
        status: 200,
        reason: "",
        content_type: "application/json; charset=utf-8",
        body: bytes,
        headers: Vec::new(),
    }
}

fn split_target(target: &str) -> (&str, Option<&str>) {
    target
        .split_once('?')
        .map_or((target, None), |(path, query)| (path, Some(query)))
}

fn parse_query(query: &str) -> Result<BTreeMap<String, String>, String> {
    if query.is_empty() {
        return Err("read query is empty".to_string());
    }
    let mut values = BTreeMap::new();
    for pair in query.split('&') {
        let (raw_key, raw_value) = pair
            .split_once('=')
            .ok_or_else(|| "malformed read query".to_string())?;
        let key = decode_query_component(raw_key)?;
        let value = decode_query_component(raw_value)?;
        if !matches!(key.as_str(), "request_id" | "op" | "payload") {
            return Err("read query contains an unknown field".to_string());
        }
        if values.insert(key, value).is_some() {
            return Err("read query contains a duplicate field".to_string());
        }
    }
    Ok(values)
}

fn decode_query_component(value: &str) -> Result<String, String> {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = hex(bytes[index + 1])
                    .ok_or_else(|| "invalid percent escape in read query".to_string())?;
                let low = hex(bytes[index + 2])
                    .ok_or_else(|| "invalid percent escape in read query".to_string())?;
                decoded.push((high << 4) | low);
                index += 3;
            }
            b'%' => return Err("truncated percent escape in read query".to_string()),
            byte if byte.is_ascii() => {
                decoded.push(byte);
                index += 1;
            }
            _ => return Err("read query is not ASCII encoded".to_string()),
        }
    }
    String::from_utf8(decoded).map_err(|_| "read query is not valid UTF-8".to_string())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parser_rejects_duplicates_and_unknown_fields() {
        assert!(parse_query("request_id=a&request_id=b").is_err());
        assert!(parse_query("request_id=a&sql=x").is_err());
        assert_eq!(
            parse_query("request_id=req%2Done&op=graph.detail&payload=%7B%7D")
                .unwrap()
                .get("request_id")
                .map(String::as_str),
            Some("req-one")
        );
        assert_eq!(
            parse_query("request_id=req&op=graph.detail&payload=%7B%22work_key%22%3A%22%E6%97%A5%E6%9C%AC%E8%AA%9E%22%7D")
                .unwrap()
                .get("payload")
                .map(String::as_str),
            Some(r#"{"work_key":"日本語"}"#)
        );
    }
}
