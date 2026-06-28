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
        [cmd, db_path, plugins_dir, plugin_id] if cmd == "discover" => {
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
                    DiscoverLimits::default(),
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
        _ => Err("invalid arguments".into()),
    }
}

fn usage() -> &'static str {
    "Usage:\n  mh init-db <path>\n  mh inspect <path>\n  mh discover <db-path> <plugins-dir> <plugin-id>"
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
