use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub(super) const MAX_ASSET_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct OpenedAsset {
    pub(crate) content_type: &'static str,
    pub(crate) bytes: Vec<u8>,
}

pub(super) fn validate_relative_path(requested: &str) -> Result<PathBuf, String> {
    let decoded = percent_decode_once(requested)?;
    if decoded.is_empty()
        || decoded.contains('\0')
        || decoded.contains('\\')
        || decoded.contains('%')
        || decoded.contains('?')
        || decoded.contains('#')
        || decoded.starts_with('/')
        || decoded.ends_with('/')
        || decoded.split('/').any(str::is_empty)
    {
        return Err("extension asset must be a clean relative path".to_string());
    }
    let relative = Path::new(&decoded);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("extension asset contains a non-normal path component".to_string());
    }
    if relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .any(|component| component.chars().any(char::is_control))
    {
        return Err("extension asset contains a control character".to_string());
    }
    Ok(relative.to_path_buf())
}

pub(super) fn open_confined_file(root: &Path, requested: &str) -> Result<OpenedAsset, String> {
    let relative = validate_relative_path(requested)?;
    let candidate = root.join(&relative);
    let canonical = fs::canonicalize(&candidate)
        .map_err(|error| format!("extension asset cannot be opened: {error}"))?;
    ensure_within(root, &canonical)?;
    let content_type = asset_content_type(&relative)
        .ok_or_else(|| "extension asset has an unsupported media type".to_string())?;
    let file = File::open(&canonical)
        .map_err(|error| format!("extension asset cannot be opened: {error}"))?;
    let opened_metadata = file
        .metadata()
        .map_err(|error| format!("extension asset metadata failed: {error}"))?;
    if !opened_metadata.is_file() {
        return Err("extension asset is not a regular file".to_string());
    }
    let after_open = fs::canonicalize(&candidate)
        .map_err(|error| format!("extension asset changed during open: {error}"))?;
    ensure_within(root, &after_open)?;
    let after_metadata = fs::metadata(&after_open)
        .map_err(|error| format!("extension asset metadata failed: {error}"))?;
    if !same_file(&opened_metadata, &after_metadata) {
        return Err("extension asset changed during open".to_string());
    }

    let mut bytes = Vec::new();
    let max_plus_one = u64::try_from(MAX_ASSET_BYTES)
        .expect("asset limit fits in u64")
        .saturating_add(1);
    file.take(max_plus_one)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("extension asset read failed: {error}"))?;
    if bytes.len() > MAX_ASSET_BYTES {
        return Err("extension asset exceeds the size limit".to_string());
    }
    Ok(OpenedAsset {
        content_type,
        bytes,
    })
}

fn ensure_within(root: &Path, candidate: &Path) -> Result<(), String> {
    if candidate.starts_with(root) {
        Ok(())
    } else {
        Err("extension asset escapes its configured root".to_string())
    }
}

#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len() && left.modified().ok() == right.modified().ok()
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

fn percent_decode_once(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = hex(bytes[index + 1])
                    .ok_or_else(|| "invalid percent escape in asset path".to_string())?;
                let low = hex(bytes[index + 2])
                    .ok_or_else(|| "invalid percent escape in asset path".to_string())?;
                output.push((high << 4) | low);
                index += 3;
            }
            b'%' => return Err("truncated percent escape in asset path".to_string()),
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).map_err(|_| "asset path is not UTF-8".to_string())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mh-ui-ext-path-{name}-{stamp}"));
        fs::create_dir_all(&path).expect("mkdir");
        path
    }

    #[test]
    fn path_validation_rejects_plain_encoded_and_double_encoded_escape() {
        for path in [
            "../index.html",
            "%2e%2e/index.html",
            "%2E%2E/index.html",
            "%252e%252e/index.html",
            "/index.html",
            "nested/../../index.html",
            "nested\\index.html",
            "index.html%00",
            "index.html%2f..%2fsecret.html",
            "index.html/",
        ] {
            assert!(validate_relative_path(path).is_err(), "accepted {path:?}");
        }
        assert_eq!(
            validate_relative_path("nested/index.mjs").unwrap(),
            PathBuf::from("nested/index.mjs")
        );
    }

    #[test]
    fn mjs_has_a_javascript_media_type() {
        assert_eq!(
            asset_content_type(Path::new("index.mjs")),
            Some("text/javascript; charset=utf-8")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected_before_bytes_are_returned() {
        use std::os::unix::fs::symlink;
        let root = temp_dir("symlink");
        let outside = temp_dir("outside");
        fs::write(outside.join("secret.html"), b"secret").expect("write outside");
        symlink(outside.join("secret.html"), root.join("secret.html")).expect("symlink");
        assert!(open_confined_file(&root, "secret.html").is_err());
    }
}
