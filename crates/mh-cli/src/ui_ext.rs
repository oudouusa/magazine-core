mod http;
mod registry;
mod shell;

use std::error::Error;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use mh_db::Database;

use self::http::ServerRole;
use self::registry::ExtensionRegistry;

const DEFAULT_SHELL_PORT: u16 = 8766;

#[derive(Debug, Clone)]
pub(super) struct Options {
    pub(super) db_path: PathBuf,
    pub(super) shell_port: u16,
    pub(super) asset_port: u16,
    pub(super) registry: Arc<ExtensionRegistry>,
}

impl Options {
    pub(super) fn shell_origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.shell_port)
    }

    pub(super) fn asset_origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.asset_port)
    }
}

#[derive(Debug)]
struct ParsedOptions {
    db_path: PathBuf,
    trusted_extensions_dir: PathBuf,
    shell_port: u16,
}

pub(crate) fn usage() -> &'static str {
    "Usage:\n  mh-ui-ext --db <core.db> --trusted-extensions-dir <extensions.d> [--port N]\n\nThe host is read-only. UI extension JavaScript is trusted local code and may transmit any data delivered to it."
}

pub(crate) fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    if matches!(args.as_slice(), [flag] if flag == "-h" || flag == "--help") {
        println!("{}", usage());
        return Ok(());
    }

    let parsed = parse_options(&args)?;
    Database::open_read_only(&parsed.db_path)?.inspect()?;
    let registry = Arc::new(ExtensionRegistry::load(&parsed.trusted_extensions_dir)?);

    let asset_listener = bind_listener(0)?;
    let asset_port = asset_listener.local_addr()?.port();
    let shell_listener = bind_listener(parsed.shell_port)?;
    let shell_port = shell_listener.local_addr()?.port();
    let options = Arc::new(Options {
        db_path: parsed.db_path,
        shell_port,
        asset_port,
        registry,
    });

    println!("mh-ui-ext listening on {}", options.shell_origin());
    eprintln!("mh-ui-ext extension assets on {}", options.asset_origin());
    eprintln!(
        "warning: trusted UI extensions are local executable content; they can read and transmit data delivered to them. This host is read-only and provides no management routes."
    );

    let asset_options = Arc::clone(&options);
    thread::Builder::new()
        .name("mh-ui-ext-assets".to_string())
        .spawn(move || http::serve(asset_listener, asset_options, ServerRole::Assets))?;
    http::serve(shell_listener, options, ServerRole::Shell);
    Ok(())
}

fn parse_options(args: &[String]) -> Result<ParsedOptions, Box<dyn Error>> {
    let mut db_path = None;
    let mut trusted_extensions_dir = None;
    let mut shell_port = DEFAULT_SHELL_PORT;
    let mut port_seen = false;
    let mut index = 0;

    while index < args.len() {
        let flag = args[index].as_str();
        if matches!(flag, "--host" | "--bind") {
            return Err(format!("{flag} is not accepted by the local-only extension host").into());
        }
        if flag == "--manage" {
            return Err("--manage is not supported by the trusted extension host".into());
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
            "--trusted-extensions-dir" => {
                if trusted_extensions_dir
                    .replace(PathBuf::from(value))
                    .is_some()
                {
                    return Err("--trusted-extensions-dir specified more than once".into());
                }
            }
            "--port" => {
                if port_seen {
                    return Err("--port specified more than once".into());
                }
                port_seen = true;
                shell_port = value
                    .parse::<u16>()
                    .map_err(|_| -> Box<dyn Error> { "--port must be a TCP port".into() })?;
            }
            _ => return Err(format!("unknown trusted extension host option: {flag}").into()),
        }
        index += 2;
    }

    Ok(ParsedOptions {
        db_path: db_path.ok_or("--db is required")?,
        trusted_extensions_dir: trusted_extensions_dir
            .ok_or("--trusted-extensions-dir is required")?,
        shell_port,
    })
}

fn bind_listener(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn options_are_explicit_and_management_is_impossible() {
        assert!(parse_options(&[]).is_err());
        assert!(parse_options(&strings(&[
            "--db",
            "core.db",
            "--trusted-extensions-dir",
            "extensions.d",
        ]))
        .is_ok());
        assert!(parse_options(&strings(&[
            "--db",
            "core.db",
            "--trusted-extensions-dir",
            "extensions.d",
            "--manage",
        ]))
        .is_err());
        assert!(parse_options(&strings(&[
            "--db",
            "core.db",
            "--trusted-extensions-dir",
            "extensions.d",
            "--bind",
            "0.0.0.0",
        ]))
        .is_err());
    }

    #[test]
    fn help_is_the_only_argument_free_success_path() {
        assert!(run(strings(&["--help"])).is_ok());
        assert!(run(Vec::new()).is_err());
    }
}
