//! Subprocess plugin host runtime.
//!
//! Discover plugin command manifests, run `initialize` + `discover`, service
//! typed plugin->host requests, spool `record` notifications in memory, and
//! clean up the plugin process group.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mh_domain::SourceRecord;
use mh_fetch::{FetchPolicy, FetchRequest, FetchResponse, SafeFetcher};
use mh_protocol::message::{
    self, MAX_PENDING_REQUESTS, MAX_RECORD_BATCH, PROTOCOL_VERSION, RECORD_SCHEMA_VERSION,
};
use mh_protocol::{canonical_json, read_frame, write_frame, FrameError, Method};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Absolute host-side cap used when a caller does not provide a lower limit.
pub const HOST_MAX_RECORDS: u64 = 10_000;
/// Total serialized `record` payload bytes retained in the in-memory spool.
pub const HOST_MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;
/// Total protocol log bytes retained in a run result.
pub const HOST_MAX_LOG_BYTES: usize = 1024 * 1024;

/// Runtime definition loaded from `plugins.d/*.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDefinition {
    pub id: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub working_dir: Option<PathBuf>,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct PluginDefinitionFile {
    id: Option<String>,
    source_name: Option<String>,
    argv: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    working_dir: Option<String>,
}

/// Discover JSON plugin definitions from a directory.
///
/// Manifest shape:
///
/// ```json
/// {"id": "synthetic", "argv": ["/path/to/plugin", "--arg"]}
/// ```
///
/// `argv` is executed directly, never through a shell. Relative `working_dir`
/// values are resolved against the manifest's parent directory.
pub fn discover_plugins(dir: impl AsRef<Path>) -> Result<Vec<PluginDefinition>, HostError> {
    let dir = dir.as_ref();
    let mut entries = fs::read_dir(dir)
        .map_err(HostError::Io)?
        .collect::<Result<Vec<_>, io::Error>>()
        .map_err(HostError::Io)?;
    entries.sort_by_key(|entry| entry.path());

    let mut plugins = Vec::new();
    let mut seen_ids = BTreeMap::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(HostError::Io)?;
        let parsed: PluginDefinitionFile = serde_json::from_str(&raw).map_err(HostError::Json)?;
        if parsed.argv.is_empty() {
            return Err(HostError::Discovery(format!(
                "{} has empty argv",
                path.display()
            )));
        }
        let id = parsed
            .id
            .or(parsed.source_name)
            .or_else(|| {
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
            })
            .ok_or_else(|| HostError::Discovery(format!("{} has no id", path.display())))?;
        if id.trim().is_empty() {
            return Err(HostError::Discovery(format!(
                "{} has empty id",
                path.display()
            )));
        }
        if let Some(previous) = seen_ids.insert(id.clone(), path.clone()) {
            return Err(HostError::Discovery(format!(
                "duplicate plugin id {id:?}: {} and {}",
                previous.display(),
                path.display()
            )));
        }
        let working_dir = parsed.working_dir.map(|value| {
            let candidate = PathBuf::from(value);
            if candidate.is_absolute() {
                candidate
            } else {
                path.parent().unwrap_or(dir).join(candidate)
            }
        });
        plugins.push(PluginDefinition {
            id,
            argv: parsed.argv,
            env: parsed.env,
            working_dir,
            manifest_path: path,
        });
    }
    Ok(plugins)
}

/// Host-side view of the plugin's manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub source_name: String,
    pub display_label: String,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct InitializeResult {
    protocol_version: i64,
    record_schema_version: i64,
    manifest: PluginManifest,
}

#[derive(Debug, Deserialize)]
struct DiscoverResult {
    records: u64,
}

/// Limits passed to `discover`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DiscoverLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_records: Option<u64>,
}

impl DiscoverLimits {
    fn with_host_caps(self) -> Self {
        Self {
            max_pages: self.max_pages,
            max_records: Some(
                self.max_records
                    .unwrap_or(HOST_MAX_RECORDS)
                    .min(HOST_MAX_RECORDS),
            ),
        }
    }
}

/// Protocol log emitted by a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLog {
    pub level: String,
    pub message: String,
}

/// Completed discover run.
#[derive(Debug, Clone)]
pub struct HostRun {
    pub manifest: PluginManifest,
    pub discover_records: u64,
    pub records: Vec<SourceRecord>,
    pub logs: Vec<HostLog>,
    pub exit_status: Option<ExitStatus>,
}

/// Typed state operations plugins may request during discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateOperation {
    KnownSourceUrls {
        source_name: String,
    },
    SourcePostSummary {
        source_name: String,
        source_url: String,
    },
    LastSeenAt {
        source_name: String,
    },
    ContentFingerprint {
        source_name: String,
        source_url: String,
    },
}

/// Read-only plugin state provider. Implementations must not expose arbitrary
/// SQL or hold write transactions during network discovery.
pub trait StateProvider {
    fn query(&self, op: StateOperation) -> Result<Value, StateError>;
}

/// Explicit no-state provider used by callers that have not wired DB state.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoStateProvider;

impl StateProvider for NoStateProvider {
    fn query(&self, _op: StateOperation) -> Result<Value, StateError> {
        Err(StateError::unavailable("state provider is not configured"))
    }
}

/// State provider failure surfaced as a JSON-RPC error and host failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateError {
    message: String,
}

