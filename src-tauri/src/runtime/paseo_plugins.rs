//! The app-owned Paseo plugin bundle that `prepare_agent_web()` delivers into each sandbox
//! (`docs/features/paseo-plugins/02_dev.md`).

use std::sync::Arc;

use sha2::{Digest, Sha256};

use super::types::RuntimeError;

/// The packed `paseo-plugins/` tree with its content hash and the plugin ids it holds.
pub(crate) struct PluginBundle {
    pub archive: Vec<u8>,
    /// Hex SHA-256 of `archive`; the in-sandbox marker records it after delivery.
    pub hash: String,
    /// Sorted plugin ids, one per top-level directory of `archive`.
    pub ids: Vec<String>,
}

impl PluginBundle {
    fn new(archive: Vec<u8>, ids: Vec<String>) -> Self {
        let hash = Sha256::digest(&archive).iter().map(|b| format!("{b:02x}")).collect();
        PluginBundle { archive, hash, ids }
    }
}

/// Debug builds re-read the worktree's `paseo-plugins/` on every call, so a plugin edit needs no
/// Rust rebuild (`docs/features/paseo-plugins/02_dev.md`, The Concrete Model).
#[cfg(debug_assertions)]
pub(crate) fn plugin_bundle() -> Result<Arc<PluginBundle>, RuntimeError> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../paseo-plugins");
    let (archive, ids) = super::plugin_archive::build_plugin_archive(std::path::Path::new(dir))
        .map_err(|e| RuntimeError::agent_web_unavailable(format!("packing {dir}: {e}")))?;
    Ok(Arc::new(PluginBundle::new(archive, ids)))
}

/// Release builds use the archive `src-tauri/build.rs` packed at compile time.
#[cfg(not(debug_assertions))]
pub(crate) fn plugin_bundle() -> Result<Arc<PluginBundle>, RuntimeError> {
    static BUNDLE: std::sync::OnceLock<Arc<PluginBundle>> = std::sync::OnceLock::new();
    Ok(BUNDLE
        .get_or_init(|| {
            let archive = include_bytes!(concat!(env!("OUT_DIR"), "/paseo-plugins.tar")).to_vec();
            let ids = include_str!(concat!(env!("OUT_DIR"), "/paseo-plugin-ids.txt"))
                .lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            Arc::new(PluginBundle::new(archive, ids))
        })
        .clone())
}

#[cfg(test)]
mod tests {
    use super::super::plugin_archive::build_plugin_archive;
    use super::*;
    use std::path::Path;

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> TempDir {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = TempDir(std::env::temp_dir().join(format!("mio-plugin-archive-{}-{n}", std::process::id())));
        let root = dir.path();
        write(root, "mio-b/paseo-plugin.json", "{}");
        write(root, "mio-b/index.server.ts", "export default () => () => {};");
        write(root, "mio-b/shared/x.ts", "export const x = 1;");
        write(root, "mio-b/node_modules/dep/index.js", "x");
        write(root, "mio-b/.cache", "x");
        write(root, "mio-a/paseo-plugin.json", "{}");
        write(root, "mio-nomanifest/index.server.ts", "x");
        write(root, "other/paseo-plugin.json", "{}");
        write(root, "package.json", "{}");
        dir
    }

    fn entries(archive: &[u8]) -> Vec<(String, u64, u64, u64)> {
        tar::Archive::new(archive)
            .entries()
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                let h = e.header();
                (
                    e.path().unwrap().to_string_lossy().into_owned(),
                    h.uid().unwrap(),
                    h.gid().unwrap(),
                    h.mtime().unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn archive_excludes_node_modules_and_dotfiles_and_zeroes_owner_and_mtime() {
        let dir = fixture();
        let (archive, _) = build_plugin_archive(dir.path()).unwrap();
        let entries = entries(&archive);
        let paths: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(
            paths,
            [
                "mio-a/",
                "mio-a/paseo-plugin.json",
                "mio-b/",
                "mio-b/index.server.ts",
                "mio-b/paseo-plugin.json",
                "mio-b/shared/",
                "mio-b/shared/x.ts",
            ]
        );
        assert!(entries.iter().all(|e| e.1 == 0 && e.2 == 0 && e.3 == 0));
    }

    /// `docs/features/paseo-plugins/02_dev.md`, Resolved Decision 4.
    #[test]
    fn plugin_sdk_and_manifests_match_the_pinned_paseo_cli() {
        let (_, cli_version) = super::super::split_tool_spec(super::super::PASEO_CLI_TOOL);
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../paseo-plugins"));
        let package: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("package.json")).unwrap()).unwrap();
        assert_eq!(
            package["devDependencies"]["@getpaseo/plugin"].as_str(),
            Some(cli_version),
            "paseo-plugins/package.json @getpaseo/plugin must equal PASEO_CLI_TOOL"
        );
        let (_, ids) = build_plugin_archive(root).unwrap();
        assert!(!ids.is_empty());
        for id in ids {
            let manifest: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(root.join(&id).join("paseo-plugin.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest["id"].as_str(), Some(id.as_str()), "{id}: manifest id must equal its directory");
            assert_eq!(
                manifest["requirements"]["paseo"].as_str(),
                Some(format!(">={cli_version}").as_str()),
                "{id}: requirements.paseo must be >= PASEO_CLI_TOOL's version"
            );
        }
    }
}
