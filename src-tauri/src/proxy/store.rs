//! The proxy state-directory layout under the app data root: per sandbox `meta.json`, the secret-value
//! store, the CA, the flow log and the remote sidecar's staging. Holding the root instead of an
//! `AppHandle` keeps path derivation testable without a running app.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Runtime};

use super::io_err;
use super::types::{ProxyEngine, ProxyError};
use mio_domain::types::ProviderId;

/// Owns `<app_data_root>/proxy/`, which holds one directory per sandbox. `MIO_APP_DATA_DIR`
/// relocates the whole tree per worktree via `crate::paths::app_data_root`, mandatory here because
/// the directory holds the sandbox's CA private key.
#[derive(Debug, Clone)]
pub struct ProxyStore {
    /// The resolution error's text when the app data root is unavailable.
    root: Result<PathBuf, String>,
}

impl ProxyStore {
    #[cfg(test)]
    fn new(root: PathBuf) -> Self {
        Self { root: Ok(root) }
    }

    /// The store under the app data root. An unresolvable root fails each call that needs it, never
    /// startup, so the app still opens without proxy state.
    pub fn for_app<R: Runtime>(app: &AppHandle<R>) -> Self {
        Self { root: crate::paths::app_data_root(app).map(|root| root.join("proxy")).map_err(|e| e.to_string()) }
    }

    fn root(&self) -> Result<&Path, ProxyError> {
        self.root.as_deref().map_err(|e| ProxyError::CommandFailed(e.clone()))
    }

    fn sandbox_root(&self, sandbox: &str) -> Result<PathBuf, ProxyError> {
        Ok(self.root()?.join(sandbox))
    }

    /// `<root>/<sandbox>/`, created on demand.
    pub(super) fn state_dir(&self, sandbox: &str) -> Result<PathBuf, ProxyError> {
        let dir = self.sandbox_root(sandbox)?;
        std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
        Ok(dir)
    }

    pub(super) fn certs_dir(&self, sandbox: &str) -> Result<PathBuf, ProxyError> {
        let dir = self.state_dir(sandbox)?.join("certs");
        std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
        Ok(dir)
    }

    /// The CA the sandbox must trust: the app-generated `ca.pem` an iron-proxy sidecar mounts.
    fn ca_pem_path(&self, sandbox: &str, provider_id: ProviderId) -> Result<PathBuf, ProxyError> {
        let file = match provider_id {
            ProviderId::Podman => "ca.pem",
        };
        Ok(self.certs_dir(sandbox)?.join(file))
    }

    pub fn read_ca_pem(&self, sandbox: &str, provider_id: ProviderId) -> Result<Vec<u8>, ProxyError> {
        let path = self.ca_pem_path(sandbox, provider_id)?;
        std::fs::read(&path).map_err(|e| io_err(&path, e))
    }

    /// The NDJSON flow log the sidecar appends to and `flowlog::watch_flows` polls, a single-file
    /// mount at the state root.
    pub fn flows_path(&self, sandbox: &str) -> Result<PathBuf, ProxyError> {
        let dir = self.state_dir(sandbox)?;
        Ok(match self.sidecar_provider_id(sandbox) {
            ProviderId::Podman => dir.join("flows.ndjson"),
        })
    }

    /// The provider `sandbox`'s sidecar runs on, from `meta.json`'s `provider` tag; Podman when the
    /// file or tag is missing, which every sidecar created before the tag was written lacks.
    pub fn sidecar_provider_id(&self, sandbox: &str) -> ProviderId {
        self.read_meta(sandbox)
            .and_then(|meta| meta["provider"].as_str().and_then(|tag| tag.parse().ok()))
            .unwrap_or_default()
    }

    /// The proxy's own network name and pinned IP, cached beside its state so `status` can report them
    /// without a `podman network inspect` round trip — the container's presence, not this file, remains
    /// the source of truth for whether the sidecar exists (Resolved Decision 3).
    fn meta_path(&self, sandbox: &str) -> Result<PathBuf, ProxyError> {
        Ok(self.state_dir(sandbox)?.join("meta.json"))
    }

