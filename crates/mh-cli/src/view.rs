//! One-shot read commands over a core database.
//!
//! The bundled `mh ui` already exposes this data, but only to a browser on a
//! port that has to stay running. An external reader — an operator script, a
//! scheduler, an agent driving `mh` through a shell — needs a command that
//! answers once and exits. `mh inspect` returns whole-database counts only; the
//! per-source breakdown existed nowhere outside the HTTP surface.
//!
//! Three shapes, no more:
//!
//! * `sources` — what has been collected, per source
//! * `posts`   — a keyset page of post metadata, without URLs
//! * `assets`  — the URL groups for posts the caller names explicitly
//!
//! Splitting `posts` from `assets` is deliberate. A listing is the output most
//! likely to be forwarded somewhere else, and the URLs are the bulk of the
//! third-party data. Keeping them behind an explicit request means the caller
//! decides when that material leaves the machine.
//!
//! Output is a single JSON object on stdout. Errors go to stderr with a
//! non-zero exit, so a caller can branch on the exit code without parsing prose.

use std::error::Error;
use std::path::PathBuf;

use mh_db::Database;
use serde_json::json;

/// Default page size. Small enough that an unqualified call stays readable when
/// a caller pipes it straight into something with a context budget.
const DEFAULT_POSTS_LIMIT: u32 = 20;
/// Upper bound on one `assets` call, mirroring the page cap on `posts`.
const MAX_ASSET_IDS: usize = 50;

pub(crate) fn run_view(args: &[String]) -> Result<(), Box<dyn Error>> {
    let Some((subcommand, rest)) = args.split_first() else {
        return Err("view requires a subcommand: sources | posts | assets".into());
    };
    match subcommand.as_str() {
        "sources" => run_sources(rest),
        "posts" => run_posts(rest),
        "assets" => run_assets(rest),
        other => Err(format!("unknown view subcommand: {other}").into()),
    }
}

fn open_read_only(path: &str) -> Result<Database, Box<dyn Error>> {
    Ok(Database::open_read_only(PathBuf::from(path))?)
}

fn run_sources(args: &[String]) -> Result<(), Box<dyn Error>> {
    let [db_path] = args else {
        return Err("view sources requires <db-path>".into());
    };
    let db = open_read_only(db_path)?;
    let inspection = db.inspect()?;
    let sources = db.source_summaries()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version": inspection.schema_version,
            "totals": inspection,
            "sources": sources,
        }))?
    );
    Ok(())
}

fn run_posts(args: &[String]) -> Result<(), Box<dyn Error>> {
    let Some((db_path, flags)) = args.split_first() else {
        return Err("view posts requires <db-path>".into());
    };
    let mut limit = DEFAULT_POSTS_LIMIT;
    let mut after_id: Option<i64> = None;
    let mut source_name: Option<String> = None;
    let mut include_extra = false;

    let mut index = 0;
    while index < flags.len() {
        let flag = flags[index].as_str();
        match flag {
            "--include-extra" => {
                include_extra = true;
                index += 1;
            }
            "--limit" | "--after-id" | "--source" => {
                let value = flags
                    .get(index + 1)
                    .ok_or_else(|| format!("{flag} requires a value"))?;
                match flag {
                    "--limit" => {
                        limit = value
                            .parse::<u32>()
                            .map_err(|_| "--limit must be a positive integer".to_string())?;
                        if limit == 0 {
                            return Err("--limit must be a positive integer".into());
                        }
                    }
                    "--after-id" => {
                        after_id = Some(
                            value
                                .parse::<i64>()
                                .map_err(|_| "--after-id must be an integer".to_string())?,
                        );
                    }
                    _ => source_name = Some(value.clone()),
                }
                index += 2;
            }
            other => return Err(format!("unknown view posts option: {other}").into()),
        }
    }

    let db = open_read_only(db_path)?;
    let page = db.view_posts_page(limit, after_id, source_name.as_deref(), include_extra)?;
    println!("{}", serde_json::to_string_pretty(&page)?);
    Ok(())
}