impl StateError {
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn backend(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StateError {}

/// Safe fetch provider for plugin `fetch_request` traffic.
trait FetchProvider {
    fn fetch(
        &self,
        manifest: &PluginManifest,
        request: FetchRequest,
        timeout: Duration,
    ) -> Result<FetchResponse, HostFetchError>;
}

/// Production host fetch provider using `mh-fetch` safety checks.
#[derive(Debug, Clone, Default)]
struct SafeFetchProvider {
    fetcher: SafeFetcher,
}

impl FetchProvider for SafeFetchProvider {
    fn fetch(
        &self,
        manifest: &PluginManifest,
        request: FetchRequest,
        timeout: Duration,
    ) -> Result<FetchResponse, HostFetchError> {
        let mut policy = FetchPolicy::for_allowed_domains(manifest.allowed_domains.clone());
        policy.total_timeout = policy.total_timeout.min(timeout);
        policy.connect_timeout = policy.connect_timeout.min(policy.total_timeout);
        self.fetcher
            .fetch(request, &policy)
            .map_err(|err| HostFetchError::backend(err.to_string()))
    }
}

/// Fetch failure surfaced as a JSON-RPC error and host failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFetchError {
    message: String,
}

impl HostFetchError {
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn backend(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for HostFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HostFetchError {}

/// Plugin host runtime.
#[derive(Debug, Clone)]
pub struct PluginHost {
    host_version: String,
    shutdown_grace: Duration,
}

impl Default for PluginHost {
    fn default() -> Self {
        Self {
            host_version: format!("mh-host/{}", env!("CARGO_PKG_VERSION")),
            shutdown_grace: Duration::from_millis(150),
        }
    }
}

impl PluginHost {
    pub fn new(host_version: impl Into<String>) -> Self {
        Self {
            host_version: host_version.into(),
            ..Self::default()
        }
    }

    /// Run `initialize` then `discover`, spool records, and terminate the
    /// subprocess if it does not exit promptly after discover completes.
    pub fn run_discover(
        &self,
        plugin: &PluginDefinition,
        request_id: &str,
        limits: DiscoverLimits,
        timeout: Duration,
    ) -> Result<HostRun, HostError> {
        self.run_discover_with_providers(
            plugin,
            request_id,
            limits,
            timeout,
            &NoStateProvider,
            &SafeFetchProvider::default(),
        )
    }

    /// Run discovery with a typed state provider available to `state_query`.
    pub fn run_discover_with_state_provider(
        &self,
        plugin: &PluginDefinition,
        request_id: &str,
        limits: DiscoverLimits,
        timeout: Duration,
        state_provider: &dyn StateProvider,
    ) -> Result<HostRun, HostError> {
        self.run_discover_with_providers(
            plugin,
            request_id,
            limits,
            timeout,
            state_provider,
            &SafeFetchProvider::default(),
        )
    }

    /// Run discovery with typed state and safe fetch providers.
    fn run_discover_with_providers(
        &self,
        plugin: &PluginDefinition,
        request_id: &str,
        limits: DiscoverLimits,
        timeout: Duration,
        state_provider: &dyn StateProvider,
        fetch_provider: &dyn FetchProvider,
    ) -> Result<HostRun, HostError> {
        let effective_limits = limits.with_host_caps();
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| HostError::Protocol("timeout overflow".to_string()))?;
        let mut process = PluginProcess::spawn(plugin)?;
        let mut state = RunState::new(effective_limits.max_records);

        process.send(&message::request(
            "h-1",
            Method::Initialize,
            json!({"protocol_version": PROTOCOL_VERSION, "host_version": self.host_version}),
        ))?;
        let init_value = self.read_until_response(
            &mut process,
            "h-1",
            deadline,
            LoopContext {
                request_id,
                manifest: None,
                state: &mut state,
                state_provider,
                fetch_provider,
                deadline,
            },
        )?;
        let init: InitializeResult = serde_json::from_value(init_value).map_err(HostError::Json)?;
        if init.protocol_version != PROTOCOL_VERSION {
            return Err(HostError::Protocol(format!(
                "unsupported protocol_version {}",
                init.protocol_version
            )));
        }
        if init.record_schema_version != RECORD_SCHEMA_VERSION {
            return Err(HostError::Protocol(format!(
                "unsupported record_schema_version {}",
                init.record_schema_version
            )));
        }
        if init.manifest.source_name.trim().is_empty() {
            return Err(HostError::Protocol(
                "plugin manifest source_name is empty".to_string(),
            ));
        }

        process.send(&message::request(
            "h-2",
            Method::Discover,
            json!({
                "request_id": request_id,
                "limits": effective_limits,
                "remaining_ms": remaining_ms(deadline),
            }),
        ))?;
        let discover_value = self.read_until_response(
            &mut process,
            "h-2",
            deadline,
            LoopContext {
                request_id,
                manifest: Some(&init.manifest),
                state: &mut state,
                state_provider,
                fetch_provider,
                deadline,
            },
        )?;
        let discover: DiscoverResult =
            serde_json::from_value(discover_value).map_err(HostError::Json)?;
        let exit_status = process.wait_or_terminate(self.shutdown_grace)?;
        let exit_status = match exit_status {
            Some(status) if status.success() => status,
            Some(status) => return Err(HostError::PluginExitStatus(status)),
            None => return Err(HostError::PluginDidNotExit),
        };
        process.ensure_no_frames_after_response()?;
        if discover.records != state.spooled_records() {
            return Err(HostError::Protocol(format!(
                "discover result records {} did not match spooled records {}",
                discover.records,
                state.spooled_records()
            )));
        }

        Ok(HostRun {
            manifest: init.manifest,
            discover_records: discover.records,
            records: state.records,
            logs: state.logs,
            exit_status: Some(exit_status),
        })
    }

    fn read_until_response(
        &self,
        process: &mut PluginProcess,
        expected_id: &str,
        deadline: Instant,
        mut context: LoopContext<'_, '_>,
    ) -> Result<Value, HostError> {
        loop {
            let payload = match process.recv_frame(deadline) {
                Ok(payload) => payload,
                Err(HostError::Timeout) => {
                    let _ = process.send_cancel(context.request_id);
                    let _ = process.wait_or_terminate(self.shutdown_grace);
                    return Err(HostError::Timeout);
                }
                Err(err @ HostError::PluginExited(_)) => return Err(err),
                Err(err) => {
                    let _ = process.terminate_tree(self.shutdown_grace);
                    return Err(err);
                }
            };
            let value: Value = serde_json::from_str(&payload).map_err(HostError::Json)?;
            require_jsonrpc(&value)?;

            if let Some(method) = value.get("method").and_then(Value::as_str) {
                self.handle_plugin_message(process, method, &value, &mut context)?;
                continue;
            }

            let Some(id) = value.get("id").and_then(Value::as_str) else {
                return Err(HostError::Protocol(
                    "message without method or string id".to_string(),
                ));
            };
            if id != expected_id {
                return Err(HostError::Protocol(format!(
                    "unexpected response id {id}, expected {expected_id}"
                )));
            }
            if let Some(error) = value.get("error") {
                return Err(HostError::PluginError(error.clone()));
            }
            return value
                .get("result")
                .cloned()
                .ok_or_else(|| HostError::Protocol("response missing result".to_string()));
        }
    }

    fn handle_plugin_message(
        &self,
        process: &mut PluginProcess,
        method: &str,
        value: &Value,
        context: &mut LoopContext<'_, '_>,
    ) -> Result<(), HostError> {
        let Some(parsed) = Method::parse(method) else {
            if let Some(id) = value.get("id") {
                process.send(&message::response_err(id, -32601, "unknown method"))?;
            }
            return Err(HostError::Protocol(format!(
                "unknown plugin method {method}"
            )));
        };

        match parsed {
            Method::Record => {
                require_notification(value, method)?;
                let manifest = context.manifest.ok_or_else(|| {
                    HostError::Protocol("record received before discover started".to_string())
                })?;
                let params = value
                    .get("params")
                    .ok_or_else(|| HostError::Protocol("record missing params".to_string()))?;
                let actual_request_id = params
                    .get("request_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| HostError::Protocol("record missing request_id".to_string()))?;
                if actual_request_id != context.request_id {
                    return Err(HostError::Protocol(format!(
                        "record request_id {actual_request_id} did not match {}",
                        context.request_id
                    )));
                }
                self.spool_records(params, manifest, context.state)
            }
            Method::Log => {
                require_notification(value, method)?;
                let params = value
                    .get("params")
                    .ok_or_else(|| HostError::Protocol("log missing params".to_string()))?;
                let level = params
                    .get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("info")
                    .to_string();
                let message = params
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                context.state.push_log(level, message)
            }
            Method::StateQuery => {
                require_discover_phase(context.manifest, method)?;
                self.handle_state_query(process, value, context.state_provider)
            }
            Method::FetchRequest => {
                require_discover_phase(context.manifest, method)?;
                let manifest = context.manifest.expect("checked discover phase");
                self.handle_fetch_request(
                    process,
                    value,
                    manifest,
                    context.fetch_provider,
                    context.deadline,
                )
            }
            Method::Initialize | Method::Discover | Method::Cancel => Err(HostError::Protocol(
                format!("method {method} is not valid plugin->host traffic"),
            )),
        }
    }

    fn spool_records(
        &self,
        params: &Value,
        manifest: &PluginManifest,
        state: &mut RunState,
    ) -> Result<(), HostError> {
        if let Some(record) = params.get("record") {
            state.reserve_record_values([record])?;
            let record: SourceRecord =
                serde_json::from_value(record.clone()).map_err(HostError::Json)?;
            record
                .validate_for_source(&manifest.source_name)
                .map_err(HostError::Validation)?;
            state.records.push(record);
            return Ok(());
        }

        let records = params
            .get("records")
            .and_then(Value::as_array)
            .ok_or_else(|| HostError::Protocol("record missing record(s)".to_string()))?;
        if records.len() > MAX_RECORD_BATCH {
            return Err(HostError::Protocol(format!(
                "record batch too large: {} > {MAX_RECORD_BATCH}",
                records.len()
            )));
        }
        state.reserve_record_values(records.iter())?;
        for record in records {
            let record: SourceRecord =
                serde_json::from_value(record.clone()).map_err(HostError::Json)?;
            record
                .validate_for_source(&manifest.source_name)
                .map_err(HostError::Validation)?;
            state.records.push(record);
        }
        Ok(())
    }

    fn handle_state_query(
        &self,
        process: &mut PluginProcess,
        value: &Value,
        state_provider: &dyn StateProvider,
    ) -> Result<(), HostError> {
        let id = value
            .get("id")
            .ok_or_else(|| HostError::Protocol("state_query missing json-rpc id".to_string()))?;
        require_plugin_request_id(id, "state_query")?;
        let params = value
            .get("params")
            .ok_or_else(|| HostError::Protocol("state_query missing params".to_string()))?;
        let (query_id, operation) = match parse_state_query(params) {
            Ok(parsed) => parsed,
            Err(err) => {
                process.send(&message::response_err(id, -32602, &err.to_string()))?;
                return Err(err);
            }
        };
        let result = match state_provider.query(operation) {
            Ok(result) => result,
            Err(err) => {
                process.send(&message::response_err(id, -32000, err.message()))?;
                return Err(HostError::State(err));
            }
        };
        process.send(&message::response_ok(
            id,
            json!({"id": query_id, "result": result}),
        ))
    }

    fn handle_fetch_request(
        &self,
        process: &mut PluginProcess,
        value: &Value,
        manifest: &PluginManifest,
        fetch_provider: &dyn FetchProvider,
        deadline: Instant,
    ) -> Result<(), HostError> {
        let id = value
            .get("id")
            .ok_or_else(|| HostError::Protocol("fetch_request missing id".to_string()))?;
        require_plugin_request_id(id, "fetch_request")?;
        let params = value
            .get("params")
            .ok_or_else(|| HostError::Protocol("fetch_request missing params".to_string()))?;
        let (request_id, request) = match parse_fetch_request(params) {
            Ok(parsed) => parsed,
            Err(err) => {
                process.send(&message::response_err(id, -32602, &err.to_string()))?;
                return Err(err);
            }
        };
        let Some(timeout) = deadline.checked_duration_since(Instant::now()) else {
            process.send(&message::response_err(id, -32000, "fetch deadline elapsed"))?;
            return Err(HostError::Timeout);
        };
        let response = match fetch_provider.fetch(manifest, request, timeout) {
            Ok(response) => response,
            Err(err) => {
                process.send(&message::response_err(id, -32000, err.message()))?;
                return Err(HostError::Fetch(err));
            }
        };
        process.send(&message::response_ok(
            id,
            json!({
                "id": request_id,
                "status": response.status,
                "final_url": response.final_url,
                "body_base64": response.body_base64,
            }),
        ))
    }
}

struct LoopContext<'a, 'b> {
    request_id: &'a str,
    manifest: Option<&'a PluginManifest>,
    state: &'b mut RunState,
    state_provider: &'a dyn StateProvider,
    fetch_provider: &'a dyn FetchProvider,
    deadline: Instant,
}

struct RunState {
    records: Vec<SourceRecord>,
    logs: Vec<HostLog>,
    max_records: Option<u64>,
    record_bytes: usize,
    log_bytes: usize,
}

impl RunState {
    fn new(max_records: Option<u64>) -> Self {
        Self {
            records: Vec::new(),
            logs: Vec::new(),
            max_records,
            record_bytes: 0,
            log_bytes: 0,
        }
    }

    fn spooled_records(&self) -> u64 {
        u64::try_from(self.records.len()).unwrap_or(u64::MAX)
    }

    fn reserve_record_values<'a>(
        &mut self,
        records: impl IntoIterator<Item = &'a Value>,
    ) -> Result<(), HostError> {
        let mut incoming = 0usize;
        let mut incoming_bytes = 0usize;
        for record in records {
            incoming = incoming.saturating_add(1);
            incoming_bytes = incoming_bytes.saturating_add(json_size(record)?);
        }
        let Some(max_records) = self.max_records else {
            return self.reserve_record_bytes(incoming_bytes);
        };
        let incoming = u64::try_from(incoming).unwrap_or(u64::MAX);
        let next = self.spooled_records().saturating_add(incoming);
        if next > max_records {
            return Err(HostError::Protocol(format!(
                "record spool exceeded max_records: {next} > {max_records}"
            )));
        }
        self.reserve_record_bytes(incoming_bytes)
    }

    fn reserve_record_bytes(&mut self, incoming_bytes: usize) -> Result<(), HostError> {
        let next = self.record_bytes.saturating_add(incoming_bytes);
        if next > HOST_MAX_RECORD_BYTES {
            return Err(HostError::Protocol(format!(
                "record spool exceeded byte limit: {next} > {HOST_MAX_RECORD_BYTES}"
            )));
        }
        self.record_bytes = next;
        Ok(())
    }

    fn push_log(&mut self, level: String, message: String) -> Result<(), HostError> {
        let incoming = level.len().saturating_add(message.len());
        let next = self.log_bytes.saturating_add(incoming);
        if next > HOST_MAX_LOG_BYTES {
            return Err(HostError::Protocol(format!(
                "log spool exceeded byte limit: {next} > {HOST_MAX_LOG_BYTES}"
            )));
        }
        self.log_bytes = next;
        self.logs.push(HostLog { level, message });
        Ok(())
    }
}

struct PluginProcess {
    child: Child,
    stdin: ChildStdin,
    frames: Receiver<Result<String, FrameError>>,
    frame_queue_overflowed: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl PluginProcess {
    fn spawn(plugin: &PluginDefinition) -> Result<Self, HostError> {
        let mut command = Command::new(&plugin.argv[0]);
        command
            .args(&plugin.argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env_clear();
        if let Some(path) = env::var_os("PATH") {
            command.env("PATH", path);
        }
        if let Some(lang) = env::var_os("LANG") {
            command.env("LANG", lang);
        }
        for (key, value) in &plugin.env {
            command.env(key, value);
        }
        if let Some(working_dir) = &plugin.working_dir {
            command.current_dir(working_dir);
        }

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // The child starts a new session/process group so timeout/cancel
            // can terminate descendants without involving a shell.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
        }

        let mut child = command.spawn().map_err(HostError::Io)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| HostError::Protocol("failed to capture plugin stdin".to_string()))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| HostError::Protocol("failed to capture plugin stdout".to_string()))?;
        let (tx, rx) = mpsc::sync_channel(MAX_PENDING_REQUESTS);
        let frame_queue_overflowed = Arc::new(AtomicBool::new(false));
        let reader_frame_queue_overflowed = Arc::clone(&frame_queue_overflowed);
        let reader = thread::spawn(move || loop {
            let frame = read_frame(&mut stdout);
            let done = frame.is_err();
            match tx.try_send(frame) {
                Ok(()) => {
                    if done {
                        break;
                    }
                }
                Err(TrySendError::Full(_)) => {
                    reader_frame_queue_overflowed.store(true, Ordering::SeqCst);
                    break;
                }
                Err(TrySendError::Disconnected(_)) => break,
            }
        });

