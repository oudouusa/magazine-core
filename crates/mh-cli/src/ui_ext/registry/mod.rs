mod manifest;
mod path;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use self::manifest::{parse_manifest, ExtensionManifest};
use self::path::{asset_content_type, open_confined_file};

pub(super) use self::path::OpenedAsset as RegistryOpenedAsset;

const MAX_EXTENSIONS: usize = 32;
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;

#[derive(Debug)]
pub(crate) struct ExtensionRegistry {
    extensions: BTreeMap<String, RegisteredExtension>,
}

#[derive(Debug, Clone)]
pub(crate) struct RegisteredExtension {
    pub(crate) manifest: ExtensionManifest,
    pub(crate) root: PathBuf,
}

impl ExtensionRegistry {
    pub(super) fn load(root: impl AsRef<Path>) -> Result<Self, String> {
        let configured_root = root.as_ref();
        let configured_metadata = fs::symlink_metadata(configured_root)
            .map_err(|error| format!("trusted extension root metadata failed: {error}"))?;
        if configured_metadata.file_type().is_symlink() || !configured_metadata.is_dir() {
            return Err("trusted extension root must be a real directory".to_string());
        }
        let root = fs::canonicalize(configured_root)
            .map_err(|error| format!("trusted extension root cannot be opened: {error}"))?;
        let metadata = fs::symlink_metadata(&root)
            .map_err(|error| format!("trusted extension root metadata failed: {error}"))?;
        if !metadata.is_dir() {
            return Err("trusted extension root is not a directory".to_string());
        }

        let mut entries = fs::read_dir(&root)
            .map_err(|error| format!("trusted extension root cannot be read: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("trusted extension root cannot be read: {error}"))?;
        entries.sort_by_key(|entry| entry.file_name());
        let mut extensions = BTreeMap::new();
        for entry in entries {
            let file_type = entry
                .file_type()
                .map_err(|error| format!("extension entry metadata failed: {error}"))?;
            if file_type.is_symlink() {
                return Err(format!(
                    "trusted extension root contains a symlink: {}",
                    entry.path().display()
                ));
            }
            if !file_type.is_dir() {
                // The root is a directory of extension directories. Regular
                // files such as README are not extension registrations.
                continue;
            }
            if extensions.len() >= MAX_EXTENSIONS {
                return Err(format!(
                    "trusted extension root exceeds the {MAX_EXTENSIONS} extension limit"
                ));
            }
            let directory_name = entry
                .file_name()
                .to_str()
                .ok_or_else(|| "extension directory name is not UTF-8".to_string())?
                .to_string();
            let extension_root = fs::canonicalize(entry.path())
                .map_err(|error| format!("extension directory cannot be opened: {error}"))?;
            if !extension_root.starts_with(&root) {
                return Err("extension directory escapes its configured root".to_string());
            }
            let manifest_path = extension_root.join("plugin.json");
            let manifest_metadata = fs::symlink_metadata(&manifest_path)
                .map_err(|error| format!("extension manifest cannot be opened: {error}"))?;
            if manifest_metadata.file_type().is_symlink() || !manifest_metadata.is_file() {
                return Err(format!(
                    "extension {directory_name} has no regular plugin.json"
                ));
            }
            if manifest_metadata.len() > MAX_MANIFEST_BYTES {
                return Err(format!(
                    "extension {directory_name} plugin.json exceeds 16 KiB"
                ));
            }
            let raw = fs::read_to_string(&manifest_path)
                .map_err(|error| format!("extension manifest cannot be read: {error}"))?;
            let manifest = parse_manifest(&raw)?;
            if manifest.name != directory_name {
                return Err(format!(
                    "extension manifest name {:?} does not match directory {:?}",
                    manifest.name, directory_name
                ));
            }
            if extensions
                .insert(
                    manifest.name.clone(),
                    RegisteredExtension {
                        manifest: manifest.clone(),
                        root: extension_root.clone(),
                    },
                )
                .is_some()
            {
                return Err(format!("duplicate extension name: {}", manifest.name));
            }
            let _entry = open_confined_file(&extension_root, &manifest.entry)?;
            if asset_content_type(Path::new(&manifest.entry)).is_none() {
                return Err(format!(
                    "extension entry has unsupported media type: {}",
                    manifest.entry
                ));
            }
        }
        Ok(Self { extensions })
    }

    pub(super) fn all(&self) -> impl Iterator<Item = &RegisteredExtension> {
        self.extensions.values()
    }

    pub(super) fn get(&self, name: &str) -> Option<&RegisteredExtension> {
        self.extensions.get(name)
    }

    pub(super) fn asset(&self, name: &str, requested: &str) -> Result<RegistryOpenedAsset, String> {
        let extension = self
            .get(name)
            .ok_or_else(|| "unknown extension".to_string())?;
        open_confined_file(&extension.root, requested)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mh-ui-ext-registry-{name}-{stamp}"));
        fs::create_dir_all(&path).expect("mkdir");
        path
    }

    fn write_extension(root: &Path, name: &str, entry: &str, extra: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).expect("extension mkdir");
        fs::write(
            dir.join("plugin.json"),
            format!(r#"{{"name":"{name}","title":"{name} title","entry":"{entry}"{extra}}}"#),
        )
        .expect("manifest");
        fs::write(dir.join(entry), b"<script type=module></script>").expect("entry");
    }

    #[test]
    fn registry_is_deterministic_and_manifest_is_directory_bound() {
        let root = temp_dir("order");
        write_extension(&root, "zeta", "index.mjs", "");
        write_extension(&root, "alpha", "index.html", "");
        let registry = ExtensionRegistry::load(&root).expect("registry");
        let names = registry
            .all()
            .map(|extension| extension.manifest.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["alpha", "zeta"]);
        assert!(registry.get("alpha").unwrap().root.is_absolute());
    }

    #[test]
    fn unknown_manifest_fields_and_name_mismatch_fail_closed() {
        let root = temp_dir("invalid");
        write_extension(&root, "alpha", "index.html", ",\"command\":\"run\"");
        assert!(ExtensionRegistry::load(&root).is_err());

        let root = temp_dir("mismatch");
        write_extension(&root, "alpha", "index.html", "");
        fs::write(
            root.join("alpha/plugin.json"),
            r#"{"name":"beta","title":"Beta","entry":"index.html"}"#,
        )
        .expect("rewrite");
        assert!(ExtensionRegistry::load(&root).is_err());
    }

    #[test]
    fn missing_manifest_and_symlink_directory_fail_closed() {
        let root = temp_dir("missing");
        fs::create_dir(root.join("alpha")).expect("mkdir");
        assert!(ExtensionRegistry::load(&root).is_err());

        let root = temp_dir("symlink");
        write_extension(&root, "alpha", "index.html", "");
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(root.join("alpha"), root.join("beta")).expect("symlink");
            assert!(ExtensionRegistry::load(&root).is_err());
        }
    }
}
