use std::collections::BTreeMap;
use std::error::Error;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mh_db::Database;
use mh_host::{
    discover_plugins, inspect_plugin_manifests, CancellationToken, DiscoverLimits, HostError,
    PluginHost, StateError,
};
use serde_json::{json, Value};

const DEFAULT_UI_PORT: u16 = 8765;
const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;
const MAX_MANAGE_DISCOVER_PAGES: u64 = 1_000;
const MAX_MANAGE_DISCOVER_PER_PAGE: u64 = 1_000;
const MAX_MANAGE_DISCOVER_RECORDS: u64 = 10_000;
const MAX_MANAGE_DISCOVER_TIMEOUT_SECONDS: u64 = 3_600;
/// Per-connection socket deadline. The UI is loopback-only, so a request that
/// stalls this long is not a slow client but a connection that will never
/// complete. Without it a single silent connection blocks the whole listener.
const UI_SOCKET_TIMEOUT: Duration = Duration::from_secs(15);
/// Absolute budget for reading one request. The per-read timeout above only
/// bounds a single blocking read, so a client trickling one byte at a time
/// would otherwise hold a worker (and its concurrency slot) indefinitely.
const UI_REQUEST_DEADLINE: Duration = Duration::from_secs(30);
/// Upper bound on connections served at once. Excess connections are refused
/// with 503 rather than queued behind the accept loop.
const MAX_CONCURRENT_CONNECTIONS: usize = 64;

#[derive(Debug, Clone)]
pub(crate) struct UiOptions {
    db_path: PathBuf,
    plugins_dir: PathBuf,
    port: u16,
    bound_port: u16,
    manage: bool,
    token: Option<String>,
    run_state: Arc<Mutex<ManagementState>>,
}

#[derive(Debug, Default)]
struct ManagementState {
    active: Option<ActiveRun>,
    last: Option<CompletedRun>,
}

#[derive(Debug, Clone)]
struct ActiveRun {
    run_id: String,
    plugin_id: String,
    started_at: u64,
    cancel_token: CancellationToken,
    cancellable: bool,
}

#[derive(Debug, Clone)]
struct CompletedRun {
    run_id: String,
    plugin_id: String,
    status: &'static str,
    finished_at: u64,
    result: Value,
}

#[derive(Debug, Clone)]
struct DiscoverRequest {
    plugin_id: String,
    limits: DiscoverLimits,
    timeout: Duration,
}

pub(crate) fn parse_ui_options(args: &[String]) -> Result<UiOptions, Box<dyn Error>> {
    let mut db_path = None;
    let mut plugins_dir = None;
    let mut port = DEFAULT_UI_PORT;
    let mut port_seen = false;
    let mut manage = false;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        if matches!(flag, "--host" | "--bind") {
            return Err(format!("{flag} is not accepted by the local-only UI").into());
        }
        if flag == "--manage" {
            if manage {
                return Err("--manage specified more than once".into());
            }
            manage = true;
            index += 1;
            continue;
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
        bound_port: port,
        manage,
        token: None,
        run_state: Arc::new(Mutex::new(ManagementState::default())),
    })
}

pub(crate) fn run_ui(mut options: UiOptions) -> Result<(), Box<dyn Error>> {
    let listener = bind_ui_listener(options.port)?;
    let address = listener.local_addr()?;
    options.bound_port = address.port();
    if options.manage {
        options.token = Some(generate_token()?);
    }
    println!("mh ui listening on http://{address}");
    if options.manage {
        println!("mh ui management mode enabled for this local process");
    }
    serve_listener(&listener, Arc::new(options));
    Ok(())
}

fn serve_listener(listener: &TcpListener, options: Arc<UiOptions>) {
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                if active.load(Ordering::Acquire) >= MAX_CONCURRENT_CONNECTIONS {
                    let _ = json_response(503, json!({"error": "too many connections"}))
                        .write_to(&mut stream);
                    continue;
                }
                active.fetch_add(1, Ordering::AcqRel);
                // The slot is released by Drop, so both the served and the
                // failed-to-spawn paths decrement exactly once.
                let slot = ConnectionSlot(Arc::clone(&active));
                let options = Arc::clone(&options);
                let spawned = thread::Builder::new()
                    .name("mh-ui-connection".to_string())
                    .spawn(move || {
                        let _slot = slot;
                        if let Err(err) = serve_connection(&options, &mut stream) {
                            eprintln!("ui request error: {err}");
                        }
                    });
                if let Err(err) = spawned {
                    eprintln!("ui connection spawn error: {err}");
                }
            }
            Err(err) => eprintln!("ui accept error: {err}"),
        }
    }
}

