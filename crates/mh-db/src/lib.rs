//! SQLite schema management and transactional ingestion for `SourceRecord`.
//!
//! The core DB schema is canonical and separate from downstream legacy DBs.
//! Opening an existing non-core DB fails closed so `init-db` cannot stamp an
//! incompatible production database as migrated.

use std::path::Path;
use std::path::PathBuf;

use mh_domain::{SourceRecord, ValidationError};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde::Serialize;
use serde_json::{json, Value};

const LATEST_SCHEMA_VERSION: i64 = 1;

const MIGRATION_0001: &str = r#"
CREATE TABLE IF NOT EXISTS mh_schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS source_posts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_name TEXT NOT NULL,
    source_url TEXT NOT NULL,
    title TEXT NOT NULL,
    brand_raw TEXT NOT NULL,
    issue_no TEXT,
    release_date TEXT,
    post_date TEXT,
    brand_normalized TEXT,
    normalizer_id TEXT,
    normalizer_version TEXT,
    content_fingerprint TEXT,
    extra_json TEXT NOT NULL DEFAULT '{}',
    first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (source_name, source_url)
);

CREATE TABLE IF NOT EXISTS source_post_performers (
    source_post_id INTEGER NOT NULL REFERENCES source_posts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    performer_raw TEXT NOT NULL,
    PRIMARY KEY (source_post_id, position)
);

CREATE TABLE IF NOT EXISTS source_post_covers (
    source_post_id INTEGER NOT NULL REFERENCES source_posts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    cover_url TEXT NOT NULL,
    PRIMARY KEY (source_post_id, position)
);

CREATE TABLE IF NOT EXISTS source_post_pages (
    source_post_id INTEGER NOT NULL REFERENCES source_posts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    page_url TEXT NOT NULL,
    PRIMARY KEY (source_post_id, position)
);

CREATE TABLE IF NOT EXISTS source_post_external_links (
    source_post_id INTEGER NOT NULL REFERENCES source_posts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    url TEXT NOT NULL,
    provider TEXT,
    label TEXT,
    kind TEXT,
    external_id TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (source_post_id, position)
);

CREATE INDEX IF NOT EXISTS idx_source_post_performers_raw
    ON source_post_performers(performer_raw);
CREATE INDEX IF NOT EXISTS idx_source_post_covers_url
    ON source_post_covers(cover_url);
CREATE INDEX IF NOT EXISTS idx_source_post_pages_url
    ON source_post_pages(page_url);
CREATE INDEX IF NOT EXISTS idx_source_post_external_links_url
    ON source_post_external_links(url);

INSERT OR IGNORE INTO mh_schema_migrations(version) VALUES (1);
"#;

