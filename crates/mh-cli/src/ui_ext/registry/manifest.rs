use std::collections::BTreeSet;

use serde_json::Value;

use super::path::validate_relative_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionManifest {
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) entry: String,
    pub(crate) description: Option<String>,
}

pub(super) fn parse_manifest(raw: &str) -> Result<ExtensionManifest, String> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|error| format!("invalid extension manifest JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "extension manifest must be a JSON object".to_string())?;
    let allowed = ["name", "title", "entry", "description"]
        .into_iter()
        .collect::<BTreeSet<_>>();
    if object.keys().any(|key| !allowed.contains(key.as_str())) {
        return Err("extension manifest contains an unknown field".to_string());
    }
    let name = required_string(object, "name")?;
    if !valid_name(&name) {
        return Err("extension manifest name does not match ^[a-z][a-z0-9-]{0,31}$".to_string());
    }
    let title = required_string(object, "title")?;
    validate_text(&title, 4096, "title")?;
    let entry = required_string(object, "entry")?;
    validate_relative_path(&entry)?;
    if entry.len() > 4096 {
        return Err("entry exceeds 4096 UTF-8 bytes".to_string());
    }
    let description = match object.get("description") {
        Some(value) => {
            let description = value
                .as_str()
                .ok_or_else(|| "description must be a string".to_string())?;
            validate_text(description, 4096, "description")?;
            Some(description.to_string())
        }
        None => None,
    };
    Ok(ExtensionManifest {
        name,
        title,
        entry,
        description,
    })
}

fn required_string(object: &serde_json::Map<String, Value>, field: &str) -> Result<String, String> {
    let value = object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field} must be a string"))?;
    if value.trim().is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    Ok(value.to_string())
}

fn validate_text(value: &str, max_bytes: usize, field: &str) -> Result<(), String> {
    if value.len() > max_bytes {
        return Err(format!("{field} exceeds {max_bytes} UTF-8 bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} contains a control character"));
    }
    Ok(())
}

pub(super) fn valid_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 32 || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_shape_is_exact_and_name_is_bounded() {
        let manifest = parse_manifest(
            r#"{"name":"alpha-1","title":"Alpha","entry":"index.mjs","description":"synthetic"}"#,
        )
        .unwrap();
        assert_eq!(manifest.name, "alpha-1");
        assert_eq!(manifest.description.as_deref(), Some("synthetic"));
        for name in [
            "",
            "Alpha",
            "1alpha",
            "alpha_1",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            let raw = format!(r#"{{"name":"{name}","title":"A","entry":"index.html"}}"#);
            assert!(parse_manifest(&raw).is_err(), "accepted name {name:?}");
        }
        assert!(parse_manifest(
            r#"{"name":"alpha","title":"A","entry":"index.html","command":"run"}"#,
        )
        .is_err());
    }

    #[test]
    fn manifest_rejects_path_escape_and_non_string_description() {
        assert!(
            parse_manifest(r#"{"name":"alpha","title":"A","entry":"%2e%2e/index.html"}"#,).is_err()
        );
        assert!(parse_manifest(
            r#"{"name":"alpha","title":"A","entry":"index.html","description":null}"#,
        )
        .is_err());
    }
}
