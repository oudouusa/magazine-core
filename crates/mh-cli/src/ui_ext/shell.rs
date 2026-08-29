use std::fmt::Write as _;

use serde_json::{json, Value};

use super::broker::{BROKER_CHANNEL, MAX_IFRAME_CONCURRENT_REQUESTS};
use super::registry::ExtensionRegistry;

const READ_ROUTE_PREFIX: &str = "/__mh_ui_ext/read/";

#[derive(Debug)]
pub(super) struct ShellPage {
    pub(super) bytes: Vec<u8>,
    pub(super) csp: String,
}

pub(super) fn read_route_prefix() -> &'static str {
    READ_ROUTE_PREFIX
}

pub(super) fn render_shell(
    registry: &ExtensionRegistry,
    shell_origin: &str,
    asset_origin: &str,
) -> Result<ShellPage, String> {
    let nonce = make_nonce()?;
    let registrations = registry
        .all()
        .map(|extension| {
            json!({
                "name": extension.manifest.name,
                "title": extension.manifest.title,
                "description": extension.manifest.description,
                "src": format!(
                    "{}/extensions/{}/{}",
                    asset_origin,
                    extension.manifest.name,
                    extension.manifest.entry
                ),
            })
        })
        .collect::<Vec<_>>();
    let registrations_json = script_json(
        &serde_json::to_value(registrations)
            .map_err(|error| format!("extension registration JSON failed: {error}"))?,
    );
    let script = broker_script(&registrations_json);

    let mut frames = String::new();
    for registration in registry.all() {
        let name = html_escape(&registration.manifest.name);
        let title = html_escape(&registration.manifest.title);
        let src = format!(
            "{}/extensions/{}/{}",
            asset_origin, registration.manifest.name, registration.manifest.entry
        );
        let src = html_escape(&src);
        let description = registration
            .manifest
            .description
            .as_deref()
            .map(html_escape)
            .unwrap_or_default();
        let _ = write!(
            frames,
            "<article class=\"mh-ui-ext-card\"><h2>{title}</h2>{description}<iframe id=\"mh-ui-ext-{name}\" title=\"{title}\" src=\"{src}\" sandbox=\"allow-scripts\"></iframe></article>",
            description = if description.is_empty() {
                String::new()
            } else {
                format!("<p>{description}</p>")
            }
        );
    }
    let asset_origin = html_escape(asset_origin);
    let shell_origin = html_escape(shell_origin);
    let csp = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; connect-src 'self'; frame-src {asset_origin}; img-src 'none'; font-src 'none'; media-src 'none'; object-src 'none'; worker-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    );
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\"><meta name=\"mh-ui-ext-shell\" content=\"{shell_origin}\"><title>mh-ui-ext</title><style nonce=\"{nonce}\">body{{font-family:system-ui,sans-serif;margin:2rem;background:#f7f7f7;color:#222}}main{{max-width:72rem;margin:auto}}.mh-ui-ext-grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(20rem,1fr));gap:1rem}}.mh-ui-ext-card{{background:white;border:1px solid #ccc;border-radius:.5rem;padding:1rem}}iframe{{display:block;width:100%;min-height:16rem;border:1px solid #ddd;border-radius:.25rem}}</style></head><body><main><h1>mh-ui-ext</h1><p>Read-only trusted UI extensions. Extension assets are local executable code.</p><section class=\"mh-ui-ext-grid\">{frames}</section></main><script nonce=\"{nonce}\">{script}</script></body></html>"
    );
    Ok(ShellPage {
        bytes: html.into_bytes(),
        csp,
    })
}