/// A SQLite database handle with foreign keys enabled.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open a new core DB file or an existing core DB. Existing unknown DBs are
    /// rejected before migrations run.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DbError> {
        let path = path.as_ref();
        let exists = path.exists();
        let flags = if exists {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        };
        let conn = Connection::open_with_flags(path, flags)?;
        enable_foreign_keys(&conn)?;
        if exists {
            ensure_openable_for_init(&conn, path)?;
        }
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        enable_foreign_keys(&conn)?;
        Ok(Self { conn })
    }

    /// Inspect an existing core DB read-only. Missing files are not created.
    pub fn inspect_path(path: impl AsRef<Path>) -> Result<DbInspection, DbError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(DbError::MissingDatabase(path.to_path_buf()));
        }
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        ensure_openable_for_inspect(&conn, path)?;
        Database { conn }.inspect()
    }

    /// Apply all known migrations. This operation is idempotent.
    pub fn initialize(&self) -> Result<(), DbError> {
        self.conn.execute_batch(MIGRATION_0001)?;
        validate_core_schema(&self.conn)?;
        Ok(())
    }

    /// Return lightweight DB state for CLI and smoke tests.
    pub fn inspect(&self) -> Result<DbInspection, DbError> {
        Ok(DbInspection {
            schema_version: schema_version(&self.conn)?,
            source_posts: table_count(&self.conn, "source_posts")?,
            performers: table_count(&self.conn, "source_post_performers")?,
            covers: table_count(&self.conn, "source_post_covers")?,
            pages: table_count(&self.conn, "source_post_pages")?,
            external_links: table_count(&self.conn, "source_post_external_links")?,
        })
    }

    /// Return all known source URLs for a source, sorted for deterministic
    /// plugin skip decisions.
    pub fn known_source_urls(&self, source_name: &str) -> Result<Vec<String>, DbError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT source_url
            FROM source_posts
            WHERE source_name = ?1
            ORDER BY source_url
            "#,
        )?;
        let rows = stmt.query_map(params![source_name], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::Sqlite)
    }

    /// Return a lightweight post summary by opaque source URL.
    pub fn source_post_summary(
        &self,
        source_name: &str,
        source_url: &str,
    ) -> Result<Option<SourcePostSummary>, DbError> {
        self.conn
            .query_row(
                r#"
                SELECT title, last_seen_at
                FROM source_posts
                WHERE source_name = ?1 AND source_url = ?2
                LIMIT 1
                "#,
                params![source_name, source_url],
                |row| {
                    Ok(SourcePostSummary {
                        exists: true,
                        title: Some(row.get(0)?),
                        last_seen_at: Some(row.get(1)?),
                    })
                },
            )
            .optional()
            .map_err(DbError::Sqlite)
    }

    /// Return the most recent observation timestamp for a source.
    pub fn last_seen_at(&self, source_name: &str) -> Result<Option<String>, DbError> {
        self.conn
            .query_row(
                r#"
                SELECT last_seen_at
                FROM source_posts
                WHERE source_name = ?1
                ORDER BY last_seen_at DESC, id DESC
                LIMIT 1
                "#,
                params![source_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(DbError::Sqlite)
    }

    /// Return the stored content fingerprint for a source URL, if known.
    pub fn content_fingerprint(
        &self,
        source_name: &str,
        source_url: &str,
    ) -> Result<Option<String>, DbError> {
        self.conn
            .query_row(
                r#"
                SELECT content_fingerprint
                FROM source_posts
                WHERE source_name = ?1 AND source_url = ?2
                LIMIT 1
                "#,
                params![source_name, source_url],
                |row| row.get(0),
            )
            .optional()
            .map_err(DbError::Sqlite)
    }

    /// Ingest records in one transaction. Any validation or write failure
    /// rolls back the whole batch.
    pub fn ingest_records(&mut self, records: &[SourceRecord]) -> Result<IngestReport, DbError> {
        let tx = self.conn.transaction()?;
        for record in records {
            record.validate()?;
            ingest_one(&tx, record)?;
        }
        tx.commit()?;
        Ok(IngestReport {
            records: records.len(),
        })
    }
}

/// Summary returned by `inspect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DbInspection {
    pub schema_version: i64,
    pub source_posts: i64,
    pub performers: i64,
    pub covers: i64,
    pub pages: i64,
    pub external_links: i64,
}

/// Lightweight state returned to plugins via typed `state_query`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourcePostSummary {
    pub exists: bool,
    pub title: Option<String>,
    pub last_seen_at: Option<String>,
}

/// Transaction ingest result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestReport {
    pub records: usize,
}