        Ok(Self {
            child,
            stdin,
            frames: rx,
            frame_queue_overflowed,
            reader: Some(reader),
        })
    }

    fn send(&mut self, value: &Value) -> Result<(), HostError> {
        let payload = canonical_json(value);
        write_frame(&mut self.stdin, &payload).map_err(HostError::Frame)
    }

    fn send_cancel(&mut self, request_id: &str) -> Result<(), HostError> {
        self.send(&message::notification(
            Method::Cancel,
            json!({"request_id": request_id}),
        ))
    }

    fn recv_frame(&mut self, deadline: Instant) -> Result<String, HostError> {
        let Some(wait) = deadline.checked_duration_since(Instant::now()) else {
            return Err(HostError::Timeout);
        };
        match self.frames.recv_timeout(wait) {
            Ok(Ok(payload)) => Ok(payload),
            Ok(Err(FrameError::Eof)) => {
                let status = self.child.try_wait().map_err(HostError::Io)?;
                Err(HostError::PluginExited(status))
            }
            Ok(Err(err)) => Err(HostError::Frame(err)),
            Err(RecvTimeoutError::Timeout) => Err(HostError::Timeout),
            Err(RecvTimeoutError::Disconnected) => {
                if self.frame_queue_overflowed.load(Ordering::SeqCst) {
                    return Err(HostError::FrameQueueFull);
                }
                let status = self.child.try_wait().map_err(HostError::Io)?;
                Err(HostError::PluginExited(status))
            }
        }
    }