fn run_assets(args: &[String]) -> Result<(), Box<dyn Error>> {
    let Some((db_path, flags)) = args.split_first() else {
        return Err("view assets requires <db-path> --post-ids <id,...>".into());
    };
    let mut raw_ids: Option<String> = None;
    let mut index = 0;
    while index < flags.len() {
        match flags[index].as_str() {
            "--post-ids" => {
                let value = flags
                    .get(index + 1)
                    .ok_or_else(|| "--post-ids requires a value".to_string())?;
                if raw_ids.is_some() {
                    return Err("--post-ids specified more than once".into());
                }
                raw_ids = Some(value.clone());
                index += 2;
            }
            other => return Err(format!("unknown view assets option: {other}").into()),
        }
    }
    let raw_ids = raw_ids.ok_or_else(|| "view assets requires --post-ids <id,...>".to_string())?;

    let mut post_ids = Vec::new();
    for token in raw_ids.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let id = token
            .parse::<i64>()
            .map_err(|_| format!("--post-ids entry is not an integer: {token}"))?;
        if !post_ids.contains(&id) {
            post_ids.push(id);
        }
    }
    if post_ids.is_empty() {
        return Err("--post-ids requires at least one id".into());
    }
    if post_ids.len() > MAX_ASSET_IDS {
        return Err(format!(
            "--post-ids accepts at most {MAX_ASSET_IDS} ids, got {}",
            post_ids.len()
        )
        .into());
    }

    let db = open_read_only(db_path)?;
    let assets = db.view_post_assets(&post_ids)?;
    println!("{}", serde_json::to_string_pretty(&assets)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn message(result: Result<(), Box<dyn Error>>) -> String {
        result.expect_err("expected an error").to_string()
    }

    #[test]
    fn view_requires_a_known_subcommand() {
        assert!(message(run_view(&[])).contains("sources | posts | assets"));
        assert!(message(run_view(&args(&["records"]))).contains("unknown view subcommand"));
    }

    #[test]
    fn view_rejects_malformed_options_before_touching_the_database() {
        // Every case below must fail on argument shape, not on a missing file,
        // so an operator typo never reads as "database problem".
        let cases: Vec<(Vec<String>, &str)> = vec![
            (args(&["posts"]), "requires <db-path>"),
            (args(&["posts", "db", "--limit"]), "requires a value"),
            (args(&["posts", "db", "--limit", "0"]), "positive integer"),
            (args(&["posts", "db", "--limit", "x"]), "positive integer"),
            (
                args(&["posts", "db", "--after-id", "x"]),
                "must be an integer",
            ),
            (args(&["posts", "db", "--source"]), "requires a value"),
            (
                args(&["posts", "db", "--nope"]),
                "unknown view posts option",
            ),
            (args(&["assets", "db"]), "requires --post-ids"),
            (args(&["assets", "db", "--post-ids"]), "requires a value"),
            (
                args(&["assets", "db", "--post-ids", ","]),
                "at least one id",
            ),
            (
                args(&["assets", "db", "--post-ids", "1,x"]),
                "not an integer",
            ),
            (
                args(&["assets", "db", "--nope", "1"]),
                "unknown view assets option",
            ),
            (args(&["sources"]), "requires <db-path>"),
            (args(&["sources", "a", "b"]), "requires <db-path>"),
        ];
        for (argv, expected) in cases {
            let actual = message(run_view(&argv));
            assert!(
                actual.contains(expected),
                "args {argv:?}: expected {expected:?}, got {actual:?}"
            );
        }
    }

    #[test]
    fn view_assets_bounds_the_id_list_and_drops_duplicates() {
        let too_many = (1..=MAX_ASSET_IDS + 1)
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let actual = message(run_view(&args(&["assets", "db", "--post-ids", &too_many])));
        assert!(actual.contains("at most"), "{actual}");

        // A duplicated id must not consume budget twice; this list is over the
        // cap only if duplicates are counted.
        let duplicated = std::iter::repeat_n("7", MAX_ASSET_IDS + 5)
            .collect::<Vec<_>>()
            .join(",");
        let actual = message(run_view(&args(&[
            "assets",
            "db",
            "--post-ids",
            &duplicated,
        ])));
        assert!(
            !actual.contains("at most"),
            "duplicates were counted against the cap: {actual}"
        );
    }

    #[test]
    fn view_rejects_a_repeated_post_ids_flag() {
        let actual = message(run_view(&args(&[
            "assets",
            "db",
            "--post-ids",
            "1",
            "--post-ids",
            "2",
        ])));
        assert!(actual.contains("more than once"), "{actual}");
    }
}