    /// `meta.json`'s path when it exists, without creating the state directory as a side effect.
    fn meta_path_if_exists(&self, sandbox: &str) -> Option<PathBuf> {
        let path = self.sandbox_root(sandbox).ok()?.join("meta.json");
        path.is_file().then_some(path)
    }

    pub(super) fn write_meta(&self, sandbox: &str, body: &serde_json::Value) -> Result<(), ProxyError> {
        let path = self.meta_path(sandbox)?;
        mio_files::write_compact(&path, body).map_err(|e| ProxyError::CommandFailed(e.to_string()))
    }

    pub(super) fn read_meta(&self, sandbox: &str) -> Option<serde_json::Value> {
        mio_files::read_value(&self.meta_path_if_exists(sandbox)?)
    }

    pub fn sidecar_engine(&self, sandbox: &str) -> ProxyEngine {
        self.read_meta(sandbox).map_or(ProxyEngine::IronProxy, |m| meta_engine(&m))
    }

    /// A sandbox's secret-value store, deleted with its state directory at destroy. Written only by
    /// `apply::apply_policy`, never read back over IPC
    /// (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 8).
    fn values_path(&self, sandbox: &str) -> Result<PathBuf, ProxyError> {
        Ok(self.state_dir(sandbox)?.join("values.json"))
    }

    /// `sandbox`'s stored values; a sandbox with none stored resolves nothing. A read creates no directory.
    pub fn load_secrets(&self, sandbox: &str) -> Result<HashMap<String, String>, ProxyError> {
        let path = self.sandbox_root(sandbox)?.join("values.json");
        match mio_files::read_value(&path) {
            Some(v) => serde_json::from_value(v).map_err(|e| {
                ProxyError::CommandFailed(format!("failed to parse {}: {e}", path.to_string_lossy()))
            }),
            None => Ok(HashMap::new()),
        }
    }

    /// Overwrites the whole value store; `apply_policy` uses it to restore a failed apply.
    pub(super) fn save_secrets(
        &self,
        sandbox: &str,
        secrets: &HashMap<String, String>,
    ) -> Result<(), ProxyError> {
        let path = self.values_path(sandbox)?;
        mio_files::write_pretty(&path, secrets).map_err(|e| ProxyError::CommandFailed(e.to_string()))
    }

    /// Set (or overwrite) the value stored for `secret_id` in `sandbox`. Never logged or returned.
    pub fn set_secret(&self, sandbox: &str, secret_id: &str, value: &str) -> Result<(), ProxyError> {
        let mut secrets = self.load_secrets(sandbox)?;
        secrets.insert(secret_id.to_string(), value.to_string());
        self.save_secrets(sandbox, &secrets)
    }

    pub fn delete_secret(&self, sandbox: &str, secret_id: &str) -> Result<(), ProxyError> {
        let mut secrets = self.load_secrets(sandbox)?;
        secrets.remove(secret_id);
        self.save_secrets(sandbox, &secrets)
    }

    /// The app-global value file older versions wrote, before values were kept per sandbox.
    pub(super) fn legacy_secrets_path(&self) -> Result<PathBuf, ProxyError> {
        Ok(self.root()?.join("secrets.json"))
    }

    /// `<state>/remote/`: the laptop's staging of the remote sidecar's files plus its connection cache
    /// and audit cursor. Created on demand.
    pub(super) fn remote_dir(&self, sandbox: &str) -> Result<PathBuf, ProxyError> {
        let dir = self.state_dir(sandbox)?.join("remote");
        std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
        Ok(dir)
    }

    /// The remote directory when it exists, without creating the state tree as a side effect.
    fn remote_dir_if_exists(&self, sandbox: &str) -> Option<PathBuf> {
        let dir = self.sandbox_root(sandbox).ok()?.join("remote");
        dir.is_dir().then_some(dir)
    }

    /// The remote sidecar's connection, network and address, a cache like `meta.json`: whether a sandbox
    /// is deployed stays decided by its Mutagen session alone.
    pub fn read_remote_meta(&self, sandbox: &str) -> Option<serde_json::Value> {
        mio_files::read_value(&self.remote_dir_if_exists(sandbox)?.join("remote.json"))
    }

