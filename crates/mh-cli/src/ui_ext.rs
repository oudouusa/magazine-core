//! Explicitly opt-in, read-only host for trusted UI extensions.
//!
//! This binary is deliberately separate from `mh ui`.  It serves a small shell
//! and namespaced extension assets on two loopback origins, and brokers exactly
//! two read operations to an operator-started Unix socket provider.  It does
//! not execute a manifest command or share the management UI's state.

mod broker;
mod http;
mod provider;
mod registry;
mod shell;

use std::error::Error;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use mh_db::Database;

use self::provider::ProviderClient;
use self::registry::ExtensionRegistry;

const DEFAULT_SHELL_PORT: u16 = 8766;

#[derive(Debug, Clone)]
pub(super) struct Options {
    pub(super) shell_port: u16,
    pub(super) asset_port: u16,
    pub(super) registry: Arc<ExtensionRegistry>,
    pub(super) provider: Arc<Mutex<ProviderClient>>,
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
    provider_socket: PathBuf,
    shell_port: u16,
}

pub(crate) fn usage() -> &'static str {
    "Usage:\n  mh-ui-ext --db <core.db> --trusted-extensions-dir <extensions.d> --provider-socket <absolute-path> [--port N]\n\nThe host is read-only and loopback-only. Trusted extension JavaScript is local executable code and may transmit any data delivered to it; sandboxing, CSP, and Permissions-Policy are defense in depth, not a network sandbox. The provider is never started by this binary."
}

pub(crate) fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    if matches!(args.as_slice(), [flag] if flag == "-h" || flag == "--help") {
        println!("{}", usage());
        return Ok(());
    }

    let parsed = parse_options(&args)?;
    // This is intentionally read-only and also rejects an uninitialised or
    // non-canonical database.  No handle is retained after the check.
    Database::open_read_only(&parsed.db_path)?.inspect()?;
    let registry = Arc::new(ExtensionRegistry::load(&parsed.trusted_extensions_dir)?);
    let provider = Arc::new(Mutex::new(ProviderClient::connect(
        &parsed.provider_socket,
    )?));

    let asset_listener = bind_listener(0)?;
    let asset_port = asset_listener.local_addr()?.port();
    let shell_listener = bind_listener(parsed.shell_port)?;
    let shell_port = shell_listener.local_addr()?.port();
    let options = Arc::new(Options {
        shell_port,
        asset_port,
        registry,
        provider,
    });

    println!("mh-ui-ext listening on {}", options.shell_origin());
    eprintln!("mh-ui-ext extension assets on {}", options.asset_origin());
    eprintln!(
        "warning: trusted UI extensions are local executable code and may read and transmit data delivered to them; sandbox/CSP/Permissions-Policy are defense-in-depth only and do not provide a network sandbox"
    );

    let asset_options = Arc::clone(&options);
    thread::Builder::new()
        .name("mh-ui-ext-assets".to_string())
        .spawn(move || http::serve(asset_listener, asset_options, http::ServerRole::Assets))?;
    http::serve(shell_listener, options, http::ServerRole::Shell);
    Ok(())
}

fn parse_options(args: &[String]) -> Result<ParsedOptions, Box<dyn Error>> {
    let mut db_path = None;
    let mut trusted_extensions_dir = None;
    let mut provider_socket = None;
    let mut shell_port = DEFAULT_SHELL_PORT;
    let mut port_seen = false;
    let mut index = 0;

    while index < args.len() {
        let flag = args[index].as_str();
        if matches!(
            flag,
            "--host"
                | "--bind"
                | "--manage"
                | "--token"
                | "--route"
                | "--write"
                | "--init"
                | "--discover"
                | "--cancel"
        ) {
            return Err(format!("{flag} is not supported by the read-only extension host").into());
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
            "--provider-socket" => {
                if provider_socket.replace(PathBuf::from(value)).is_some() {
                    return Err("--provider-socket specified more than once".into());
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

    let provider_socket = provider_socket.ok_or("--provider-socket is required")?;
    if !provider_socket.is_absolute() {
        return Err("--provider-socket must be an absolute path".into());
    }
    Ok(ParsedOptions {
        db_path: db_path.ok_or("--db is required")?,
        trusted_extensions_dir: trusted_extensions_dir
            .ok_or("--trusted-extensions-dir is required")?,
        provider_socket,
        shell_port,
    })
}

fn bind_listener(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

pub(super) fn is_loopback_authority(value: &str, port: u16) -> bool {
    let value = value.trim();
    let (host, port_text) = match value.rsplit_once(':') {
        Some((host, port_text)) => (host, port_text),
        None => (value, "80"),
    };
    host == "127.0.0.1" && port_text.parse::<u16>().ok() == Some(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn options_require_provider_and_reject_management_or_bind_flags() {
        assert!(parse_options(&[]).is_err());
        let base = [
            "--db",
            "core.db",
            "--trusted-extensions-dir",
            "extensions.d",
            "--provider-socket",
            "/run/user/1000/provider.sock",
        ];
        assert!(parse_options(&strings(&base)).is_ok());
        for flag in [
            "--host",
            "--bind",
            "--manage",
            "--token",
            "--route",
            "--write",
            "--init",
            "--discover",
            "--cancel",
        ] {
            let mut args = base
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>();
            args.push(flag.to_string());
            args.push("value".to_string());
            assert!(parse_options(&args).is_err(), "{flag} was accepted");
        }
        let relative = strings(&[
            "--db",
            "core.db",
            "--trusted-extensions-dir",
            "extensions.d",
            "--provider-socket",
            "provider.sock",
        ]);
        assert!(parse_options(&relative).is_err());
    }

    #[test]
    fn only_help_is_successful_without_required_arguments() {
        assert!(run(strings(&["--help"])).is_ok());
        assert!(run(Vec::new()).is_err());
    }

    #[test]
    fn host_guard_accepts_only_exact_ipv4_loopback_authority() {
        assert!(is_loopback_authority("127.0.0.1:8766", 8766));
        assert!(!is_loopback_authority("localhost:8766", 8766));
        assert!(!is_loopback_authority("127.0.0.2:8766", 8766));
        assert!(!is_loopback_authority("127.0.0.1:8767", 8766));
    }
}
