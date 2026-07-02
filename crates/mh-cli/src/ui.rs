use std::error::Error;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::PathBuf;

use mh_db::Database;
use mh_host::inspect_plugin_manifests;
use serde_json::{json, Value};

const DEFAULT_UI_PORT: u16 = 8765;
const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UiOptions {
    db_path: PathBuf,
    plugins_dir: PathBuf,
    port: u16,
}

pub(crate) fn parse_ui_options(args: &[String]) -> Result<UiOptions, Box<dyn Error>> {
    let mut db_path = None;
    let mut plugins_dir = None;
    let mut port = DEFAULT_UI_PORT;
    let mut port_seen = false;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        if matches!(flag, "--host" | "--bind" | "--manage") {
            return Err(format!("{flag} is not accepted by the read-only v1 UI").into());
        }
        let Some(value) = args.get(index + 1) else {
            return Err(format!("{flag} requires a value").into());
        };
        match flag {
            "--db" => {
                if db_path.replace(PathBuf::from(value)).is_some() {
                    return Err("--db specified more than once".into());
                }
            }
            "--plugins-dir" => {
                if plugins_dir.replace(PathBuf::from(value)).is_some() {
                    return Err("--plugins-dir specified more than once".into());
                }
            }
            "--port" => {
                if port_seen {
                    return Err("--port specified more than once".into());
                }
                port_seen = true;
                port = value
                    .parse::<u16>()
                    .map_err(|_| -> Box<dyn Error> { "--port must be a TCP port".into() })?;
            }
            _ => return Err(format!("unknown ui option: {flag}").into()),
        }
        index += 2;
    }
    Ok(UiOptions {
        db_path: db_path.ok_or("--db is required")?,
        plugins_dir: plugins_dir.ok_or("--plugins-dir is required")?,
        port,
    })
}

pub(crate) fn run_ui(options: UiOptions) -> Result<(), Box<dyn Error>> {
    let listener = bind_ui_listener(options.port)?;
    let address = listener.local_addr()?;
    println!("mh ui listening on http://{address}");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                if let Err(err) = serve_connection(&options, &mut stream) {
                    eprintln!("ui request error: {err}");
                }
            }
            Err(err) => eprintln!("ui accept error: {err}"),
        }
    }
    Ok(())
}