/// Database and validation errors.
#[derive(Debug)]
pub enum DbError {
    DatabaseTooNew(i64),
    IncompatibleSchema {
        table: &'static str,
        missing_columns: Vec<&'static str>,
    },
    MissingTable(&'static str),
    NeedsMigration(PathBuf),
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    MissingDatabase(PathBuf),
    UnsupportedSchemaVersion(i64),
    UnknownExistingDatabase(PathBuf),
    Validation(ValidationError),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::DatabaseTooNew(version) => write!(
                f,
                "database schema version {version} is newer than supported version {LATEST_SCHEMA_VERSION}"
            ),
            DbError::IncompatibleSchema {
                table,
                missing_columns,
            } => write!(
                f,
                "incompatible core schema in {table}; missing columns: {}",
                missing_columns.join(", ")
            ),
            DbError::MissingTable(table) => {
                write!(f, "incompatible core schema; missing table: {table}")
            }
            DbError::NeedsMigration(path) => write!(
                f,
                "database is not initialized; run init-db first: {}",
                path.display()
            ),
            DbError::Sqlite(err) => write!(f, "sqlite error: {err}"),
            DbError::Json(err) => write!(f, "json error: {err}"),
            DbError::MissingDatabase(path) => {
                write!(f, "database does not exist: {}", path.display())
            }
            DbError::UnsupportedSchemaVersion(version) => {
                write!(f, "unsupported database schema version: {version}")
            }
            DbError::UnknownExistingDatabase(path) => write!(
                f,
                "refusing to initialize unknown existing database: {}",
                path.display()
            ),
            DbError::Validation(err) => write!(f, "validation error: {err}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(err: rusqlite::Error) -> Self {
        DbError::Sqlite(err)
    }
}

impl From<serde_json::Error> for DbError {
    fn from(err: serde_json::Error) -> Self {
        DbError::Json(err)
    }
}

impl From<ValidationError> for DbError {
    fn from(err: ValidationError) -> Self {
        DbError::Validation(err)
    }
}

fn enable_foreign_keys(conn: &Connection) -> Result<(), DbError> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn ingest_one(tx: &Transaction<'_>, record: &SourceRecord) -> Result<(), DbError> {
    let existing = existing_post(tx, record)?;
    let effective_release_date = record
        .release_date
        .clone()
        .or_else(|| existing.as_ref().and_then(|post| post.release_date.clone()));
    let content_fingerprint = content_fingerprint(record, effective_release_date.as_deref())?;
    let extra_json = serde_json::to_string(&record.extra)?;

    let source_post_id = if let Some(existing) = existing {
        let content_changed = existing.content_fingerprint.as_deref() != Some(&content_fingerprint);
        tx.execute(
            r#"
            UPDATE source_posts
            SET
                title = ?1,
                brand_raw = ?2,
                issue_no = ?3,
                release_date = ?4,
                post_date = ?5,
                brand_normalized = ?6,
                normalizer_id = ?7,
                normalizer_version = ?8,
                content_fingerprint = ?9,
                extra_json = ?10,
                last_seen_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                updated_at = CASE
                    WHEN ?11 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                    ELSE updated_at
                END
            WHERE id = ?12
            "#,
            params![
                record.title,
                record.brand_raw,
                record.issue_no,
                effective_release_date,
                record.post_date,
                record.brand_normalized,
                record.normalizer_id,
                record.normalizer_version,
                content_fingerprint,
                extra_json,
                if content_changed { 1 } else { 0 },
                existing.id
            ],
        )?;
        existing.id
    } else {
        tx.execute(
            r#"
            INSERT INTO source_posts (
                source_name,
                source_url,
                title,
                brand_raw,
                issue_no,
                release_date,
                post_date,
                brand_normalized,
                normalizer_id,
                normalizer_version,
                content_fingerprint,
                extra_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
            params![
                record.source_name,
                record.source_url,
                record.title,
                record.brand_raw,
                record.issue_no,
                effective_release_date,
                record.post_date,
                record.brand_normalized,
                record.normalizer_id,
                record.normalizer_version,
                content_fingerprint,
                extra_json
            ],
        )?;
        tx.last_insert_rowid()
    };

    replace_values(
        tx,
        "source_post_performers",
        "performer_raw",
        source_post_id,
        &record.performers_raw,
    )?;
    replace_values(
        tx,
        "source_post_covers",
        "cover_url",
        source_post_id,
        &record.cover_urls,
    )?;
    replace_values(
        tx,
        "source_post_pages",
        "page_url",
        source_post_id,
        &record.page_urls,
    )?;
    replace_external_links(tx, source_post_id, record)?;
    Ok(())
}

struct ExistingPost {
    id: i64,
    release_date: Option<String>,
    content_fingerprint: Option<String>,
}

fn existing_post(
    tx: &Transaction<'_>,
    record: &SourceRecord,
) -> Result<Option<ExistingPost>, DbError> {
    tx.query_row(
        r#"
        SELECT id, release_date, content_fingerprint
        FROM source_posts
        WHERE source_name = ?1 AND source_url = ?2
        "#,
        params![record.source_name, record.source_url],
        |row| {
            Ok(ExistingPost {
                id: row.get(0)?,
                release_date: row.get(1)?,
                content_fingerprint: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(DbError::Sqlite)
}

fn content_fingerprint(
    record: &SourceRecord,
    effective_release_date: Option<&str>,
) -> Result<String, DbError> {
    let value = json!({
        "source_name": record.source_name,
        "source_url": record.source_url,
        "title": record.title,
        "brand_raw": record.brand_raw,
        "performers_raw": record.performers_raw,
        "cover_urls": record.cover_urls,
        "page_urls": record.page_urls,
        "external_links": record.external_links,
        "issue_no": record.issue_no,
        "release_date": effective_release_date,
        "post_date": record.post_date,
        "brand_normalized": record.brand_normalized,
        "normalizer_id": record.normalizer_id,
        "normalizer_version": record.normalizer_version,
        "extra": record.extra,
    });
    let canonical = canonical_json(&value)?;
    Ok(format!("fnv1a64:{:016x}", fnv1a64(canonical.as_bytes())))
}

fn canonical_json(value: &Value) -> Result<String, DbError> {
    fn sort(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for key in keys {
                    out.insert(key.clone(), sort(&map[key]));
                }
                Value::Object(out)
            }
            Value::Array(items) => Value::Array(items.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(value)).map_err(DbError::Json)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn replace_values(
    tx: &Transaction<'_>,
    table: &'static str,
    value_column: &'static str,
    source_post_id: i64,
    values: &[String],
) -> Result<(), DbError> {
    tx.execute(
        &format!("DELETE FROM {table} WHERE source_post_id = ?1"),
        params![source_post_id],
    )?;
    let sql = format!(
        "INSERT INTO {table} (source_post_id, position, {value_column}) VALUES (?1, ?2, ?3)"
    );
    for (position, value) in values.iter().enumerate() {
        tx.execute(&sql, params![source_post_id, position as i64, value])?;
    }
    Ok(())
}

fn replace_external_links(
    tx: &Transaction<'_>,
    source_post_id: i64,
    record: &SourceRecord,
) -> Result<(), DbError> {
    tx.execute(
        "DELETE FROM source_post_external_links WHERE source_post_id = ?1",
        params![source_post_id],
    )?;
    for (position, link) in record.external_links.iter().enumerate() {
        let metadata_json = serde_json::to_string(&link.metadata)?;
        tx.execute(
            r#"
            INSERT INTO source_post_external_links (
                source_post_id,
                position,
                url,
                provider,
                label,
                kind,
                external_id,
                metadata_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                source_post_id,
                position as i64,
                link.url,
                link.provider,
                link.label,
                link.kind,
                link.external_id,
                metadata_json
            ],
        )?;
    }
    Ok(())
}

fn schema_version(conn: &Connection) -> Result<i64, DbError> {
    if !table_exists(conn, "mh_schema_migrations")? {
        return Ok(0);
    }
    Ok(conn
        .query_row("SELECT MAX(version) FROM mh_schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })?
        .unwrap_or(0))
}

fn ensure_openable_for_init(conn: &Connection, path: &Path) -> Result<(), DbError> {
    match schema_version(conn)? {
        0 if user_table_count(conn)? == 0 => Ok(()),
        0 => Err(DbError::UnknownExistingDatabase(path.to_path_buf())),
        LATEST_SCHEMA_VERSION => validate_schema_for_version(conn, LATEST_SCHEMA_VERSION),
        version if version > LATEST_SCHEMA_VERSION => Err(DbError::DatabaseTooNew(version)),
        version => validate_schema_for_version(conn, version),
    }
}

fn ensure_openable_for_inspect(conn: &Connection, path: &Path) -> Result<(), DbError> {
    match schema_version(conn)? {
        0 if user_table_count(conn)? == 0 => Err(DbError::NeedsMigration(path.to_path_buf())),
        0 => Err(DbError::UnknownExistingDatabase(path.to_path_buf())),
        LATEST_SCHEMA_VERSION => validate_schema_for_version(conn, LATEST_SCHEMA_VERSION),
        version if version > LATEST_SCHEMA_VERSION => Err(DbError::DatabaseTooNew(version)),
        version => validate_schema_for_version(conn, version),
    }
}

fn validate_core_schema(conn: &Connection) -> Result<(), DbError> {
    validate_schema_for_version(conn, LATEST_SCHEMA_VERSION)
}

fn validate_schema_for_version(conn: &Connection, version: i64) -> Result<(), DbError> {
    match version {
        1 => validate_schema_tables(conn, EXPECTED_CORE_COLUMNS),
        other if other > LATEST_SCHEMA_VERSION => Err(DbError::DatabaseTooNew(other)),
        other => Err(DbError::UnsupportedSchemaVersion(other)),
    }
}

fn validate_schema_tables(
    conn: &Connection,
    expected: &[(&'static str, &[&'static str])],
) -> Result<(), DbError> {
    for (table, columns) in expected {
        if !table_exists(conn, table)? {
            return Err(DbError::MissingTable(table));
        }
        validate_table_columns(conn, table, columns)?;
    }
    Ok(())
}

const EXPECTED_CORE_COLUMNS: &[(&str, &[&str])] = &[
    ("mh_schema_migrations", &["version", "applied_at"]),
    (
        "source_posts",
        &[
            "id",
            "source_name",
            "source_url",
            "title",
            "brand_raw",
            "issue_no",
            "release_date",
            "post_date",
            "brand_normalized",
            "normalizer_id",
            "normalizer_version",
            "content_fingerprint",
            "extra_json",
            "first_seen_at",
            "last_seen_at",
            "updated_at",
        ],
    ),
    (
        "source_post_performers",
        &["source_post_id", "position", "performer_raw"],
    ),
    (
        "source_post_covers",
        &["source_post_id", "position", "cover_url"],
    ),
    (
        "source_post_pages",
        &["source_post_id", "position", "page_url"],
    ),
    (
        "source_post_external_links",
        &[
            "source_post_id",
            "position",
            "url",
            "provider",
            "label",
            "kind",
            "external_id",
            "metadata_json",
        ],
    ),
];

fn validate_table_columns(
    conn: &Connection,
    table: &'static str,
    expected_columns: &[&'static str],
) -> Result<(), DbError> {
    let actual_columns = table_columns(conn, table)?;
    let missing_columns: Vec<&'static str> = expected_columns
        .iter()
        .copied()
        .filter(|expected| !actual_columns.iter().any(|actual| actual == expected))
        .collect();
    if missing_columns.is_empty() {
        Ok(())
    } else {
        Err(DbError::IncompatibleSchema {
            table,
            missing_columns,
        })
    }
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::Sqlite)
}

fn user_table_count(conn: &Connection) -> Result<i64, DbError> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get(0),
    )?)
}

fn table_count(conn: &Connection, table: &'static str) -> Result<i64, DbError> {
    if !table_exists(conn, table)? {
        return Ok(0);
    }
    Ok(
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?,
    )
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, DbError> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(exists)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_domain::ExternalLink;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("mh-db-{name}-{stamp}.db"))
    }

    fn record(source_url: &str) -> SourceRecord {
        SourceRecord {
            source_name: "synthetic".to_string(),
            source_url: source_url.to_string(),
            title: "Example title".to_string(),
            brand_raw: "Example brand".to_string(),
            performers_raw: vec!["Alice".to_string(), "Bob".to_string()],
            cover_urls: vec![
                "https://example.test/cover-1.jpg".to_string(),
                "https://example.test/cover-2.jpg".to_string(),
            ],
            page_urls: vec!["https://example.test/page/1".to_string()],
            external_links: vec![ExternalLink {
                url: "https://retail.example.test/item/1".to_string(),
                provider: Some("retail".to_string()),
                label: Some("Retail".to_string()),
                kind: Some("retail".to_string()),
                external_id: Some("X1".to_string()),
                metadata: serde_json::Map::new(),
            }],
            issue_no: Some("42".to_string()),
            release_date: Some("2026-06-25".to_string()),
            post_date: Some("2026-06-26".to_string()),
            brand_normalized: Some("Example Brand".to_string()),
            normalizer_id: Some("example".to_string()),
            normalizer_version: Some("1.0.0".to_string()),
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn initialize_creates_empty_schema() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.inspect().unwrap().schema_version, 0);
        db.initialize().unwrap();

        assert_eq!(
            db.inspect().unwrap(),
            DbInspection {
                schema_version: 1,
                source_posts: 0,
                performers: 0,
                covers: 0,
                pages: 0,
                external_links: 0,
            }
        );
    }

    #[test]
    fn inspect_path_requires_existing_core_db_and_does_not_create_files() {
        let path = temp_db_path("inspect-missing");
        assert!(!path.exists());

        let missing = Database::inspect_path(&path);

        assert!(matches!(missing, Err(DbError::MissingDatabase(_))));
        assert!(!path.exists());

        let db = Database::open(&path).unwrap();
        db.initialize().unwrap();
        drop(db);

        assert_eq!(Database::inspect_path(&path).unwrap().schema_version, 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn open_rejects_unknown_existing_legacy_db() {
        let path = temp_db_path("legacy");
        let legacy = Connection::open(&path).unwrap();
        legacy
            .execute_batch(
                r#"
                CREATE TABLE source_posts (
                    id INTEGER PRIMARY KEY,
                    source TEXT NOT NULL
                );
                CREATE TABLE source_post_performers (
                    source_post_id INTEGER NOT NULL,
                    name_raw TEXT,
                    name_normalized TEXT
                );
                CREATE TABLE source_post_covers (
                    source_post_id INTEGER NOT NULL,
                    image_url TEXT NOT NULL
                );
                "#,
            )
            .unwrap();
        drop(legacy);

        let result = Database::open(&path);

        assert!(matches!(result, Err(DbError::UnknownExistingDatabase(_))));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn open_rejects_v1_marker_with_missing_required_tables() {
        let path = temp_db_path("broken-v1");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE mh_schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            INSERT INTO mh_schema_migrations(version, applied_at)
            VALUES (1, '2026-06-25T00:00:00.000Z');
            "#,
        )
        .unwrap();
        drop(conn);

        let result = Database::open(&path);

        assert!(matches!(result, Err(DbError::MissingTable("source_posts"))));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn open_rejects_too_new_schema_version() {
        let path = temp_db_path("too-new");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE mh_schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            INSERT INTO mh_schema_migrations(version, applied_at)
            VALUES (999, '2026-06-25T00:00:00.000Z');
            "#,
        )
        .unwrap();
        drop(conn);

        let result = Database::open(&path);

        assert!(matches!(result, Err(DbError::DatabaseTooNew(999))));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn empty_existing_file_can_be_initialized_but_not_inspected_first() {
        let path = temp_db_path("empty-existing");
        fs::write(&path, "").unwrap();

        let inspect = Database::inspect_path(&path);
        assert!(matches!(inspect, Err(DbError::NeedsMigration(_))));

        let db = Database::open(&path).unwrap();
        db.initialize().unwrap();
        drop(db);

        assert_eq!(Database::inspect_path(&path).unwrap().schema_version, 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn ingest_is_idempotent_by_source_name_and_url() {
        let mut db = Database::open_in_memory().unwrap();
        db.initialize().unwrap();
        let mut record = record("synthetic://post/1");

        db.ingest_records(&[record.clone()]).unwrap();
        db.ingest_records(&[record.clone()]).unwrap();

        assert_eq!(
            db.inspect().unwrap(),
            DbInspection {
                schema_version: 1,
                source_posts: 1,
                performers: 2,
                covers: 2,
                pages: 1,
                external_links: 1,
            }
        );

        record.title = "Updated title".to_string();
        record.performers_raw = vec!["Carol".to_string()];
        record.external_links = Vec::new();
        db.ingest_records(&[record]).unwrap();

        assert_eq!(db.inspect().unwrap().source_posts, 1);
        assert_eq!(db.inspect().unwrap().performers, 1);
        assert_eq!(db.inspect().unwrap().external_links, 0);
        let title: String = db
            .conn
            .query_row("SELECT title FROM source_posts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(title, "Updated title");
    }

    #[test]
    fn ingest_preserves_release_date_and_updates_timestamp_only_on_content_change() {
        let mut db = Database::open_in_memory().unwrap();
        db.initialize().unwrap();
        let mut record = record("synthetic://post/1");
        db.ingest_records(&[record.clone()]).unwrap();
        db.conn
            .execute(
                "UPDATE source_posts SET updated_at = '2000-01-01T00:00:00.000Z'",
                [],
            )
            .unwrap();

        record.release_date = None;
        db.ingest_records(&[record.clone()]).unwrap();

        let stable: (Option<String>, String, Option<String>) = db
            .conn
            .query_row(
                "SELECT release_date, updated_at, content_fingerprint FROM source_posts",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(stable.0.as_deref(), Some("2026-06-25"));
        assert_eq!(stable.1, "2000-01-01T00:00:00.000Z");
        assert!(stable.2.is_some());

        record.title = "Changed title".to_string();
        db.ingest_records(&[record]).unwrap();

        let changed: (String, Option<String>) = db
            .conn
            .query_row(
                "SELECT updated_at, content_fingerprint FROM source_posts",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_ne!(changed.0, "2000-01-01T00:00:00.000Z");
        assert_ne!(changed.1, stable.2);
    }

    #[test]
    fn state_queries_return_db_backed_values() {
        let mut db = Database::open_in_memory().unwrap();
        db.initialize().unwrap();
        db.ingest_records(&[record("synthetic://post/2"), record("synthetic://post/1")])
            .unwrap();
        let before_inspect = db.inspect().unwrap();
        let before_last_seen = db.last_seen_at("synthetic").unwrap();

        assert_eq!(
            db.known_source_urls("synthetic").unwrap(),
            vec![
                "synthetic://post/1".to_string(),
                "synthetic://post/2".to_string()
            ]
        );
        assert!(db.known_source_urls("missing").unwrap().is_empty());

        let summary = db
            .source_post_summary("synthetic", "synthetic://post/1")
            .unwrap()
            .unwrap();
        assert_eq!(
            summary,
            SourcePostSummary {
                exists: true,
                title: Some("Example title".to_string()),
                last_seen_at: summary.last_seen_at.clone(),
            }
        );
        assert!(summary.last_seen_at.unwrap().ends_with('Z'));
        assert!(db
            .source_post_summary("synthetic", "synthetic://missing")
            .unwrap()
            .is_none());

        assert!(db
            .last_seen_at("synthetic")
            .unwrap()
            .unwrap()
            .ends_with('Z'));
        assert!(db.last_seen_at("missing").unwrap().is_none());

        let fingerprint = db
            .content_fingerprint("synthetic", "synthetic://post/1")
            .unwrap()
            .unwrap();
        assert!(fingerprint.starts_with("fnv1a64:"));
        assert!(db
            .content_fingerprint("synthetic", "synthetic://missing")
            .unwrap()
            .is_none());
        assert_eq!(db.inspect().unwrap(), before_inspect);
        assert_eq!(db.last_seen_at("synthetic").unwrap(), before_last_seen);
    }

    #[test]
    fn source_url_state_queries_are_scoped_by_source_name() {
        let mut db = Database::open_in_memory().unwrap();
        db.initialize().unwrap();
        let shared_url = "https://example.test/shared";
        let mut first = record(shared_url);
        first.source_name = "source-a".to_string();
        first.title = "A title".to_string();
        let mut second = record(shared_url);
        second.source_name = "source-b".to_string();
        second.title = "B title".to_string();
        db.ingest_records(&[first, second]).unwrap();

        assert_eq!(
            db.source_post_summary("source-a", shared_url)
                .unwrap()
                .unwrap()
                .title
                .as_deref(),
            Some("A title")
        );
        assert_eq!(
            db.source_post_summary("source-b", shared_url)
                .unwrap()
                .unwrap()
                .title
                .as_deref(),
            Some("B title")
        );
        assert_ne!(
            db.content_fingerprint("source-a", shared_url).unwrap(),
            db.content_fingerprint("source-b", shared_url).unwrap()
        );
    }

    #[test]
    fn ingest_rolls_back_full_batch_on_failure() {
        let mut db = Database::open_in_memory().unwrap();
        db.initialize().unwrap();
        let mut invalid = record("synthetic://post/bad");
        invalid.page_urls = vec!["/relative/page".to_string()];

        let result = db.ingest_records(&[record("synthetic://post/1"), invalid]);

        assert!(matches!(result, Err(DbError::Validation(_))));
        assert_eq!(db.inspect().unwrap().source_posts, 0);
        assert_eq!(db.inspect().unwrap().pages, 0);
    }
}
