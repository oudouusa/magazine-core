use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process;

use mh_db::Database;
use mh_host::{
    discover_plugins, DiscoverLimits, PluginHost, StateError, StateOperation, StateProvider,
};
use serde_json::{json, Value};

fn main() {
    if let Err(err) = run(env::args().skip(1).collect()) {
        eprintln!("error: {err}");
        eprintln!();
        eprintln!("{}", usage());
        process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    match args.as_slice() {
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{}", usage());
            Ok(())
        }
        [cmd, path] if cmd == "init-db" => {
            let db = Database::open(PathBuf::from(path))?;
            db.initialize()?;
            println!("{}", serde_json::to_string_pretty(&db.inspect()?)?);
            Ok(())
        }
        [cmd, path] if cmd == "inspect" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&Database::inspect_path(PathBuf::from(path))?)?
            );
            Ok(())
        }
        [cmd, rest @ ..] if cmd == "discover" => run_discover(rest),
        _ => Err("invalid arguments".into()),
    }
}

fn run_discover(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() < 3 {
        return Err("discover requires <db-path> <plugins-dir> <plugin-id>".into());
    }
    let db_path = &args[0];
    let plugins_dir = &args[1];
    let plugin_id = &args[2];
    let limits = parse_discover_limits(&args[3..])?;
    let mut db = Database::open(PathBuf::from(db_path))?;
    db.initialize()?;
    let plugins = discover_plugins(PathBuf::from(plugins_dir))?;
    let plugin = plugins
        .iter()
        .find(|plugin| plugin.id == *plugin_id)
        .ok_or_else(|| format!("plugin not found: {plugin_id}"))?;
    let run = {
        let state_provider = DbStateProvider { db: &db };
        PluginHost::default().run_discover_with_state_provider(
            plugin,
            "cli-run",
            limits,
            std::time::Duration::from_secs(60),
            &state_provider,
        )?
    };
    let ingest = db.ingest_records(&run.records)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "plugin_id": plugin.id,
            "source_name": run.manifest.source_name,
            "discover_records": run.discover_records,
            "spooled_records": run.records.len(),
            "ingested_records": ingest.records,
            "exit_status": run.exit_status.as_ref().and_then(|status| status.code()),
            "logs": run.logs.iter().map(|log| {
                json!({"level": log.level, "message": log.message})
            }).collect::<Vec<_>>()
        }))?
    );
    Ok(())
}

fn parse_discover_limits(args: &[String]) -> Result<DiscoverLimits, Box<dyn Error>> {
    let mut limits = DiscoverLimits::default();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let Some(value) = args.get(index + 1) else {
            return Err(format!("{flag} requires a value").into());
        };
        let parsed = parse_limit_value(flag, value)?;
        match flag {
            "--max-pages" => {
                if limits.max_pages.replace(parsed).is_some() {
                    return Err("--max-pages specified more than once".into());
                }
            }
            "--max-records" => {
                if limits.max_records.replace(parsed).is_some() {
                    return Err("--max-records specified more than once".into());
                }
            }
            "--per-page" => {
                if limits.per_page.replace(parsed).is_some() {
                    return Err("--per-page specified more than once".into());
                }
            }
            _ => return Err(format!("unknown discover option: {flag}").into()),
        }
        index += 2;
    }
    Ok(limits)
}

fn parse_limit_value(flag: &str, value: &str) -> Result<u64, Box<dyn Error>> {
    let parsed = value.parse::<u64>().map_err(|_| -> Box<dyn Error> {
        format!("{flag} must be a non-negative integer").into()
    })?;
    if matches!(flag, "--max-pages" | "--per-page") && parsed == 0 {
        return Err(format!("{flag} must be greater than zero").into());
    }
    Ok(parsed)
}

fn usage() -> &'static str {
    "Usage:\n  mh init-db <path>\n  mh inspect <path>\n  mh discover <db-path> <plugins-dir> <plugin-id> [--max-pages N] [--max-records N] [--per-page N]"
}

struct DbStateProvider<'a> {
    db: &'a Database,
}