fn bind_ui_listener(port: u16) -> io::Result<TcpListener> {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

fn serve_connection(options: &UiOptions, stream: &mut TcpStream) -> io::Result<()> {
    let request = read_http_request(stream)?;
    let response = match request {
        Some(request) => handle_request(options, &request.method, &request.target),
        None => HttpResponse::new(
            400,
            "Bad Request",
            "application/json",
            json_bytes(json!({
                "error": "bad request"
            })),
        ),
    };
    response.write_to(stream)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpRequest {
    method: String,
    target: String,
}

fn read_http_request(stream: &mut TcpStream) -> io::Result<Option<HttpRequest>> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buffer.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_REQUEST_HEADER_BYTES {
            return Ok(None);
        }
    }
    let text = String::from_utf8_lossy(&buffer);
    let Some(line) = text.lines().next() else {
        return Ok(None);
    };
    let mut parts = line.split_whitespace();
    let Some(method) = parts.next() else {
        return Ok(None);
    };
    let Some(target) = parts.next() else {
        return Ok(None);
    };
    let Some(version) = parts.next() else {
        return Ok(None);
    };
    if !version.starts_with("HTTP/") || parts.next().is_some() {
        return Ok(None);
    }
    Ok(Some(HttpRequest {
        method: method.to_string(),
        target: target.to_string(),
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpResponse {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
    extra_headers: Vec<(&'static str, String)>,
}

impl HttpResponse {
    fn new(status: u16, reason: &'static str, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            reason,
            content_type,
            body,
            extra_headers: Vec::new(),
        }
    }

    fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.extra_headers.push((name, value.into()));
        self
    }

    fn without_body(mut self) -> Self {
        self.body.clear();
        self
    }

    fn write_to(&self, stream: &mut TcpStream) -> io::Result<()> {
        write!(
            stream,
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n",
            self.status,
            self.reason,
            self.content_type,
            self.body.len()
        )?;
        for (name, value) in &self.extra_headers {
            write!(stream, "{name}: {value}\r\n")?;
        }
        write!(stream, "\r\n")?;
        stream.write_all(&self.body)
    }
}

fn handle_request(options: &UiOptions, method: &str, target: &str) -> HttpResponse {
    if !matches!(method, "GET" | "HEAD") {
        return HttpResponse::new(
            405,
            "Method Not Allowed",
            "application/json",
            json_bytes(json!({"error": "method not allowed"})),
        )
        .with_header("Allow", "GET, HEAD");
    }
    let head = method == "HEAD";
    let (path, query) = split_target(target);
    let mut response = match path {
        "/" => html_response(INDEX_HTML),
        "/api/summary" => summary_response(options),
        "/api/records" => records_response(options, query),
        "/api/state/known-source-urls" => known_source_urls_response(options, query),
        "/api/plugins" => plugins_response(options),
        _ => HttpResponse::new(
            404,
            "Not Found",
            "application/json",
            json_bytes(json!({"error": "not found"})),
        ),
    };
    if head {
        response = response.without_body();
    }
    response
}

fn summary_response(options: &UiOptions) -> HttpResponse {
    match Database::open_read_only(&options.db_path).and_then(|db| db.ui_summary()) {
        Ok(summary) => json_response(200, json!(summary)),
        Err(_) => HttpResponse::new(
            500,
            "Internal Server Error",
            "application/json",
            json_bytes(json!({"error": "database read failed"})),
        ),
    }
}

fn records_response(options: &UiOptions, query: Option<&str>) -> HttpResponse {
    let limit = query_u32(query, "limit").unwrap_or(50).clamp(1, 200);
    let offset = query_u32(query, "offset").unwrap_or(0);
    match Database::open_read_only(&options.db_path)
        .and_then(|db| db.source_record_page(limit, offset))
    {
        Ok(page) => json_response(200, json!(page)),
        Err(_) => HttpResponse::new(
            500,
            "Internal Server Error",
            "application/json",
            json_bytes(json!({"error": "database read failed"})),
        ),
    }
}

fn known_source_urls_response(options: &UiOptions, query: Option<&str>) -> HttpResponse {
    let source_name = query_value(query, "source_name");
    match Database::open_read_only(&options.db_path)
        .and_then(|db| db.known_source_url_state(source_name.as_deref()))
    {
        Ok(state) => json_response(200, json!(state)),
        Err(_) => HttpResponse::new(
            500,
            "Internal Server Error",
            "application/json",
            json_bytes(json!({"error": "database read failed"})),
        ),
    }
}

fn plugins_response(options: &UiOptions) -> HttpResponse {
    match inspect_plugin_manifests(&options.plugins_dir) {
        Ok(plugins) => json_response(200, json!({"plugins": plugins})),
        Err(_) => HttpResponse::new(
            500,
            "Internal Server Error",
            "application/json",
            json_bytes(json!({"error": "plugin manifest read failed"})),
        ),
    }
}

fn html_response(html: &str) -> HttpResponse {
    HttpResponse::new(
        200,
        "OK",
        "text/html; charset=utf-8",
        html.as_bytes().to_vec(),
    )
}

fn json_response(status: u16, value: Value) -> HttpResponse {
    let reason = match status {
        200 => "OK",
        _ => "OK",
    };
    HttpResponse::new(
        status,
        reason,
        "application/json; charset=utf-8",
        json_bytes(value),
    )
}

fn json_bytes(value: Value) -> Vec<u8> {
    serde_json::to_vec(&value).unwrap_or_else(|_| b"{\"error\":\"json encode failed\"}".to_vec())
}

fn split_target(target: &str) -> (&str, Option<&str>) {
    let target = target.split('#').next().unwrap_or(target);
    match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    }
}

fn query_u32(query: Option<&str>, key: &str) -> Option<u32> {
    query_value(query, key)?.parse().ok()
}

fn query_value(query: Option<&str>, key: &str) -> Option<String> {
    let query = query?;
    query.split('&').find_map(|part| {
        let (candidate, value) = part.split_once('=')?;
        if percent_decode(candidate).ok()?.as_str() == key {
            percent_decode(value).ok()
        } else {
            None
        }
    })
}

fn percent_decode(value: &str) -> Result<String, ()> {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = hex(bytes[index + 1])?;
                let low = hex(bytes[index + 2])?;
                out.push((high << 4) | low);
                index += 3;
            }
            b'%' => return Err(()),
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

fn hex(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(()),
    }
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>magazine-core</title>
<style>
:root {
  color-scheme: light;
  --bg: #f7f8fa;
  --panel: #ffffff;
  --ink: #182026;
  --muted: #62707c;
  --line: #d6dde3;
  --accent: #0f766e;
  --warn: #a16207;
  --bad: #b42318;
}
* { box-sizing: border-box; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--ink);
  font: 14px/1.5 system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
header, main { max-width: 1180px; margin: 0 auto; padding: 18px; }
header { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
h1 { margin: 0; font-size: 22px; font-weight: 650; letter-spacing: 0; }
h2 { margin: 0 0 10px; font-size: 15px; font-weight: 650; letter-spacing: 0; }
.status { color: var(--muted); font-size: 13px; }
.grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; }
.metric, section { background: var(--panel); border: 1px solid var(--line); border-radius: 8px; }
.metric { padding: 12px; min-height: 78px; }
.metric b { display: block; font-size: 24px; line-height: 1.1; }
.metric span, th { color: var(--muted); font-size: 12px; font-weight: 600; text-transform: uppercase; }
main { display: grid; gap: 14px; }
section { padding: 14px; overflow: hidden; }
table { width: 100%; border-collapse: collapse; }
th, td { border-bottom: 1px solid var(--line); padding: 8px; text-align: left; vertical-align: top; }
td { word-break: break-word; }
code { color: var(--accent); font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
.pill { display: inline-block; border: 1px solid var(--line); border-radius: 999px; padding: 1px 7px; margin: 1px 3px 1px 0; color: var(--muted); }
.invalid { color: var(--bad); }
.valid { color: var(--accent); }
.list { display: grid; gap: 4px; }
.muted { color: var(--muted); }
@media (max-width: 760px) {
  header { display: block; }
  .grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  th:nth-child(4), td:nth-child(4) { display: none; }
}
</style>
</head>
<body>
<header>
  <h1>magazine-core</h1>
  <div id="status" class="status">loading</div>
</header>
<main>
  <div class="grid" id="metrics"></div>
  <section>
    <h2>Sources</h2>
    <table><thead><tr><th>source</th><th>records</th><th>last seen</th></tr></thead><tbody id="sources"></tbody></table>
  </section>
  <section>
    <h2>Records</h2>
    <table><thead><tr><th>source</th><th>title</th><th>assets</th><th>links</th></tr></thead><tbody id="records"></tbody></table>
  </section>
  <section>
    <h2>Known source URLs</h2>
    <div id="known" class="list"></div>
  </section>
  <section>
    <h2>Plugins</h2>
    <table><thead><tr><th>manifest</th><th>id</th><th>argv</th><th>status</th></tr></thead><tbody id="plugins"></tbody></table>
  </section>
</main>
<script>
const text = (value) => value === null || value === undefined || value === "" ? "-" : String(value);
const cell = (value) => {
  const td = document.createElement("td");
  td.textContent = text(value);
  return td;
};
const metric = (label, value) => {
  const node = document.createElement("div");
  node.className = "metric";
  const labelNode = document.createElement("span");
  labelNode.textContent = label;
  const valueNode = document.createElement("b");
  valueNode.textContent = text(value);
  node.append(labelNode, valueNode);
  return node;
};
async function fetchJson(path) {
  const response = await fetch(path, {cache: "no-store"});
  if (!response.ok) throw new Error(path + " " + response.status);
  return response.json();
}
function renderSummary(summary) {
  const metrics = document.getElementById("metrics");
  metrics.replaceChildren(
    metric("records", summary.inspection.source_posts),
    metric("sources", summary.sources.length),
    metric("covers", summary.inspection.covers),
    metric("pages", summary.inspection.pages)
  );
  const body = document.getElementById("sources");
  body.replaceChildren(...summary.sources.map((source) => {
    const tr = document.createElement("tr");
    tr.append(cell(source.source_name), cell(source.records), cell(source.last_seen_at));
    return tr;
  }));
}
function assetText(record) {
  return [
    record.performers_raw.length + " performers",
    record.cover_urls.length + " covers",
    record.page_urls.length + " pages"
  ].join(" / ");
}
function renderRecords(page) {
  const body = document.getElementById("records");
  body.replaceChildren(...page.records.map((record) => {
    const tr = document.createElement("tr");
    const title = document.createElement("td");
    const titleText = document.createElement("div");
    titleText.textContent = record.title;
    const url = document.createElement("code");
    url.textContent = record.source_url;
    title.append(titleText, url);
    const links = document.createElement("td");
    if (record.external_links.length === 0) links.textContent = "-";
    for (const link of record.external_links) {
      const item = document.createElement("div");
      item.textContent = [link.provider, link.kind, link.url].filter(Boolean).join(" ");
      links.append(item);
    }
    tr.append(cell(record.source_name), title, cell(assetText(record)), links);
    return tr;
  }));
}
function renderKnown(groups) {
  const known = document.getElementById("known");
  known.replaceChildren(...groups.map((group) => {
    const row = document.createElement("div");
    const name = document.createElement("b");
    name.textContent = group.source_name;
    const count = document.createElement("span");
    count.className = "pill";
    count.textContent = group.urls.length + " urls";
    row.append(name, " ", count);
    return row;
  }));
}
function renderPlugins(payload) {
  const body = document.getElementById("plugins");
  body.replaceChildren(...payload.plugins.map((plugin) => {
    const tr = document.createElement("tr");
    const status = document.createElement("td");
    status.className = plugin.status === "valid" ? "valid" : "invalid";
    status.textContent = plugin.status;
    if (plugin.errors.length) {
      const details = document.createElement("div");
      details.className = "muted";
      details.textContent = plugin.errors.join("; ");
      status.append(details);
    }
    tr.append(cell(plugin.file_name), cell(plugin.id), cell(plugin.argv.join(" ")), status);
    return tr;
  }));
}
async function boot() {
  const [summary, records, known, plugins] = await Promise.all([
    fetchJson("/api/summary"),
    fetchJson("/api/records?limit=50"),
    fetchJson("/api/state/known-source-urls"),
    fetchJson("/api/plugins")
  ]);
  renderSummary(summary);
  renderRecords(records);
  renderKnown(known);
  renderPlugins(plugins);
  document.getElementById("status").textContent = "read-only";
}
boot().catch((error) => {
  document.getElementById("status").textContent = error.message;
});
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use mh_domain::{ExternalLink, SourceRecord};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mh-ui-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record() -> SourceRecord {
        SourceRecord {
            source_name: "synthetic".to_string(),
            source_url: "synthetic://post/1".to_string(),
            title: "Synthetic One".to_string(),
            brand_raw: "Synthetic Brand".to_string(),
            performers_raw: vec!["Alice".to_string()],
            cover_urls: vec!["https://example.test/cover.jpg".to_string()],
            page_urls: vec!["https://example.test/page/1".to_string()],
            external_links: vec![ExternalLink {
                url: "https://retail.example.test/item/1".to_string(),
                provider: Some("retail".to_string()),
                label: Some("Retail".to_string()),
                kind: Some("retail".to_string()),
                external_id: Some("R1".to_string()),
                metadata: serde_json::Map::new(),
            }],
            issue_no: None,
            release_date: Some("2026-07-02".to_string()),
            post_date: None,
            brand_normalized: None,
            normalizer_id: None,
            normalizer_version: None,
            extra: serde_json::Map::new(),
        }
    }

    fn fixture() -> (PathBuf, UiOptions) {
        let dir = temp_dir("fixture");
        let db_path = dir.join("core.db");
        let plugins_dir = dir.join("plugins.d");
        fs::create_dir(&plugins_dir).unwrap();
        let mut db = Database::open(&db_path).unwrap();
        db.initialize().unwrap();
        db.ingest_records(&[record()]).unwrap();
        drop(db);
        fs::write(
            plugins_dir.join("synthetic.json"),
            serde_json::to_string_pretty(&json!({
                "id": "synthetic",
                "argv": ["python3", dir.join("private").join("plugin.py").to_string_lossy().to_string()],
                "env": {"API_TOKEN": "secret"}
            }))
            .unwrap(),
        )
        .unwrap();
        let options = UiOptions {
            db_path,
            plugins_dir,
            port: 0,
        };
        (dir, options)
    }

    #[test]
    fn parse_ui_options_accepts_read_only_shape_only() {
        let options = parse_ui_options(&[
            "--db".to_string(),
            "core.db".to_string(),
            "--plugins-dir".to_string(),
            "plugins.d".to_string(),
            "--port".to_string(),
            "0".to_string(),
        ])
        .unwrap();

        assert_eq!(options.db_path, PathBuf::from("core.db"));
        assert_eq!(options.plugins_dir, PathBuf::from("plugins.d"));
        assert_eq!(options.port, 0);
        assert!(parse_ui_options(&["--host".to_string(), "127.0.0.1".to_string()]).is_err());
        assert!(parse_ui_options(&["--bind".to_string(), "127.0.0.1".to_string()]).is_err());
        assert!(parse_ui_options(&["--manage".to_string()]).is_err());
        assert!(parse_ui_options(&[
            "--db".to_string(),
            "core.db".to_string(),
            "--plugins-dir".to_string(),
            "plugins.d".to_string(),
            "--manage".to_string(),
            "1".to_string(),
        ])
        .is_err());
    }

    #[test]
    fn ui_listener_binds_loopback_only() {
        let listener = bind_ui_listener(0).unwrap();
        let address = listener.local_addr().unwrap();

        assert!(address.ip().is_loopback());
        assert_eq!(address.ip().to_string(), "127.0.0.1");
    }

    #[test]
    fn api_routes_return_read_only_core_data() {
        let (dir, options) = fixture();
        let before = fs::read(&options.db_path).unwrap();

        let summary = handle_request(&options, "GET", "/api/summary");
        assert_eq!(summary.status, 200);
        let summary_json: Value = serde_json::from_slice(&summary.body).unwrap();
        assert_eq!(summary_json["inspection"]["source_posts"], json!(1));
        assert_eq!(
            summary_json["sources"][0]["source_name"],
            json!("synthetic")
        );

        let records = handle_request(&options, "GET", "/api/records?limit=10&offset=0");
        assert_eq!(records.status, 200);
        let records_json: Value = serde_json::from_slice(&records.body).unwrap();
        assert_eq!(
            records_json["records"][0]["cover_urls"][0],
            json!("https://example.test/cover.jpg")
        );
        assert_eq!(
            records_json["records"][0]["page_urls"][0],
            json!("https://example.test/page/1")
        );
        assert_eq!(
            records_json["records"][0]["external_links"][0]["provider"],
            json!("retail")
        );

        let state = handle_request(
            &options,
            "GET",
            "/api/state/known-source-urls?source_name=synthetic",
        );
        let state_json: Value = serde_json::from_slice(&state.body).unwrap();
        assert_eq!(state_json[0]["urls"][0], json!("synthetic://post/1"));
        assert_eq!(fs::read(&options.db_path).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn plugins_route_does_not_leak_env_values_or_local_paths() {
        let (dir, options) = fixture();

        let response = handle_request(&options, "GET", "/api/plugins");
        assert_eq!(response.status, 200);
        let body = String::from_utf8(response.body).unwrap();

        assert!(!body.contains("API_TOKEN"));
        assert!(!body.contains("\"secret\""));
        assert!(body.contains("<redacted-secret-env>"));
        assert!(!body.contains(dir.to_string_lossy().as_ref()));
        assert!(body.contains("<path:plugin.py>"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mutating_methods_are_rejected_without_cors_headers_or_db_touch() {
        let (dir, options) = fixture();
        let before = fs::read(&options.db_path).unwrap();

        let response = handle_request(&options, "POST", "/api/summary");

        assert_eq!(response.status, 405);
        assert_eq!(
            response.extra_headers,
            vec![("Allow", "GET, HEAD".to_string())]
        );
        assert!(!response
            .extra_headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("access-control-allow-origin")));
        assert_eq!(fs::read(&options.db_path).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn head_request_reuses_get_routes_without_body() {
        let (dir, options) = fixture();

        let response = handle_request(&options, "HEAD", "/");

        assert_eq!(response.status, 200);
        assert!(response.body.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn percent_decodes_query_values() {
        assert_eq!(
            query_value(Some("source_name=synthetic%2Fone+two"), "source_name").as_deref(),
            Some("synthetic/one two")
        );
        assert_eq!(query_value(Some("source_name=%zz"), "source_name"), None);
    }
}