fn broker_script(registrations_json: &str) -> String {
    // Keep this script self-contained. It is intentionally a broker, not an
    // extension API: iframe identity comes from the registered DOM window.
    format!(
        r#"(function () {{
  'use strict';
  const CHANNEL = {channel};
  const MAX_REQUEST_BYTES = 16384;
  const MAX_RESPONSE_BYTES = 8388608;
  const MAX_CONCURRENT = {max_concurrent};
  const READ_PREFIX = {read_prefix};
  const registrations = {registrations};
  const pending = new Map();
  const counts = new WeakMap();

  function jsonByteLength(value) {{
    try {{ return new TextEncoder().encode(JSON.stringify(value)).length; }}
    catch (_error) {{ return Infinity; }}
  }}
  function textByteLength(value) {{
    try {{ return new TextEncoder().encode(value).length; }}
    catch (_error) {{ return Infinity; }}
  }}
  function isPlainObject(value) {{
    if (value === null || typeof value !== 'object') return false;
    const prototype = Object.getPrototypeOf(value);
    return prototype === Object.prototype || prototype === null;
  }}
  function hasExactKeys(value, keys) {{
    const actual = Object.keys(value).sort();
    const expected = keys.slice().sort();
    return actual.length === expected.length &&
      actual.every((key, index) => key === expected[index]);
  }}
  function textOkay(value, limit) {{
    return typeof value === 'string' && value.trim() !== '' &&
      textByteLength(value) <= limit &&
      !/[\\u0000-\\u001f\\u007f]/.test(value);
  }}
  function requestIdOkay(value) {{
    return typeof value === 'string' && /^[A-Za-z0-9_-]{{1,64}}$/.test(value);
  }}
  function payloadOkay(op, payload) {{
    if (!isPlainObject(payload)) return false;
    if (op === 'gallery.list') {{
      if (!hasExactKeys(payload, ['limit', 'offset'])) return false;
      return Number.isSafeInteger(payload.limit) && payload.limit >= 1 &&
        payload.limit <= 200 && Number.isSafeInteger(payload.offset) &&
        payload.offset >= 0;
    }}
    if (op === 'graph.detail') {{
      return hasExactKeys(payload, ['work_key']) &&
        textOkay(payload.work_key, 4096);
    }}
    return false;
  }}
  function registrationFor(source) {{
    for (const registration of registrations) {{
      if (source === registration.frame.contentWindow) return registration;
    }}
    return null;
  }}
  function sendResult(registration, request, result) {{
    let message = {{
      channel: CHANNEL,
      type: 'read-result',
      request_id: request.request_id,
      op: request.op,
      generation: result && typeof result.generation === 'string'
        ? result.generation : null,
      ok: result && result.ok === true,
      payload: result && result.ok === true ? result.payload : null
    }};
    if (result && result.ok !== true) {{
      message.error = result.error && typeof result.error.code === 'string'
        ? {{code: result.error.code}} : {{code: 'read_failed'}};
    }}
    if (jsonByteLength(message) > MAX_RESPONSE_BYTES) {{
      message = {{
        channel: CHANNEL, type: 'read-result',
        request_id: request.request_id, op: request.op,
        generation: null, ok: false, payload: null,
        error: 'response exceeds 16 KiB'
      }};
    }}
    registration.frame.contentWindow.postMessage(message, '*');
  }}
  function finish(registration, request, result) {{
    pending.delete(request.request_id);
    counts.set(registration.frame, Math.max(0, (counts.get(registration.frame) || 1) - 1));
    sendResult(registration, request, result);
  }}
  window.addEventListener('message', function (event) {{
    const registration = registrationFor(event.source);
    if (!registration) return;
    const message = event.data;
    if (!isPlainObject(message) ||
        jsonByteLength(message) > MAX_REQUEST_BYTES ||
        !hasExactKeys(message, ['channel', 'type', 'request_id', 'op', 'payload']) ||
        message.channel !== CHANNEL || message.type !== 'read' ||
        !requestIdOkay(message.request_id) ||
        (message.op !== 'gallery.list' && message.op !== 'graph.detail') ||
        !payloadOkay(message.op, message.payload) ||
        pending.has(message.request_id)) return;
    const count = counts.get(registration.frame) || 0;
    if (count >= MAX_CONCURRENT) {{
      sendResult(registration, message, {{ok: false}});
      return;
    }}
    counts.set(registration.frame, count + 1);
    pending.set(message.request_id, {{registration: registration, request: message}});
    const query = new URLSearchParams();
    query.set('request_id', message.request_id);
    query.set('op', message.op);
    query.set('payload', JSON.stringify(message.payload));
    fetch(READ_PREFIX + encodeURIComponent(registration.name) + '?' + query.toString(),
      {{method: 'GET', cache: 'no-store', credentials: 'same-origin'}})
      .then(function (response) {{
        if (!response.ok) throw new Error('read failed');
        return response.json();
      }})
      .then(function (result) {{
        const current = pending.get(message.request_id);
        if (!current || current.registration !== registration ||
            !result || result.request_id !== message.request_id ||
            result.op !== message.op || typeof result.ok !== 'boolean') {{
          throw new Error('broker response mismatch');
        }}
        finish(registration, message, result);
      }})
      .catch(function () {{
        if (pending.has(message.request_id)) finish(registration, message, {{ok: false}});
      }});
  }});
  for (const registration of registrations) {{
    registration.frame = document.getElementById('mh-ui-ext-' + registration.name);
    counts.set(registration.frame, 0);
  }}
}})();"#,
        channel = script_json(&Value::String(BROKER_CHANNEL.to_string())),
        read_prefix = script_json(&Value::String(READ_ROUTE_PREFIX.to_string())),
        registrations = registrations_json,
        max_concurrent = MAX_IFRAME_CONCURRENT_REQUESTS,
    )
}

fn script_json(value: &Value) -> String {
    serde_json::to_string(value)
        .expect("JSON values are serializable")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&#39;")
}

fn make_nonce() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| format!("shell CSP nonce generation failed: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_ext::registry::ExtensionRegistry;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mh-ui-ext-shell-{stamp}"));
        fs::create_dir_all(path.join("alpha")).expect("mkdir");
        fs::write(
            path.join("alpha/plugin.json"),
            r#"{"name":"alpha","title":"Alpha","entry":"index.mjs"}"#,
        )
        .expect("manifest");
        fs::write(path.join("alpha/index.mjs"), b"export default 1;").expect("asset");
        path
    }

    #[test]
    fn shell_uses_sandboxed_iframe_and_separate_asset_origin() {
        let root = temp_dir();
        let registry = ExtensionRegistry::load(&root).unwrap();
        let page = render_shell(
            &registry,
            "http://127.0.0.1:10001",
            "http://127.0.0.1:10002",
        )
        .unwrap();
        let html = String::from_utf8(page.bytes).unwrap();
        assert!(html.contains("sandbox=\"allow-scripts\""));
        assert!(!html.contains("allow-same-origin"));
        assert!(html.contains("http://127.0.0.1:10002/extensions/alpha/index.mjs"));
        assert!(page.csp.contains("connect-src 'self'"));
        assert!(html.contains(BROKER_CHANNEL));
        assert!(html.contains("MAX_CONCURRENT = 8"));
    }
}