    fn ensure_no_frames_after_response(&mut self) -> Result<(), HostError> {
        if self.frame_queue_overflowed.load(Ordering::SeqCst) {
            return Err(HostError::FrameQueueFull);
        }
        match self.frames.try_recv() {
            Ok(Ok(_)) => Err(HostError::Protocol(
                "plugin emitted protocol frame after discover response".to_string(),
            )),
            Ok(Err(FrameError::Eof)) => Ok(()),
            Ok(Err(err)) => Err(HostError::Frame(err)),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => Ok(()),
        }
    }

    fn wait_or_terminate(&mut self, grace: Duration) -> Result<Option<ExitStatus>, HostError> {
        let deadline = Instant::now() + grace;
        loop {
            if let Some(status) = self.child.try_wait().map_err(HostError::Io)? {
                self.join_reader();
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                self.terminate_tree(grace)?;
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn terminate_tree(&mut self, grace: Duration) -> Result<(), HostError> {
        if self.child.try_wait().map_err(HostError::Io)?.is_some() {
            self.join_reader();
            return Ok(());
        }

        #[cfg(unix)]
        {
            let pgid = self.child.id() as i32;
            unsafe {
                libc::kill(-pgid, libc::SIGTERM);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }

        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if self.child.try_wait().map_err(HostError::Io)?.is_some() {
                self.join_reader();
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }

        #[cfg(unix)]
        {
            let pgid = self.child.id() as i32;
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }

        let _ = self.child.wait();
        self.join_reader();
        Ok(())
    }

    fn join_reader(&mut self) {
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        let _ = self.terminate_tree(Duration::from_millis(20));
    }
}

/// Host runtime errors.
#[derive(Debug)]
pub enum HostError {
    Discovery(String),
    Frame(FrameError),
    Io(io::Error),
    Json(serde_json::Error),
    Fetch(HostFetchError),
    PluginError(Value),
    PluginDidNotExit,
    PluginExitStatus(ExitStatus),
    PluginExited(Option<ExitStatus>),
    FrameQueueFull,
    Protocol(String),
    State(StateError),
    Timeout,
    Validation(mh_domain::ValidationError),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::Discovery(err) => write!(f, "plugin discovery error: {err}"),
            HostError::Frame(err) => write!(f, "frame error: {err}"),
            HostError::Io(err) => write!(f, "io error: {err}"),
            HostError::Json(err) => write!(f, "json error: {err}"),
            HostError::Fetch(err) => write!(f, "host fetch error: {err}"),
            HostError::PluginError(err) => write!(f, "plugin returned error: {err}"),
            HostError::PluginDidNotExit => {
                write!(f, "plugin did not exit cleanly after discover response")
            }
            HostError::PluginExitStatus(status) => {
                write!(
                    f,
                    "plugin exited with non-zero status after discover: {status}"
                )
            }
            HostError::PluginExited(status) => {
                write!(f, "plugin exited before response: {status:?}")
            }
            HostError::FrameQueueFull => write!(f, "plugin frame queue exceeded host limit"),
            HostError::Protocol(err) => write!(f, "protocol error: {err}"),
            HostError::State(err) => write!(f, "state provider error: {err}"),
            HostError::Timeout => write!(f, "plugin timed out"),
            HostError::Validation(err) => write!(f, "record validation error: {err}"),
        }
    }
}

fn json_size(value: &Value) -> Result<usize, HostError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(HostError::Json)
}

fn parse_state_query(params: &Value) -> Result<(Value, StateOperation), HostError> {
    let query_id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| HostError::Protocol("state_query missing params.id".to_string()))?;
    let op = params
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| HostError::Protocol("state_query missing op".to_string()))?;
    let args = params.get("args").unwrap_or(&Value::Null);
    let operation = match op {
        "known_source_urls" => StateOperation::KnownSourceUrls {
            source_name: required_state_arg(args, op, "source_name")?,
        },
        "source_post_summary" => StateOperation::SourcePostSummary {
            source_name: required_state_arg(args, op, "source_name")?,
            source_url: required_state_arg(args, op, "source_url")?,
        },
        "last_seen_at" => StateOperation::LastSeenAt {
            source_name: required_state_arg(args, op, "source_name")?,
        },
        "content_fingerprint" => StateOperation::ContentFingerprint {
            source_name: required_state_arg(args, op, "source_name")?,
            source_url: required_state_arg(args, op, "source_url")?,
        },
        _ => {
            return Err(HostError::Protocol(format!("unknown state_query op {op}")));
        }
    };
    Ok((Value::String(query_id.to_string()), operation))
}