/// Releases one concurrency slot when dropped.
struct ConnectionSlot(Arc<AtomicUsize>);

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn bind_ui_listener(port: u16) -> io::Result<TcpListener> {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

fn serve_connection(options: &UiOptions, stream: &mut TcpStream) -> io::Result<()> {
    stream.set_read_timeout(Some(UI_SOCKET_TIMEOUT))?;
    stream.set_write_timeout(Some(UI_SOCKET_TIMEOUT))?;
    let request = read_http_request(stream)?;
    let response = match request {
        Some(request) => handle_request(options, &request),
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
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut TcpStream) -> io::Result<Option<HttpRequest>> {
    read_http_request_until(stream, Instant::now() + UI_REQUEST_DEADLINE)
}

/// Reads into `chunk` without letting the socket block past `deadline`.
///
/// The per-read timeout alone cannot hold the budget: a client that delivers a
/// byte just before the deadline would still buy a full `UI_SOCKET_TIMEOUT` on
/// the following read. Deriving each timeout from the remaining budget keeps the
/// whole request bounded by the deadline instead of `deadline + one timeout`.
/// `Ok(None)` means the budget is exhausted or the client stalled.
fn read_within_deadline(
    stream: &mut TcpStream,
    chunk: &mut [u8],
    deadline: Instant,
) -> io::Result<Option<usize>> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Ok(None);
    }
    stream.set_read_timeout(Some(remaining.min(UI_SOCKET_TIMEOUT)))?;
    match stream.read(chunk) {
        Ok(read) => Ok(Some(read)),
        // A stalled client is not an I/O failure; let the caller answer 400.
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

fn read_http_request_until(
    stream: &mut TcpStream,
    deadline: Instant,
) -> io::Result<Option<HttpRequest>> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        let Some(read) = read_within_deadline(stream, &mut chunk, deadline)? else {
            return Ok(None);
        };
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_REQUEST_HEADER_BYTES {
            return Ok(None);
        }
    };
    let text = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = text.lines();
    let Some(line) = lines.next() else {
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
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Ok(None);
        };
        let key = name.trim().to_ascii_lowercase();
        if key == "transfer-encoding" {
            return Ok(None);
        }
        if matches!(key.as_str(), "host" | "content-length") && headers.contains_key(&key) {
            return Ok(None);
        }
        headers.insert(key, value.trim().to_string());
    }
    let content_length = match headers.get("content-length") {
        Some(value) => match value.parse::<usize>() {
            Ok(parsed) => parsed,
            Err(_) => return Ok(None),
        },
        None => 0,
    };
    if content_length > MAX_REQUEST_BODY_BYTES {
        return Ok(None);
    }
    let mut body = buffer[header_end..].to_vec();
    while body.len() < content_length {
        let Some(read) = read_within_deadline(stream, &mut chunk, deadline)? else {
            return Ok(None);
        };
        if read == 0 {
            return Ok(None);
        }
        body.extend_from_slice(&chunk[..read]);
        if body.len() > MAX_REQUEST_BODY_BYTES {
            return Ok(None);
        }
    }
    // The loops above break on the delimiter/length check before re-testing the
    // clock, so a request whose last bytes landed past the budget would still
    // reach here. Reject it rather than serving a request that outlived its
    // deadline.
    if Instant::now() >= deadline {
        return Ok(None);
    }
    body.truncate(content_length);
    Ok(Some(HttpRequest {
        method: method.to_string(),
        target: target.to_string(),
        headers,
        body,
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

/// Rejects any request whose `Host` is not the loopback authority this process
/// is bound to. A browser coerced into resolving an attacker-controlled name to
/// 127.0.0.1 still sends that name here, so this is what stops DNS rebinding
/// from reaching the read-only routes.
fn require_loopback_host(options: &UiOptions, request: &HttpRequest) -> Option<HttpResponse> {
    let Some(host) = request.headers.get("host") else {
        return Some(json_response(
            400,
            json!({"error": "Host header is required"}),
        ));
    };
    if !is_allowed_loopback_authority(host, options.bound_port) {
        return Some(json_response(403, json!({"error": "invalid Host header"})));
    }
    None
}

fn handle_request(options: &UiOptions, request: &HttpRequest) -> HttpResponse {
    let head = request.method == "HEAD";
    if let Some(rejection) = require_loopback_host(options, request) {
        return if head {
            rejection.without_body()
        } else {
            rejection
        };
    }
    let (path, query) = split_target(&request.target);
    let mut response = match (request.method.as_str(), path) {
        ("GET" | "HEAD", "/") => html_response(&index_html(options)),
        ("GET" | "HEAD", "/api/summary") => summary_response(options),
        ("GET" | "HEAD", "/api/records") => records_response(options, query),
        ("GET" | "HEAD", "/api/state/known-source-urls") => {
            known_source_urls_response(options, query)
        }
        ("GET" | "HEAD", "/api/plugins") => plugins_response(options),
        ("GET" | "HEAD", "/api/manage/status") => management_status_response(options),
        ("POST", "/api/manage/init-db") => init_db_response(options, request),
        ("POST", "/api/manage/discover") => discover_response(options, request),
        ("POST", "/api/manage/cancel") => cancel_response(options, request),
        ("GET" | "HEAD", "/api/manage/init-db" | "/api/manage/discover" | "/api/manage/cancel") => {
            method_not_allowed("POST")
        }
        ("POST", _) => method_not_allowed("GET, HEAD"),
        (_, _) if is_known_path(path) => method_not_allowed("GET, HEAD"),
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

fn is_known_path(path: &str) -> bool {
    matches!(
        path,
        "/" | "/api/summary"
            | "/api/records"
            | "/api/state/known-source-urls"
            | "/api/plugins"
            | "/api/manage/status"
            | "/api/manage/init-db"
            | "/api/manage/discover"
            | "/api/manage/cancel"
    )
}

fn method_not_allowed(allow: &'static str) -> HttpResponse {
    HttpResponse::new(
        405,
        "Method Not Allowed",
        "application/json",
        json_bytes(json!({"error": "method not allowed"})),
    )
    .with_header("Allow", allow)
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

fn management_status_response(options: &UiOptions) -> HttpResponse {
    let state = options.run_state.lock().expect("management state poisoned");
    json_response(
        200,
        json!({
            "manage": options.manage,
            "active": state.active.as_ref().map(active_run_json),
            "last": state.last.as_ref().map(completed_run_json),
        }),
    )
}

fn init_db_response(options: &UiOptions, request: &HttpRequest) -> HttpResponse {
    if let Err(response) = require_management_request(options, request) {
        return response;
    }
    match Database::open(&options.db_path).and_then(|db| {
        db.initialize()?;
        db.inspect()
    }) {
        Ok(inspection) => json_response(200, json!({"inspection": inspection})),
        Err(err) => json_response(
            500,
            json!({"error": "init-db failed", "detail": err.to_string()}),
        ),
    }
}

fn discover_response(options: &UiOptions, request: &HttpRequest) -> HttpResponse {
    if let Err(response) = require_management_request(options, request) {
        return response;
    }
    let discover = match parse_discover_request(&request.body) {
        Ok(discover) => discover,
        Err(message) => return json_response(400, json!({"error": message})),
    };
    let run_id = format!(
        "ui-{}",
        generate_token().unwrap_or_else(|_| unix_seconds().to_string())
    );
    let cancel_token = CancellationToken::new();
    {
        let mut state = options.run_state.lock().expect("management state poisoned");
        if state.active.is_some() {
            return json_response(409, json!({"error": "discover already running"}));
        }
        state.active = Some(ActiveRun {
            run_id: run_id.clone(),
            plugin_id: discover.plugin_id.clone(),
            started_at: unix_seconds(),
            cancel_token: cancel_token.clone(),
            cancellable: true,
        });
    }

    let db_path = options.db_path.clone();
    let plugins_dir = options.plugins_dir.clone();
    let run_state = Arc::clone(&options.run_state);
    let thread_run_id = run_id.clone();
    thread::spawn(move || {
        let completed = run_ui_discover(
            &db_path,
            &plugins_dir,
            &thread_run_id,
            discover,
            &cancel_token,
            &run_state,
        );
        let mut state = run_state.lock().expect("management state poisoned");
        if state.active.as_ref().map(|active| active.run_id.as_str())
            == Some(thread_run_id.as_str())
        {
            state.active = None;
        }
        state.last = Some(completed);
    });

    json_response(202, json!({"run_id": run_id, "status": "running"}))
}

fn cancel_response(options: &UiOptions, request: &HttpRequest) -> HttpResponse {
    if let Err(response) = require_management_request(options, request) {
        return response;
    }
    let requested_run_id = if request.body.is_empty() {
        None
    } else {
        match serde_json::from_slice::<Value>(&request.body) {
            Ok(value) => value
                .get("run_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            Err(_) => return json_response(400, json!({"error": "invalid JSON body"})),
        }
    };
    let mut state = options.run_state.lock().expect("management state poisoned");
    let Some(active) = state.active.as_mut() else {
        return json_response(409, json!({"error": "no active UI discover run"}));
    };
    if requested_run_id
        .as_deref()
        .is_some_and(|run_id| run_id != active.run_id)
    {
        return json_response(404, json!({"error": "active run_id does not match"}));
    }
    if !active.cancellable {
        return json_response(
            409,
            json!({"error": "active UI discover run is no longer cancellable"}),
        );
    }
    active.cancel_token.cancel();
    json_response(
        202,
        json!({"run_id": active.run_id, "status": "cancelling"}),
    )
}

fn require_management_request(
    options: &UiOptions,
    request: &HttpRequest,
) -> Result<(), HttpResponse> {
    if !options.manage {
        return Err(json_response(
            403,
            json!({"error": "management mode disabled"}),
        ));
    }
    if request.method != "POST" {
        return Err(method_not_allowed("POST"));
    }
    let host = request
        .headers
        .get("host")
        .ok_or_else(|| json_response(400, json!({"error": "Host header is required"})))?;
    if !is_allowed_loopback_authority(host, options.bound_port) {
        return Err(json_response(403, json!({"error": "invalid Host header"})));
    }
    if let Some(origin) = request.headers.get("origin") {
        if !is_allowed_origin(origin, options.bound_port) {
            return Err(json_response(
                403,
                json!({"error": "invalid Origin header"}),
            ));
        }
    }
    let expected = options
        .token
        .as_deref()
        .ok_or_else(|| json_response(500, json!({"error": "management token unavailable"})))?;
    match request.headers.get("x-mh-ui-token") {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(json_response(
            403,
            json!({"error": "invalid management token"}),
        )),
    }
}

fn run_ui_discover(
    db_path: &PathBuf,
    plugins_dir: &PathBuf,
    run_id: &str,
    discover: DiscoverRequest,
    cancel_token: &CancellationToken,
    run_state: &Arc<Mutex<ManagementState>>,
) -> CompletedRun {
    let plugin_id = discover.plugin_id.clone();
    let result = run_ui_discover_inner(
        db_path,
        plugins_dir,
        run_id,
        discover,
        cancel_token,
        run_state,
    );
    let (status, result) = match result {
        Ok(result) => ("succeeded", result),
        Err(HostError::Cancelled) => ("cancelled", json!({"error": "cancelled"})),
        Err(err) => ("failed", json!({"error": err.to_string()})),
    };
    CompletedRun {
        run_id: run_id.to_string(),
        plugin_id,
        status,
        finished_at: unix_seconds(),
        result,
    }
}

fn run_ui_discover_inner(
    db_path: &PathBuf,
    plugins_dir: &PathBuf,
    run_id: &str,
    discover: DiscoverRequest,
    cancel_token: &CancellationToken,
    run_state: &Arc<Mutex<ManagementState>>,
) -> Result<Value, HostError> {
    let mut db = Database::open(db_path)
        .map_err(|err| HostError::State(StateError::backend(err.to_string())))?;
    db.initialize()
        .map_err(|err| HostError::State(StateError::backend(err.to_string())))?;
    let plugins = discover_plugins(plugins_dir)?;
    let plugin = plugins
        .iter()
        .find(|plugin| plugin.id == discover.plugin_id)
        .ok_or_else(|| HostError::Discovery(format!("plugin not found: {}", discover.plugin_id)))?;
    let run = {
        let state_provider = crate::DbStateProvider { db: &db };
        PluginHost::default().run_discover_with_state_provider_and_cancel(
            plugin,
            run_id,
            discover.limits,
            discover.timeout,
            &state_provider,
            cancel_token,
        )?
    };
    mark_run_ingesting(run_state, run_id, cancel_token)?;
    let ingest = db
        .ingest_records(&run.records)
        .map_err(|err| HostError::State(StateError::backend(err.to_string())))?;
    Ok(json!({
        "plugin_id": plugin.id,
        "source_name": run.manifest.source_name,
        "discover_records": run.discover_records,
        "spooled_records": run.records.len(),
        "ingested_records": ingest.records,
        "exit_status": run.exit_status.as_ref().and_then(|status| status.code()),
        "logs": run.logs.iter().map(|log| {
            json!({"level": log.level, "message": log.message})
        }).collect::<Vec<_>>()
    }))
}

fn parse_discover_request(body: &[u8]) -> Result<DiscoverRequest, String> {
    let value: Value = serde_json::from_slice(body).map_err(|_| "invalid JSON body".to_string())?;
    let plugin_id = required_string(&value, "plugin_id")?;
    let max_pages = required_bounded_u64(&value, "max_pages", MAX_MANAGE_DISCOVER_PAGES)?;
    let per_page = required_bounded_u64(&value, "per_page", MAX_MANAGE_DISCOVER_PER_PAGE)?;
    let max_records = required_bounded_u64(&value, "max_records", MAX_MANAGE_DISCOVER_RECORDS)?;
    let timeout_seconds = required_bounded_u64(
        &value,
        "timeout_seconds",
        MAX_MANAGE_DISCOVER_TIMEOUT_SECONDS,
    )?;
    Ok(DiscoverRequest {
        plugin_id,
        limits: DiscoverLimits {
            max_pages: Some(max_pages),
            max_records: Some(max_records),
            per_page: Some(per_page),
        },
        timeout: Duration::from_secs(timeout_seconds),
    })
}

fn required_string(value: &Value, key: &str) -> Result<String, String> {
    let parsed = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} is required"))?;
    if parsed.trim().is_empty() {
        return Err(format!("{key} must not be empty"));
    }
    Ok(parsed.to_string())
}

fn required_bounded_u64(value: &Value, key: &str, max: u64) -> Result<u64, String> {
    let parsed = value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{key} is required"))?;
    if parsed == 0 {
        return Err(format!("{key} must be greater than zero"));
    }
    if parsed > max {
        return Err(format!("{key} must be less than or equal to {max}"));
    }
    Ok(parsed)
}

fn mark_run_ingesting(
    run_state: &Arc<Mutex<ManagementState>>,
    run_id: &str,
    cancel_token: &CancellationToken,
) -> Result<(), HostError> {
    let mut state = run_state.lock().expect("management state poisoned");
    let active = state
        .active
        .as_mut()
        .filter(|active| active.run_id == run_id)
        .ok_or_else(|| HostError::State(StateError::backend("UI run state is not active")))?;
    if active.cancel_token.is_cancelled() || cancel_token.is_cancelled() {
        return Err(HostError::Cancelled);
    }
    active.cancellable = false;
    Ok(())
}

fn active_run_json(run: &ActiveRun) -> Value {
    json!({
        "run_id": run.run_id,
        "plugin_id": run.plugin_id,
        "started_at": run.started_at,
        "status": if run.cancel_token.is_cancelled() {
            "cancelling"
        } else if run.cancellable {
            "running"
        } else {
            "ingesting"
        },
    })
}

fn completed_run_json(run: &CompletedRun) -> Value {
    json!({
        "run_id": run.run_id,
        "plugin_id": run.plugin_id,
        "status": run.status,
        "finished_at": run.finished_at,
        "result": run.result,
    })
}

/// Compares an authority against the loopback address this process is bound to.
///
/// Host names are case-insensitive and clients omit the port when it is the
/// scheme default, so a byte-exact comparison rejects legitimate requests:
/// `LOCALHOST:8765`, or a bare `127.0.0.1` when the UI was started on `:80`.
/// Normalizing here does not widen the guard — a foreign name still fails the
/// host check regardless of case or port.
fn is_allowed_loopback_authority(value: &str, port: u16) -> bool {
    const DEFAULT_HTTP_PORT: u16 = 80;
    let value = value.trim();
    let (host, authority_port) = match value.rsplit_once(':') {
        Some((host, port_text)) => match port_text.parse::<u16>() {
            Ok(parsed) => (host, parsed),
            Err(_) => return false,
        },
        None => (value, DEFAULT_HTTP_PORT),
    };
    authority_port == port
        && (host.eq_ignore_ascii_case("127.0.0.1") || host.eq_ignore_ascii_case("localhost"))
}

fn is_allowed_origin(value: &str, port: u16) -> bool {
    let value = value.trim();
    match value.get(..7) {
        Some(scheme) if scheme.eq_ignore_ascii_case("http://") => {
            is_allowed_loopback_authority(&value[7..], port)
        }
        _ => false,
    }
}

fn generate_token() -> Result<String, Box<dyn Error>> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|err| format!("token generation failed: {err}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn index_html(options: &UiOptions) -> String {
    let config = json!({
        "manage": options.manage,
        "token": options.token.as_deref().unwrap_or(""),
    });
    INDEX_HTML.replace("__MH_UI_CONFIG__", &config.to_string())
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
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
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
.controls { display: flex; flex-wrap: wrap; gap: 8px; align-items: end; }
label { display: grid; gap: 3px; color: var(--muted); font-size: 12px; font-weight: 600; }
input, select, button { border: 1px solid var(--line); border-radius: 6px; padding: 7px 8px; background: #fff; color: var(--ink); font: inherit; }
button { cursor: pointer; font-weight: 650; }
button.primary { background: var(--accent); color: #fff; border-color: var(--accent); }
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
  <section id="management" hidden>
    <h2>Management</h2>
    <div class="controls">
      <label>Plugin <select id="manage-plugin"></select></label>
      <label>Max pages <input id="manage-max-pages" type="number" min="1" max="1000" value="1"></label>
      <label>Per page <input id="manage-per-page" type="number" min="1" max="1000" value="30"></label>
      <label>Max records <input id="manage-max-records" type="number" min="1" max="10000" value="30"></label>
      <label>Timeout <input id="manage-timeout" type="number" min="1" max="3600" value="60"></label>
      <button id="init-db" type="button">Init DB</button>
      <button id="run-discover" class="primary" type="button">Run discover</button>
      <button id="cancel-discover" type="button">Cancel</button>
    </div>
    <pre id="manage-status" class="muted"></pre>
  </section>
</main>
<script>
window.MH_UI = __MH_UI_CONFIG__;
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
async function postManage(path, payload) {
  const response = await fetch(path, {
    method: "POST",
    cache: "no-store",
    headers: {"Content-Type": "application/json", "X-MH-UI-Token": window.MH_UI.token},
    body: JSON.stringify(payload || {})
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(body.error || path + " " + response.status);
  return body;
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
  const select = document.getElementById("manage-plugin");
  select.replaceChildren(...payload.plugins
    .filter((plugin) => plugin.status === "valid" && plugin.id)
    .map((plugin) => {
      const option = document.createElement("option");
      option.value = plugin.id;
      option.textContent = plugin.id;
      return option;
    }));
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
function managePayload() {
  return {
    plugin_id: document.getElementById("manage-plugin").value,
    max_pages: Number(document.getElementById("manage-max-pages").value),
    per_page: Number(document.getElementById("manage-per-page").value),
    max_records: Number(document.getElementById("manage-max-records").value),
    timeout_seconds: Number(document.getElementById("manage-timeout").value)
  };
}
function renderManageStatus(value) {
  document.getElementById("manage-status").textContent = JSON.stringify(value, null, 2);
}
async function refreshManageStatus() {
  if (!window.MH_UI.manage) return;
  renderManageStatus(await fetchJson("/api/manage/status"));
}
function wireManagement() {
  if (!window.MH_UI.manage) return;
  document.getElementById("management").hidden = false;
  document.getElementById("init-db").onclick = async () => {
    try { renderManageStatus(await postManage("/api/manage/init-db")); }
    catch (error) { renderManageStatus({error: error.message}); }
  };
  document.getElementById("run-discover").onclick = async () => {
    try { renderManageStatus(await postManage("/api/manage/discover", managePayload())); }
    catch (error) { renderManageStatus({error: error.message}); }
  };
  document.getElementById("cancel-discover").onclick = async () => {
    try { renderManageStatus(await postManage("/api/manage/cancel")); }
    catch (error) { renderManageStatus({error: error.message}); }
  };
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
  wireManagement();
  await refreshManageStatus();
  document.getElementById("status").textContent = window.MH_UI.manage ? "management" : "read-only";
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
            bound_port: 8765,
            manage: false,
            token: None,
            run_state: Arc::new(Mutex::new(ManagementState::default())),
        };
        (dir, options)
    }

    fn request(method: &str, target: &str) -> HttpRequest {
        let mut headers = BTreeMap::new();
        headers.insert("host".to_string(), "127.0.0.1:8765".to_string());
        HttpRequest {
            method: method.to_string(),
            target: target.to_string(),
            headers,
            body: Vec::new(),
        }
    }

    #[test]
    fn read_routes_reject_foreign_host() {
        let (_dir, options) = fixture();
        for target in [
            "/",
            "/api/summary",
            "/api/records",
            "/api/state/known-source-urls",
            "/api/plugins",
        ] {
            let mut request = request("GET", target);
            request
                .headers
                .insert("host".to_string(), "evil.test:8765".to_string());
            let response = handle_request(&options, &request);
            assert_eq!(response.status, 403, "{target} accepted a foreign Host");
        }
    }

    #[test]
    fn read_routes_reject_loopback_name_on_wrong_port() {
        let (_dir, options) = fixture();
        let mut request = request("GET", "/api/summary");
        request
            .headers
            .insert("host".to_string(), "127.0.0.1:9999".to_string());
        assert_eq!(handle_request(&options, &request).status, 403);
    }

    #[test]
    fn read_routes_require_host_header() {
        let (_dir, options) = fixture();
        let mut request = request("GET", "/api/summary");
        request.headers.remove("host");
        assert_eq!(handle_request(&options, &request).status, 400);
    }

    #[test]
    fn read_routes_accept_localhost_authority() {
        let (_dir, options) = fixture();
        let mut request = request("GET", "/api/summary");
        request
            .headers
            .insert("host".to_string(), "localhost:8765".to_string());
        assert_eq!(handle_request(&options, &request).status, 200);
    }

    #[test]
    fn head_with_foreign_host_is_rejected_without_body() {
        let (_dir, options) = fixture();
        let mut request = request("HEAD", "/api/summary");
        request
            .headers
            .insert("host".to_string(), "evil.test:8765".to_string());
        let response = handle_request(&options, &request);
        assert_eq!(response.status, 403);
        assert!(response.body.is_empty());
    }

    #[test]
    fn silent_connection_does_not_block_other_requests() {
        let (_dir, mut options) = fixture();
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        options.bound_port = port;
        thread::spawn(move || serve_listener(&listener, Arc::new(options)));

        // Hold a connection open without ever sending a byte. Before the accept
        // loop spawned per-connection threads this alone wedged the listener.
        let _silent = TcpStream::connect(("127.0.0.1", port)).unwrap();

        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            client,
            "GET /api/summary HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        let text = String::from_utf8_lossy(&response);
        assert!(
            text.starts_with("HTTP/1.1 200 OK"),
            "second request was not served while a silent connection was open: {text}"
        );
    }

    #[test]
    fn loopback_authority_accepts_case_and_default_port_variants() {
        // Host names are case-insensitive (RFC 3986 / 7230).
        assert!(is_allowed_loopback_authority("LOCALHOST:8765", 8765));
        assert!(is_allowed_loopback_authority("LocalHost:8765", 8765));
        assert!(is_allowed_loopback_authority(" 127.0.0.1:8765 ", 8765));
        // Clients omit the port when it is the scheme default.
        assert!(is_allowed_loopback_authority("127.0.0.1", 80));
        assert!(is_allowed_loopback_authority("localhost", 80));
        assert!(is_allowed_origin("HTTP://LOCALHOST:8765", 8765));
        assert!(is_allowed_origin("http://127.0.0.1", 80));
    }

    #[test]
    fn loopback_authority_still_rejects_foreign_and_mismatched_authorities() {
        assert!(!is_allowed_loopback_authority("evil.test:8765", 8765));
        assert!(!is_allowed_loopback_authority("EVIL.TEST:8765", 8765));
        // A bare foreign name must not slip through the default-port branch.
        assert!(!is_allowed_loopback_authority("evil.test", 80));
        // Port must still match exactly.
        assert!(!is_allowed_loopback_authority("127.0.0.1:9999", 8765));
        assert!(!is_allowed_loopback_authority("127.0.0.1", 8765));
        assert!(!is_allowed_loopback_authority("127.0.0.1:", 8765));
        assert!(!is_allowed_loopback_authority("127.0.0.1:notaport", 8765));
        // Only loopback literals, and only over http.
        assert!(!is_allowed_loopback_authority("127.0.0.2:8765", 8765));
        assert!(!is_allowed_loopback_authority("[::1]:8765", 8765));
        // A loopback prefix or suffix must not smuggle a foreign host through.
        assert!(!is_allowed_loopback_authority(
            "127.0.0.1:8765@evil.test",
            8765
        ));
        assert!(!is_allowed_loopback_authority(
            "evil.test:8765#127.0.0.1:8765",
            8765
        ));
        assert!(!is_allowed_loopback_authority(
            "127.0.0.1.evil.test:8765",
            8765
        ));
        assert!(!is_allowed_loopback_authority(
            "evil.test:127.0.0.1:8765",
            8765
        ));
        assert!(!is_allowed_loopback_authority(
            "localhost.evil.test:8765",
            8765
        ));
        assert!(!is_allowed_origin("https://127.0.0.1:8765", 8765));
        assert!(!is_allowed_origin("http://evil.test:8765", 8765));
        assert!(!is_allowed_origin("127.0.0.1:8765", 8765));
    }

    #[test]
    fn read_stops_at_the_absolute_deadline_even_while_bytes_arrive() {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicUsize::new(0));
        let writer_stop = Arc::clone(&stop);
        // Trickle header bytes forever: every read succeeds, so a per-read
        // timeout alone would never fire.
        let writer = thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            while writer_stop.load(Ordering::Acquire) == 0 {
                if stream.write_all(b"X").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
        });

        let (mut accepted, _) = listener.accept().unwrap();
        accepted
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let started = Instant::now();
        let request =
            read_http_request_until(&mut accepted, Instant::now() + Duration::from_millis(300))
                .unwrap();
        let elapsed = started.elapsed();

        stop.store(1, Ordering::Release);
        drop(accepted);
        let _ = writer.join();

        assert!(request.is_none(), "a never-finished request was accepted");
        assert!(
            elapsed < Duration::from_secs(2),
            "deadline did not cut off a trickling client: {elapsed:?}"
        );
    }

    #[test]
    fn read_does_not_block_past_the_deadline_when_the_client_stalls() {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        // Send a partial header, then go silent. Without deriving each read
        // timeout from the remaining budget, the next read would burn the full
        // UI_SOCKET_TIMEOUT past the deadline.
        let writer = thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let _ = stream.write_all(b"GET / HTTP/1.1\r\n");
            thread::sleep(Duration::from_secs(3));
        });

        let (mut accepted, _) = listener.accept().unwrap();
        accepted.set_read_timeout(Some(UI_SOCKET_TIMEOUT)).unwrap();
        let started = Instant::now();
        let request =
            read_http_request_until(&mut accepted, Instant::now() + Duration::from_millis(300))
                .unwrap();
        let elapsed = started.elapsed();

        drop(accepted);
        let _ = writer.join();

        assert!(request.is_none(), "a stalled request was accepted");
        assert!(
            elapsed < Duration::from_secs(2),
            "read blocked past the deadline instead of clamping to it: {elapsed:?}"
        );
    }

    /// Characterizes the observable behaviour: a client whose completing bytes
    /// arrive after the budget gets 400, not a served request.
    ///
    /// What this does *not* isolate: the clamped read times out before those
    /// bytes land, so the timeout alone satisfies this test — it still passes
    /// with the post-loop deadline check removed (verified). That check guards
    /// the narrower case of a read syscall returning after the budget expired,
    /// which real-socket timing cannot deterministically produce; isolating it
    /// would need an injectable reader or clock. It is kept as defence in depth,
    /// not because this test covers it.
    #[test]
    fn request_completed_after_the_deadline_is_rejected() {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let writer = thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let _ = stream.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1:1\r\n");
            // The bytes that complete the header land after the budget is gone.
            thread::sleep(Duration::from_millis(400));
            let _ = stream.write_all(b"\r\n");
            thread::sleep(Duration::from_millis(200));
        });

        let (mut accepted, _) = listener.accept().unwrap();
        let request =
            read_http_request_until(&mut accepted, Instant::now() + Duration::from_millis(200))
                .unwrap();

        drop(accepted);
        let _ = writer.join();

        assert!(
            request.is_none(),
            "a request whose final bytes arrived past the deadline was accepted"
        );
    }

    #[test]
    fn serve_connection_arms_socket_timeouts() {
        let (_dir, options) = fixture();
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let _ = stream.write_all(b"GET / HTTP/1.1\r\n\r\n");
            let mut sink = Vec::new();
            let _ = stream.read_to_end(&mut sink);
        });
        let (mut accepted, _) = listener.accept().unwrap();
        serve_connection(&options, &mut accepted).unwrap();
        assert_eq!(
            accepted.read_timeout().unwrap(),
            Some(UI_SOCKET_TIMEOUT),
            "read timeout was not armed"
        );
        assert_eq!(
            accepted.write_timeout().unwrap(),
            Some(UI_SOCKET_TIMEOUT),
            "write timeout was not armed"
        );
        drop(accepted);
        client.join().unwrap();
    }

    fn management_options(db_path: PathBuf, plugins_dir: PathBuf) -> UiOptions {
        UiOptions {
            db_path,
            plugins_dir,
            port: 0,
            bound_port: 8765,
            manage: true,
            token: Some("test-token".to_string()),
            run_state: Arc::new(Mutex::new(ManagementState::default())),
        }
    }

    fn management_request(method: &str, target: &str, token: &str, body: Value) -> HttpRequest {
        let mut request = request(method, target);
        request
            .headers
            .insert("origin".to_string(), "http://127.0.0.1:8765".to_string());
        request
            .headers
            .insert("x-mh-ui-token".to_string(), token.to_string());
        request.body = serde_json::to_vec(&body).unwrap();
        request
    }

    fn python() -> String {
        std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string())
    }

    fn write_plugin(dir: &std::path::Path, body: &str) {
        let plugin = dir.join("plugin.py");
        fs::write(&plugin, body).unwrap();
        fs::write(
            dir.join("synthetic.json"),
            serde_json::to_string_pretty(&json!({
                "id": "synthetic",
                "argv": [python(), plugin.to_string_lossy().to_string()]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn discover_body() -> Value {
        json!({
            "plugin_id": "synthetic",
            "max_pages": 1,
            "per_page": 30,
            "max_records": 30,
            "timeout_seconds": 5
        })
    }

    fn wait_for_last(options: &UiOptions) -> CompletedRun {
        for _ in 0..80 {
            if let Some(run) = options
                .run_state
                .lock()
                .expect("management state poisoned")
                .last
                .clone()
            {
                return run;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("timed out waiting for UI run");
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
        assert!(!options.manage);
        assert!(parse_ui_options(&["--host".to_string(), "127.0.0.1".to_string()]).is_err());
        assert!(parse_ui_options(&["--bind".to_string(), "127.0.0.1".to_string()]).is_err());

        let manage = parse_ui_options(&[
            "--db".to_string(),
            "core.db".to_string(),
            "--plugins-dir".to_string(),
            "plugins.d".to_string(),
            "--manage".to_string(),
        ])
        .unwrap();
        assert!(manage.manage);
        assert!(parse_ui_options(&["--manage".to_string(), "--manage".to_string()]).is_err());
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

        let summary = handle_request(&options, &request("GET", "/api/summary"));
        assert_eq!(summary.status, 200);
        let summary_json: Value = serde_json::from_slice(&summary.body).unwrap();
        assert_eq!(summary_json["inspection"]["source_posts"], json!(1));
        assert_eq!(
            summary_json["sources"][0]["source_name"],
            json!("synthetic")
        );

        let records = handle_request(&options, &request("GET", "/api/records?limit=10&offset=0"));
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
            &request("GET", "/api/state/known-source-urls?source_name=synthetic"),
        );
        let state_json: Value = serde_json::from_slice(&state.body).unwrap();
        assert_eq!(state_json[0]["urls"][0], json!("synthetic://post/1"));
        assert_eq!(fs::read(&options.db_path).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn plugins_route_does_not_leak_env_values_or_local_paths() {
        let (dir, options) = fixture();

        let response = handle_request(&options, &request("GET", "/api/plugins"));
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

        let response = handle_request(&options, &request("POST", "/api/summary"));

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
    fn management_guard_blocks_missing_or_cross_origin_authority() {
        let (dir, read_only) = fixture();
        let options = management_options(read_only.db_path.clone(), read_only.plugins_dir.clone());
        let before = fs::read(&options.db_path).unwrap();

        let disabled = handle_request(
            &read_only,
            &management_request("POST", "/api/manage/init-db", "test-token", json!({})),
        );
        assert_eq!(disabled.status, 403);

        let get = handle_request(
            &options,
            &management_request("GET", "/api/manage/init-db", "test-token", json!({})),
        );
        assert_eq!(get.status, 405);

        let missing_token = handle_request(&options, &request("POST", "/api/manage/init-db"));
        assert_eq!(missing_token.status, 403);

        let mut bad_host =
            management_request("POST", "/api/manage/init-db", "test-token", json!({}));
        bad_host
            .headers
            .insert("host".to_string(), "example.test:8765".to_string());
        assert_eq!(handle_request(&options, &bad_host).status, 403);

        let mut bad_origin =
            management_request("POST", "/api/manage/init-db", "test-token", json!({}));
        bad_origin
            .headers
            .insert("origin".to_string(), "http://example.test".to_string());
        assert_eq!(handle_request(&options, &bad_origin).status, 403);

        for response in [disabled, get, missing_token] {
            assert!(!response
                .extra_headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("access-control-allow-origin")));
        }
        assert_eq!(fs::read(&options.db_path).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn guarded_init_db_initializes_empty_database() {
        let dir = temp_dir("manage-init");
        let db_path = dir.join("core.db");
        let plugins_dir = dir.join("plugins.d");
        fs::create_dir(&plugins_dir).unwrap();
        let options = management_options(db_path.clone(), plugins_dir);

        let response = handle_request(
            &options,
            &management_request("POST", "/api/manage/init-db", "test-token", json!({})),
        );

        assert_eq!(response.status, 200);
        let body: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["inspection"]["schema_version"], json!(1));
        assert!(db_path.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn guarded_discover_requires_explicit_bounds_and_ingests() {
        let dir = temp_dir("manage-discover");
        let db_path = dir.join("core.db");
        let plugins_dir = dir.join("plugins.d");
        fs::create_dir(&plugins_dir).unwrap();
        write_plugin(
            &plugins_dir,
            r#"
import json
import struct
import sys

def read_frame():
    header = sys.stdin.buffer.read(4)
    if not header:
        raise SystemExit(0)
    size = struct.unpack(">I", header)[0]
    return json.loads(sys.stdin.buffer.read(size).decode("utf-8"))

def write_frame(value):
    payload = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    sys.stdout.buffer.write(struct.pack(">I", len(payload)))
    sys.stdout.buffer.write(payload)
    sys.stdout.buffer.flush()

init = read_frame()
write_frame({"jsonrpc": "2.0", "id": init["id"], "result": {
    "protocol_version": 1,
    "record_schema_version": 1,
    "manifest": {
        "source_name": "synthetic",
        "display_label": "Synthetic",
        "allowed_domains": [],
        "capabilities": []
    }
}})
discover = read_frame()
limits = discover["params"]["limits"]
if limits != {"max_pages": 1, "max_records": 30, "per_page": 30}:
    write_frame({"jsonrpc": "2.0", "id": discover["id"], "error": {
        "code": -32000,
        "message": "unexpected limits"
    }})
    raise SystemExit(0)
record = {
    "source_name": "synthetic",
    "source_url": "synthetic://ui/1",
    "title": "UI managed record",
    "brand_raw": "Synthetic Brand",
    "performers_raw": [],
    "cover_urls": [],
    "page_urls": [],
    "external_links": []
}
write_frame({"jsonrpc": "2.0", "method": "record", "params": {"request_id": discover["params"]["request_id"], "record": record}})
write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": 1}})
"#,
        );
        let options = management_options(db_path.clone(), plugins_dir);

        let mut missing_bound = discover_body();
        missing_bound.as_object_mut().unwrap().remove("max_records");
        assert_eq!(
            handle_request(
                &options,
                &management_request("POST", "/api/manage/discover", "test-token", missing_bound,),
            )
            .status,
            400
        );
        for key in ["max_pages", "per_page", "max_records", "timeout_seconds"] {
            for invalid in [json!(0), json!(-1), json!("1")] {
                let mut body = discover_body();
                body.as_object_mut()
                    .unwrap()
                    .insert(key.to_string(), invalid);
                assert_eq!(
                    handle_request(
                        &options,
                        &management_request("POST", "/api/manage/discover", "test-token", body,),
                    )
                    .status,
                    400,
                    "{key} should reject invalid value"
                );
            }
        }
        for (key, invalid) in [
            ("max_pages", MAX_MANAGE_DISCOVER_PAGES + 1),
            ("per_page", MAX_MANAGE_DISCOVER_PER_PAGE + 1),
            ("max_records", MAX_MANAGE_DISCOVER_RECORDS + 1),
            ("timeout_seconds", MAX_MANAGE_DISCOVER_TIMEOUT_SECONDS + 1),
        ] {
            let mut body = discover_body();
            body.as_object_mut()
                .unwrap()
                .insert(key.to_string(), json!(invalid));
            assert_eq!(
                handle_request(
                    &options,
                    &management_request("POST", "/api/manage/discover", "test-token", body,),
                )
                .status,
                400,
                "{key} should reject values above the UI cap"
            );
        }

        let response = handle_request(
            &options,
            &management_request(
                "POST",
                "/api/manage/discover",
                "test-token",
                discover_body(),
            ),
        );
        assert_eq!(response.status, 202);
        let completed = wait_for_last(&options);
        assert_eq!(completed.status, "succeeded");
        assert_eq!(completed.result["ingested_records"], json!(1));

        let summary = handle_request(&options, &request("GET", "/api/summary"));
        let summary_json: Value = serde_json::from_slice(&summary.body).unwrap();
        assert_eq!(summary_json["inspection"]["source_posts"], json!(1));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn guarded_cancel_only_targets_active_ui_discover() {
        let dir = temp_dir("manage-cancel");
        let db_path = dir.join("core.db");
        let plugins_dir = dir.join("plugins.d");
        let cancel_file = dir.join("cancel.seen");
        let ready_file = dir.join("discover.ready");
        fs::create_dir(&plugins_dir).unwrap();
        write_plugin(
            &plugins_dir,
            &format!(
                r#"
import json
import struct
import sys
import time

def read_frame():
    header = sys.stdin.buffer.read(4)
    if not header:
        raise SystemExit(0)
    size = struct.unpack(">I", header)[0]
    return json.loads(sys.stdin.buffer.read(size).decode("utf-8"))

def write_frame(value):
    payload = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    sys.stdout.buffer.write(struct.pack(">I", len(payload)))
    sys.stdout.buffer.write(payload)
    sys.stdout.buffer.flush()

init = read_frame()
write_frame({{"jsonrpc": "2.0", "id": init["id"], "result": {{
    "protocol_version": 1,
    "record_schema_version": 1,
    "manifest": {{
        "source_name": "synthetic",
        "display_label": "Synthetic",
        "allowed_domains": [],
        "capabilities": []
    }}
}}}})
discover = read_frame()
with open({ready_file:?}, "w") as f:
    f.write(discover["params"]["request_id"])
cancel = read_frame()
if cancel.get("method") == "cancel":
    with open({cancel_file:?}, "w") as f:
        f.write(cancel["params"]["request_id"])
time.sleep(0.05)
"#
            ),
        );
        let options = management_options(db_path.clone(), plugins_dir);

        assert_eq!(
            handle_request(
                &options,
                &management_request("POST", "/api/manage/cancel", "test-token", json!({})),
            )
            .status,
            409
        );

        let response = handle_request(
            &options,
            &management_request(
                "POST",
                "/api/manage/discover",
                "test-token",
                discover_body(),
            ),
        );
        assert_eq!(response.status, 202);
        for _ in 0..80 {
            if ready_file.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(ready_file.exists());
        let second = handle_request(
            &options,
            &management_request(
                "POST",
                "/api/manage/discover",
                "test-token",
                discover_body(),
            ),
        );
        assert_eq!(second.status, 409);
        let wrong_run = handle_request(
            &options,
            &management_request(
                "POST",
                "/api/manage/cancel",
                "test-token",
                json!({"run_id": "not-the-active-run"}),
            ),
        );
        assert_eq!(wrong_run.status, 404);
        let cancel = handle_request(
            &options,
            &management_request("POST", "/api/manage/cancel", "test-token", json!({})),
        );
        assert_eq!(cancel.status, 202);
        let completed = wait_for_last(&options);
        assert_eq!(completed.status, "cancelled");
        assert!(fs::read_to_string(&cancel_file).unwrap().starts_with("ui-"));
        assert_eq!(Database::inspect_path(&db_path).unwrap().source_posts, 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn accepted_cancel_before_ingest_keeps_spooled_records_out_of_db() {
        let cancel_token = CancellationToken::new();
        let run_state = Arc::new(Mutex::new(ManagementState {
            active: Some(ActiveRun {
                run_id: "ui-spooled".to_string(),
                plugin_id: "synthetic".to_string(),
                started_at: 0,
                cancel_token: cancel_token.clone(),
                cancellable: true,
            }),
            last: None,
        }));

        cancel_token.cancel();

        let err = mark_run_ingesting(&run_state, "ui-spooled", &cancel_token).unwrap_err();
        assert!(matches!(err, HostError::Cancelled));
        assert!(
            run_state
                .lock()
                .unwrap()
                .active
                .as_ref()
                .unwrap()
                .cancellable
        );
    }

    #[test]
    fn head_request_reuses_get_routes_without_body() {
        let (dir, options) = fixture();

        let response = handle_request(&options, &request("HEAD", "/"));

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