impl StateProvider for DbStateProvider<'_> {
    fn query(&self, op: StateOperation) -> Result<Value, StateError> {
        match op {
            StateOperation::KnownSourceUrls { source_name } => self
                .db
                .known_source_urls(&source_name)
                .map(|urls| json!(urls)),
            StateOperation::SourcePostSummary {
                source_name,
                source_url,
            } => self
                .db
                .source_post_summary(&source_name, &source_url)
                .and_then(|summary| serde_json::to_value(summary).map_err(mh_db::DbError::from)),
            StateOperation::LastSeenAt { source_name } => self
                .db
                .last_seen_at(&source_name)
                .map(|last_seen| json!(last_seen)),
            StateOperation::ContentFingerprint {
                source_name,
                source_url,
            } => self
                .db
                .content_fingerprint(&source_name, &source_url)
                .map(|fingerprint| json!(fingerprint)),
        }
        .map_err(|err| StateError::backend(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_domain::SourceRecord;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mh-cli-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record(source_url: &str) -> SourceRecord {
        SourceRecord {
            source_name: "synthetic".to_string(),
            source_url: source_url.to_string(),
            title: "Existing title".to_string(),
            brand_raw: "Synthetic Brand".to_string(),
            performers_raw: Vec::new(),
            cover_urls: Vec::new(),
            page_urls: Vec::new(),
            external_links: Vec::new(),
            issue_no: None,
            release_date: None,
            post_date: None,
            brand_normalized: None,
            normalizer_id: None,
            normalizer_version: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn discover_uses_db_state_before_ingest() {
        let dir = temp_dir("db-state");
        let db_path = dir.join("core.db");
        let mut db = Database::open(&db_path).unwrap();
        db.initialize().unwrap();
        db.ingest_records(&[record("synthetic://post/existing")])
            .unwrap();
        drop(db);

        let plugin = dir.join("plugin.py");
        fs::write(
            &plugin,
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
write_frame({"jsonrpc": "2.0", "id": "p-1", "method": "state_query", "params": {
    "id": "known",
    "op": "known_source_urls",
    "args": {"source_name": "synthetic"}
}})
known = read_frame()["result"]["result"]
records = []
if "synthetic://post/existing" not in known:
    records.append("synthetic://post/existing")
records.append("synthetic://post/new")
for source_url in records:
    write_frame({"jsonrpc": "2.0", "method": "record", "params": {"request_id": request_id, "record": {
        "source_name": "synthetic",
        "source_url": source_url,
        "title": "Discovered title",
        "brand_raw": "Synthetic Brand"
    }}})
write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": len(records)}})
"#,
        )
        .unwrap();
        let plugins_dir = dir.join("plugins.d");
        fs::create_dir(&plugins_dir).unwrap();
        fs::write(
            plugins_dir.join("synthetic.json"),
            serde_json::to_string_pretty(&json!({
                "id": "synthetic",
                "argv": [std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()), plugin]
            }))
            .unwrap(),
        )
        .unwrap();

        run(vec![
            "discover".to_string(),
            db_path.to_string_lossy().to_string(),
            plugins_dir.to_string_lossy().to_string(),
            "synthetic".to_string(),
        ])
        .unwrap();

        let inspection = Database::inspect_path(&db_path).unwrap();
        assert_eq!(inspection.source_posts, 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn discover_forwards_cli_limits_to_plugin() {
        let dir = temp_dir("discover-limits");
        let db_path = dir.join("core.db");

        let plugin = dir.join("plugin.py");
        fs::write(
            &plugin,
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
expected = {"max_pages": 2, "max_records": 3, "per_page": 16}
if limits != expected:
    write_frame({"jsonrpc": "2.0", "id": discover["id"], "error": {
        "code": -32000,
        "message": json.dumps(limits, sort_keys=True)
    }})
else:
    write_frame({"jsonrpc": "2.0", "id": discover["id"], "result": {"records": 0}})
"#,
        )
        .unwrap();
        let plugins_dir = dir.join("plugins.d");
        fs::create_dir(&plugins_dir).unwrap();
        fs::write(
            plugins_dir.join("synthetic.json"),
            serde_json::to_string_pretty(&json!({
                "id": "synthetic",
                "argv": [std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()), plugin]
            }))
            .unwrap(),
        )
        .unwrap();

        run(vec![
            "discover".to_string(),
            db_path.to_string_lossy().to_string(),
            plugins_dir.to_string_lossy().to_string(),
            "synthetic".to_string(),
            "--max-pages".to_string(),
            "2".to_string(),
            "--max-records".to_string(),
            "3".to_string(),
            "--per-page".to_string(),
            "16".to_string(),
        ])
        .unwrap();

        let inspection = Database::inspect_path(&db_path).unwrap();
        assert_eq!(inspection.source_posts, 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parse_discover_limits_accepts_optional_flags() {
        let limits = parse_discover_limits(&[
            "--max-pages".to_string(),
            "2".to_string(),
            "--max-records".to_string(),
            "3".to_string(),
            "--per-page".to_string(),
            "16".to_string(),
        ])
        .unwrap();

        assert_eq!(
            limits,
            DiscoverLimits {
                max_pages: Some(2),
                per_page: Some(16),
                max_records: Some(3),
            }
        );
    }

    #[test]
    fn parse_discover_limits_rejects_unknown_duplicate_and_invalid_values() {
        assert!(parse_discover_limits(&["--unknown".to_string(), "1".to_string()]).is_err());
        assert!(
            parse_discover_limits(&["--max-pages".to_string(), "--per-page".to_string()]).is_err()
        );
        assert!(parse_discover_limits(&[
            "--max-records".to_string(),
            "1".to_string(),
            "--max-records".to_string(),
            "2".to_string(),
        ])
        .is_err());
        assert!(parse_discover_limits(&["--per-page".to_string(), "-1".to_string()]).is_err());
        assert!(parse_discover_limits(&["--max-pages".to_string(), "0".to_string()]).is_err());
        assert!(parse_discover_limits(&["--per-page".to_string(), "0".to_string()]).is_err());
        assert!(parse_discover_limits(&["--max-records".to_string(), "0".to_string()]).is_ok());
    }

    #[test]
    fn db_state_provider_maps_missing_values_to_json_null() {
        let db = Database::open_in_memory().unwrap();
        db.initialize().unwrap();
        let provider = DbStateProvider { db: &db };

        assert_eq!(
            provider
                .query(StateOperation::SourcePostSummary {
                    source_name: "synthetic".to_string(),
                    source_url: "synthetic://missing".to_string(),
                })
                .unwrap(),
            Value::Null
        );
        assert_eq!(
            provider
                .query(StateOperation::ContentFingerprint {
                    source_name: "synthetic".to_string(),
                    source_url: "synthetic://missing".to_string(),
                })
                .unwrap(),
            Value::Null
        );
    }
}