fn parse_fetch_request(params: &Value) -> Result<(Value, FetchRequest), HostError> {
    let request_id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| HostError::Protocol("fetch_request missing params.id".to_string()))?;
    if request_id.trim().is_empty() {
        return Err(HostError::Protocol(
            "fetch_request params.id is empty".to_string(),
        ));
    }
    let request_value = params
        .get("request")
        .ok_or_else(|| HostError::Protocol("fetch_request missing request".to_string()))?;
    let request: FetchRequest = serde_json::from_value(request_value.clone())
        .map_err(|err| HostError::Protocol(format!("invalid fetch_request request: {err}")))?;
    Ok((Value::String(request_id.to_string()), request))
}

fn required_state_arg(args: &Value, op: &str, name: &str) -> Result<String, HostError> {
    let value = args
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| HostError::Protocol(format!("state_query {op} missing args.{name}")))?;
    if value.trim().is_empty() {
        return Err(HostError::Protocol(format!(
            "state_query {op} args.{name} is empty"
        )));
    }
    Ok(value.to_string())
}

impl std::error::Error for HostError {}

fn require_jsonrpc(value: &Value) -> Result<(), HostError> {
    if value.get("jsonrpc").and_then(Value::as_str) == Some("2.0") {
        Ok(())
    } else {
        Err(HostError::Protocol(
            "message missing jsonrpc = 2.0".to_string(),
        ))
    }
}

fn require_notification(value: &Value, method: &str) -> Result<(), HostError> {
    if value.get("id").is_some() {
        return Err(HostError::Protocol(format!(
            "method {method} must be a notification"
        )));
    }
    Ok(())
}

fn require_discover_phase(
    manifest: Option<&PluginManifest>,
    method: &str,
) -> Result<(), HostError> {
    if manifest.is_none() {
        return Err(HostError::Protocol(format!(
            "method {method} is only valid during discover"
        )));
    }
    Ok(())
}

fn require_plugin_request_id(id: &Value, method: &str) -> Result<(), HostError> {
    let Some(id) = id.as_str() else {
        return Err(HostError::Protocol(format!(
            "{method} id must be a string in the plugin namespace"
        )));
    };
    if !id.starts_with("p-") {
        return Err(HostError::Protocol(format!(
            "{method} id must use plugin namespace p-*"
        )));
    }
    Ok(())
}

