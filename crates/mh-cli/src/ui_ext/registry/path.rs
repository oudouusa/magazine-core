use std::error::Error;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(super) fn resolve_existing_file(
    root: &Path,
    requested: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let decoded = percent_decode(requested)?;
    if decoded.contains('\0')
        || decoded.contains('\\')
        || decoded.starts_with('/')
        || decoded.ends_with('/')
        || decoded.split('/').any(str::is_empty)
    {
        return Err("extension asset must be a clean relative path".into());
    }
    let relative = Path::new(&decoded);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("extension asset contains a non-normal path component".into());
    }
    let candidate = fs::canonicalize(root.join(relative))
        .map_err(|error| format!("extension asset cannot be opened: {error}"))?;
    if !candidate.starts_with(root) || !candidate.is_file() {
        return Err("extension asset escapes its configured root".into());
    }
    Ok(candidate)
}

pub(super) fn asset_content_type(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "html" => Some("text/html; charset=utf-8"),
        "js" | "mjs" => Some("text/javascript; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "json" => Some("application/json; charset=utf-8"),
        "svg" => Some("image/svg+xml"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        _ => None,
    }
}

fn percent_decode(value: &str) -> Result<String, Box<dyn Error>> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = hex(bytes[index + 1]).ok_or("invalid percent escape in asset path")?;
                let low = hex(bytes[index + 2]).ok_or("invalid percent escape in asset path")?;
                output.push((high << 4) | low);
                index += 3;
            }
            b'%' => return Err("truncated percent escape in asset path".into()),
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).map_err(|_| "asset path is not UTF-8".into())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