    pub(super) fn write_remote_meta(
        &self,
        sandbox: &str,
        connection: &str,
        network: &str,
        proxy_ip: &str,
    ) -> Result<(), ProxyError> {
        let path = self.remote_dir(sandbox)?.join("remote.json");
        let body = serde_json::json!({ "connection": connection, "network": network, "proxyIp": proxy_ip });
        mio_files::write_compact(&path, &body).map_err(|e| ProxyError::CommandFailed(e.to_string()))
    }

    /// The remote sidecar's CA, which the remote sandbox must trust instead of the local one
    /// (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 9).
    pub fn read_remote_ca_pem(&self, sandbox: &str) -> Result<Vec<u8>, ProxyError> {
        let path = self.remote_dir(sandbox)?.join("certs").join("ca.pem");
        std::fs::read(&path).map_err(|e| io_err(&path, e))
    }

    /// Deletes the laptop's remote staging, which holds the remote CA's private key.
    pub fn remove_remote_state(&self, sandbox: &str) {
        if let Some(dir) = self.remote_dir_if_exists(sandbox) {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                tracing::warn!(target: "proxy", %sandbox, error = %e, "failed to remove the remote proxy state");
            }
        }
    }

    /// Deletes the whole state directory of `sandbox`, the CA private key included.
    pub(super) fn remove_state(&self, sandbox: &str) {
        if let Ok(dir) = self.state_dir(sandbox) {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                tracing::warn!(target: "proxy", %sandbox, error = %e, "failed to remove proxy state directory");
            }
        }
    }
}

/// Iron-proxy when `meta` records no engine or one that does not parse.
pub(super) fn meta_engine(meta: &serde_json::Value) -> ProxyEngine {
    serde_json::from_value(meta["engine"].clone()).unwrap_or(ProxyEngine::IronProxy)
}

/// The sidecar's `config/` directory under `state`, a sandbox's state directory or its `remote/`.
pub(super) fn config_dir(state: &Path) -> PathBuf {
    state.join("config")
}

/// The sidecar's `secrets/` directory under `state`.
pub(super) fn secrets_dir(state: &Path) -> PathBuf {
    state.join("secrets")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (ProxyStore, PathBuf) {
        let root = std::env::temp_dir().join(format!("mio-proxy-store-{}", uuid::Uuid::new_v4()));
        (ProxyStore::new(root.join("proxy")), root)
    }

    /// The on-disk layout is shared with installs predating the store, so it must not move.
    #[test]
    fn state_files_keep_their_layout() {
        let (store, root) = temp_store();
        let state = root.join("proxy").join("web");
        assert_eq!(store.state_dir("web").unwrap(), state);
        assert_eq!(store.certs_dir("web").unwrap(), state.join("certs"));
        assert_eq!(store.ca_pem_path("web", ProviderId::Podman).unwrap(), state.join("certs").join("ca.pem"));
        assert_eq!(store.flows_path("web").unwrap(), state.join("flows.ndjson"));
        assert_eq!(store.meta_path("web").unwrap(), state.join("meta.json"));
        assert_eq!(store.values_path("web").unwrap(), state.join("values.json"));
        assert_eq!(store.remote_dir("web").unwrap(), state.join("remote"));
        assert_eq!(store.legacy_secrets_path().unwrap(), root.join("proxy").join("secrets.json"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn reads_create_no_directory_and_values_round_trip() {
        let (store, root) = temp_store();
        assert!(store.load_secrets("web").unwrap().is_empty());
        assert!(store.read_meta("web").is_none());
        assert!(store.read_remote_meta("web").is_none());
        assert!(!root.exists(), "a read must not create the state tree");
        store.set_secret("web", "a", "1").unwrap();
        store.set_secret("web", "b", "2").unwrap();
        store.delete_secret("web", "a").unwrap();
        assert_eq!(store.load_secrets("web").unwrap(), HashMap::from([("b".to_string(), "2".to_string())]));
        assert_eq!(store.sidecar_provider_id("web"), ProviderId::Podman);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