fn remaining_ms(deadline: Instant) -> u64 {
    deadline
        .checked_duration_since(Instant::now())
        .unwrap_or_else(|| Duration::from_millis(0))
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("mh-host-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_plugin(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("plugin.py");
        fs::write(&path, body).unwrap();
        path
    }

    fn python() -> String {
        env::var("PYTHON").unwrap_or_else(|_| "python3".to_string())
    }

    fn write_manifest(dir: &Path, id: &str, argv: &[String]) {
        let manifest = json!({"id": id, "argv": argv});
        fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn assert_protocol_error_contains(result: Result<HostRun, HostError>, expected: &str) {
        match result {
            Err(HostError::Protocol(message)) => {
                assert!(
                    message.contains(expected),
                    "protocol error {message:?} did not contain {expected:?}"
                );
            }
            other => panic!("expected protocol error containing {expected:?}, got {other:?}"),
        }
    }

    fn assert_protocol_unit_error_contains(result: Result<(), HostError>, expected: &str) {
        match result {
            Err(HostError::Protocol(message)) => {
                assert!(
                    message.contains(expected),
                    "protocol error {message:?} did not contain {expected:?}"
                );
            }
            other => panic!("expected protocol error containing {expected:?}, got {other:?}"),
        }
    }

    struct TestStateProvider;

    impl StateProvider for TestStateProvider {
        fn query(&self, op: StateOperation) -> Result<Value, StateError> {
            Ok(match op {
                StateOperation::KnownSourceUrls { source_name } => {
                    assert_eq!(source_name, "synthetic");
                    json!(["synthetic://post/1", "synthetic://post/2"])
                }
                StateOperation::SourcePostSummary {
                    source_name,
                    source_url,
                } => {
                    assert_eq!(source_name, "synthetic");
                    assert_eq!(source_url, "synthetic://post/1");
                    json!({
                        "exists": true,
                        "title": "Synthetic One",
                        "last_seen_at": "2026-06-25T00:00:00.000Z"
                    })
                }
                StateOperation::LastSeenAt { source_name } => {
                    assert_eq!(source_name, "synthetic");
                    json!("2026-06-25T00:00:00.000Z")
                }
                StateOperation::ContentFingerprint {
                    source_name,
                    source_url,
                } => {
                    assert_eq!(source_name, "synthetic");
                    assert_eq!(source_url, "synthetic://post/1");
                    json!("fnv1a64:0000000000000001")
                }
            })
        }
    }

    struct TestFetchProvider;

    impl FetchProvider for TestFetchProvider {
        fn fetch(
            &self,
            manifest: &PluginManifest,
            request: FetchRequest,
            timeout: Duration,
        ) -> Result<FetchResponse, HostFetchError> {
            assert!(timeout <= Duration::from_secs(5));
            assert_eq!(manifest.allowed_domains, vec!["example.test"]);
            assert_eq!(request.url, "https://example.test/page");
            assert_eq!(request.method, "GET");
            assert_eq!(
                request.headers.get("Accept").map(String::as_str),
                Some("*/*")
            );
            Ok(FetchResponse {
                status: 200,
                final_url: "https://example.test/page".to_string(),
                body_base64: "b2s=".to_string(),
            })
        }
    }

    struct TestNoFetchProvider;

    impl FetchProvider for TestNoFetchProvider {
        fn fetch(
            &self,
            _manifest: &PluginManifest,
            _request: FetchRequest,
            _timeout: Duration,
        ) -> Result<FetchResponse, HostFetchError> {
            Err(HostFetchError::unavailable("host_fetch is not configured"))
        }
    }

    #[test]
    fn discovers_plugins_d_json_manifests() {
        let dir = temp_dir("discover");
        let plugin = write_plugin(&dir, "print('not executed')\n");
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        fs::write(dir.join("ignore.txt"), "{}").unwrap();

        let plugins = discover_plugins(&dir).unwrap();

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "synthetic");
        assert_eq!(plugins[0].argv[0], python);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_duplicate_plugin_ids() {
        let dir = temp_dir("duplicate-id");
        let plugin = write_plugin(&dir, "print('not executed')\n");
        let python = python();
        let argv = &[python.clone(), plugin.to_string_lossy().to_string()];
        write_manifest(&dir, "synthetic", argv);
        fs::write(
            dir.join("synthetic-copy.json"),
            serde_json::to_string_pretty(&json!({"id": "synthetic", "argv": argv})).unwrap(),
        )
        .unwrap();

        let err = discover_plugins(&dir).unwrap_err();

        assert!(err.to_string().contains("duplicate plugin id"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn runs_initialize_discover_and_spools_records() {
        let dir = temp_dir("run");
        let plugin = write_plugin(
            &dir,
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
request_id = discover["params"]["request_id"]
write_frame({"jsonrpc": "2.0", "method": "log", "params": {"level": "info", "message": "discovering"}})
write_frame({"jsonrpc": "2.0", "method": "record", "params": {"request_id": request_id, "record": {
    "source_name": "synthetic",
    "source_url": "synthetic://post/1",
    "title": "Synthetic One",
    "brand_raw": "Synthetic Brand",
    "performers_raw": ["Alice"],
    "cover_urls": ["https://example.test/cover.jpg"],
    "page_urls": ["https://example.test/page/1"],
    "external_links": [],
    "issue_no": "1",
    "release_date": "2026-06-25",
    "post_date": None,
    "brand_normalized": None,
    "normalizer_id": None,
    "normalizer_version": None,
    "extra": {}
}}})
write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": 1}})
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let run = PluginHost::default()
            .run_discover(
                &plugins[0],
                "run-1",
                DiscoverLimits::default(),
                Duration::from_secs(5),
            )
            .unwrap();

        assert_eq!(run.manifest.source_name, "synthetic");
        assert_eq!(run.discover_records, 1);
        assert_eq!(run.records.len(), 1);
        assert_eq!(run.records[0].source_url, "synthetic://post/1");
        assert_eq!(run.logs[0].message, "discovering");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn state_query_returns_provider_values_during_discover_loop() {
        let dir = temp_dir("state-query");
        let plugin = write_plugin(
            &dir,
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
write_frame({"jsonrpc": "2.0", "id": "p-1", "method": "state_query", "params": {
    "id": "known",
    "op": "known_source_urls",
    "args": {"source_name": "synthetic"}
}})
known_response = read_frame()
write_frame({"jsonrpc": "2.0", "id": "p-2", "method": "state_query", "params": {
    "id": "summary",
    "op": "source_post_summary",
    "args": {"source_name": "synthetic", "source_url": "synthetic://post/1"}
}})
summary_response = read_frame()
write_frame({"jsonrpc": "2.0", "id": "p-3", "method": "state_query", "params": {
    "id": "seen",
    "op": "last_seen_at",
    "args": {"source_name": "synthetic"}
}})
seen_response = read_frame()
write_frame({"jsonrpc": "2.0", "id": "p-4", "method": "state_query", "params": {
    "id": "fingerprint",
    "op": "content_fingerprint",
    "args": {"source_name": "synthetic", "source_url": "synthetic://post/1"}
}})
fingerprint_response = read_frame()
write_frame({"jsonrpc": "2.0", "method": "log", "params": {
    "level": "info",
    "message": json.dumps([
        known_response,
        summary_response,
        seen_response,
        fingerprint_response
    ], sort_keys=True)
}})
write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": 0}})
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let run = PluginHost::default()
            .run_discover_with_state_provider(
                &plugins[0],
                "run-state-query",
                DiscoverLimits::default(),
                Duration::from_secs(5),
                &TestStateProvider,
            )
            .unwrap();

        let responses: Value = serde_json::from_str(&run.logs[0].message).unwrap();
        assert_eq!(responses[0]["id"], json!("p-1"));
        assert_eq!(responses[0]["result"]["id"], json!("known"));
        assert_eq!(
            responses[0]["result"]["result"],
            json!(["synthetic://post/1", "synthetic://post/2"])
        );
        assert_eq!(responses[1]["result"]["id"], json!("summary"));
        assert_eq!(responses[1]["result"]["result"]["exists"], json!(true));
        assert_eq!(
            responses[1]["result"]["result"]["title"],
            json!("Synthetic One")
        );
        assert_eq!(
            responses[2]["result"]["result"],
            json!("2026-06-25T00:00:00.000Z")
        );
        assert_eq!(
            responses[3]["result"]["result"],
            json!("fnv1a64:0000000000000001")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fetch_request_returns_provider_response_during_discover_loop() {
        let dir = temp_dir("fetch-request");
        let plugin = write_plugin(
            &dir,
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
        "allowed_domains": ["example.test"],
        "capabilities": ["host_fetch"]
    }
}})

discover = read_frame()
write_frame({"jsonrpc": "2.0", "id": "p-1", "method": "fetch_request", "params": {
    "id": "fetch-1",
    "request": {
        "url": "https://example.test/page",
        "method": "GET",
        "headers": {"Accept": "*/*"}
    }
}})
fetch_response = read_frame()
write_frame({"jsonrpc": "2.0", "method": "log", "params": {
    "level": "info",
    "message": json.dumps(fetch_response, sort_keys=True)
}})
write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": 0}})
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let run = PluginHost::default()
            .run_discover_with_providers(
                &plugins[0],
                "run-fetch",
                DiscoverLimits::default(),
                Duration::from_secs(5),
                &NoStateProvider,
                &TestFetchProvider,
            )
            .unwrap();

        let response: Value = serde_json::from_str(&run.logs[0].message).unwrap();
        assert_eq!(response["id"], json!("p-1"));
        assert_eq!(response["result"]["id"], json!("fetch-1"));
        assert_eq!(response["result"]["status"], json!(200));
        assert_eq!(
            response["result"]["final_url"],
            json!("https://example.test/page")
        );
        assert_eq!(response["result"]["body_base64"], json!("b2s="));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fetch_request_without_provider_fails_closed() {
        let dir = temp_dir("fetch-request-no-provider");
        let plugin = write_plugin(
            &dir,
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
write_frame({"jsonrpc": "2.0", "id": init["id"], "result": {
    "protocol_version": 1,
    "record_schema_version": 1,
    "manifest": {
        "source_name": "synthetic",
        "display_label": "Synthetic",
        "allowed_domains": ["example.test"],
        "capabilities": ["host_fetch"]
    }
}})

read_frame()
write_frame({"jsonrpc": "2.0", "id": "p-1", "method": "fetch_request", "params": {
    "id": "fetch-1",
    "request": {"url": "https://example.test/page", "method": "GET", "headers": {}}
}})
time.sleep(30)
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let result = PluginHost::default().run_discover_with_providers(
            &plugins[0],
            "run-fetch-no-provider",
            DiscoverLimits::default(),
            Duration::from_secs(5),
            &NoStateProvider,
            &TestNoFetchProvider,
        );

        assert!(matches!(
            result,
            Err(HostError::Fetch(err)) if err.to_string().contains("not configured")
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn state_query_without_provider_fails_closed() {
        let dir = temp_dir("state-query-no-provider");
        let plugin = write_plugin(
            &dir,
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

read_frame()
write_frame({"jsonrpc": "2.0", "id": "p-1", "method": "state_query", "params": {
    "id": "known",
    "op": "known_source_urls",
    "args": {"source_name": "synthetic"}
}})
time.sleep(30)
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let result = PluginHost::default().run_discover(
            &plugins[0],
            "run-state-query-no-provider",
            DiscoverLimits::default(),
            Duration::from_secs(5),
        );

        assert!(matches!(
            result,
            Err(HostError::State(err)) if err.to_string().contains("not configured")
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn malformed_state_query_params_are_protocol_errors() {
        for (params, expected) in [
            (
                json!({"op": "known_source_urls", "args": {"source_name": "synthetic"}}),
                "params.id",
            ),
            (
                json!({"id": "known", "op": "unknown", "args": {}}),
                "unknown state_query op",
            ),
            (
                json!({"id": "known", "op": "known_source_urls", "args": {}}),
                "args.source_name",
            ),
            (
                json!({"id": "summary", "op": "source_post_summary", "args": {"source_name": "synthetic"}}),
                "args.source_url",
            ),
            (
                json!({"id": "fingerprint", "op": "content_fingerprint", "args": {"source_name": "", "source_url": "synthetic://post/1"}}),
                "args.source_name is empty",
            ),
        ] {
            match parse_state_query(&params) {
                Err(HostError::Protocol(message)) => assert!(
                    message.contains(expected),
                    "protocol error {message:?} did not contain {expected:?}"
                ),
                other => panic!("expected protocol error, got {other:?}"),
            }
        }

        assert_protocol_unit_error_contains(
            require_plugin_request_id(&json!("h-1"), "state_query"),
            "plugin namespace p-*",
        );
    }

    #[test]
    fn malformed_fetch_request_params_are_protocol_errors() {
        for (params, expected) in [
            (
                json!({"request": {"url": "https://example.test", "method": "GET", "headers": {}}}),
                "params.id",
            ),
            (
                json!({"id": "", "request": {"url": "https://example.test", "method": "GET", "headers": {}}}),
                "params.id is empty",
            ),
            (json!({"id": "fetch-1"}), "missing request"),
            (
                json!({"id": "fetch-1", "request": {"url": "https://example.test", "method": "GET", "headers": []}}),
                "invalid fetch_request request",
            ),
        ] {
            match parse_fetch_request(&params) {
                Err(HostError::Protocol(message)) => assert!(
                    message.contains(expected),
                    "protocol error {message:?} did not contain {expected:?}"
                ),
                other => panic!("expected protocol error, got {other:?}"),
            }
        }

        assert_protocol_unit_error_contains(
            require_plugin_request_id(&json!("h-1"), "fetch_request"),
            "plugin namespace p-*",
        );
    }

    #[test]
    fn rejects_state_query_before_discover() {
        let dir = temp_dir("early-state-query");
        let plugin = write_plugin(
            &dir,
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

read_frame()
write_frame({"jsonrpc": "2.0", "id": "p-early", "method": "state_query", "params": {
    "id": "known",
    "op": "known_source_urls"
}})
time.sleep(30)
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let result = PluginHost::default().run_discover(
            &plugins[0],
            "run-early-state-query",
            DiscoverLimits::default(),
            Duration::from_secs(5),
        );

        assert_protocol_error_contains(result, "only valid during discover");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn enforces_max_records_limit_while_spooling() {
        let dir = temp_dir("max-records");
        let plugin = write_plugin(
            &dir,
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
request_id = discover["params"]["request_id"]
write_frame({"jsonrpc": "2.0", "method": "record", "params": {"request_id": request_id, "record": {
    "source_name": "synthetic",
    "source_url": "synthetic://post/limit",
    "title": "Synthetic Limit",
    "brand_raw": "Synthetic Brand"
}}})
write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": 1}})
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let result = PluginHost::default().run_discover(
            &plugins[0],
            "run-max-records",
            DiscoverLimits {
                max_pages: None,
                max_records: Some(0),
            },
            Duration::from_secs(5),
        );

        assert_protocol_error_contains(result, "record spool exceeded max_records");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_discover_record_count_mismatch() {
        let dir = temp_dir("count-mismatch");
        let plugin = write_plugin(
            &dir,
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
write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": 1}})
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let result = PluginHost::default().run_discover(
            &plugins[0],
            "run-count-mismatch",
            DiscoverLimits::default(),
            Duration::from_secs(5),
        );

        assert_protocol_error_contains(result, "did not match spooled records");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_protocol_frames_after_discover_response() {
        let dir = temp_dir("frame-after-response");
        let plugin = write_plugin(
            &dir,
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
write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": 0}})
write_frame({"jsonrpc": "2.0", "method": "log", "params": {"level": "info", "message": "late"}})
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let result = PluginHost::default().run_discover(
            &plugins[0],
            "run-frame-after-response",
            DiscoverLimits::default(),
            Duration::from_secs(5),
        );

        assert_protocol_error_contains(result, "after discover response");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_nonzero_exit_after_discover_response() {
        let dir = temp_dir("nonzero-exit");
        let plugin = write_plugin(
            &dir,
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
write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": 0}})
sys.exit(7)
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let result = PluginHost::default().run_discover(
            &plugins[0],
            "run-nonzero-exit",
            DiscoverLimits::default(),
            Duration::from_secs(5),
        );

        assert!(matches!(
            result,
            Err(HostError::PluginExitStatus(status)) if !status.success()
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_plugin_that_does_not_exit_after_discover_response() {
        let dir = temp_dir("no-clean-exit");
        let plugin = write_plugin(
            &dir,
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
write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": 0}})
time.sleep(30)
"#,
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let result = PluginHost::default().run_discover(
            &plugins[0],
            "run-no-clean-exit",
            DiscoverLimits::default(),
            Duration::from_secs(5),
        );

        assert!(matches!(result, Err(HostError::PluginDidNotExit)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bounded_frame_queue_fails_closed() {
        let dir = temp_dir("frame-queue");
        let plugin = write_plugin(
            &dir,
            r#"
import json
import struct
import sys
import time

def write_frame(value):
    payload = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    sys.stdout.buffer.write(struct.pack(">I", len(payload)))
    sys.stdout.buffer.write(payload)
    sys.stdout.buffer.flush()

for i in range(32):
    write_frame({"jsonrpc": "2.0", "method": "log", "params": {"level": "info", "message": str(i)}})
time.sleep(30)
"#,
        );
        let definition = PluginDefinition {
            id: "synthetic".to_string(),
            argv: vec![python(), plugin.to_string_lossy().to_string()],
            env: BTreeMap::new(),
            working_dir: None,
            manifest_path: dir.join("synthetic.json"),
        };
        let mut process = PluginProcess::spawn(&definition).unwrap();
        thread::sleep(Duration::from_millis(200));
        let deadline = Instant::now() + Duration::from_secs(1);
        for _ in 0..MAX_PENDING_REQUESTS {
            process.recv_frame(deadline).unwrap();
        }

        assert!(matches!(
            process.recv_frame(deadline),
            Err(HostError::FrameQueueFull)
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn run_state_enforces_record_and_log_byte_limits() {
        let mut state = RunState {
            records: Vec::new(),
            logs: Vec::new(),
            max_records: Some(HOST_MAX_RECORDS),
            record_bytes: HOST_MAX_RECORD_BYTES,
            log_bytes: 0,
        };
        assert_protocol_unit_error_contains(
            state.reserve_record_values([&json!({"source_url": "synthetic://post/too-large"})]),
            "record spool exceeded byte limit",
        );

        let mut state = RunState {
            records: Vec::new(),
            logs: Vec::new(),
            max_records: Some(HOST_MAX_RECORDS),
            record_bytes: 0,
            log_bytes: HOST_MAX_LOG_BYTES,
        };
        assert_protocol_unit_error_contains(
            state.push_log("info".to_string(), "x".to_string()),
            "log spool exceeded byte limit",
        );
    }

    #[cfg(unix)]
    #[test]
    fn timeout_terminates_plugin_process_group() {
        let dir = temp_dir("timeout");
        let pid_file = dir.join("child.pid");
        let plugin = write_plugin(
            &dir,
            &format!(
                r#"
import json
import struct
import subprocess
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

read_frame()
child = subprocess.Popen(["sleep", "30"])
with open({pid_file:?}, "w") as f:
    f.write(str(child.pid))
    f.flush()
time.sleep(30)
"#
            ),
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let result = PluginHost::default().run_discover(
            &plugins[0],
            "run-timeout",
            DiscoverLimits::default(),
            Duration::from_millis(400),
        );

        assert!(matches!(result, Err(HostError::Timeout)));
        let child_pid: i32 = fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        for _ in 0..20 {
            if !process_exists(child_pid) {
                fs::remove_dir_all(dir).unwrap();
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("child process {child_pid} was still alive after host timeout");
    }

    #[test]
    fn timeout_sends_cancel_before_terminating() {
        let dir = temp_dir("cancel");
        let cancel_file = dir.join("cancel.seen");
        let plugin = write_plugin(
            &dir,
            &format!(
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

read_frame()
cancel = read_frame()
if cancel.get("method") == "cancel":
    with open({cancel_file:?}, "w") as f:
        f.write(cancel["params"]["request_id"])
"#
            ),
        );
        let python = python();
        write_manifest(
            &dir,
            "synthetic",
            &[python.clone(), plugin.to_string_lossy().to_string()],
        );
        let plugins = discover_plugins(&dir).unwrap();

        let result = PluginHost::default().run_discover(
            &plugins[0],
            "run-cancel",
            DiscoverLimits::default(),
            Duration::from_millis(200),
        );

        assert!(matches!(result, Err(HostError::Timeout)));
        assert_eq!(fs::read_to_string(&cancel_file).unwrap(), "run-cancel");
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    fn process_exists(pid: i32) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }
}
