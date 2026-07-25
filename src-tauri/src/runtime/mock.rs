//! In-memory mock of [`RuntimeManager`], gated by `MIO_MOCK_BACKEND=1` (see `default_runtime`).
//! Lets the e2e "light" test tier drive real Tauri commands without shelling out to `podman`/`mise`.

use std::collections::HashMap;

use tokio::sync::Mutex;

use super::types::{RegistryEntry, RuntimeError, RuntimeManager};

/// Tracks installed packages per sandbox so the light e2e tier can prove per-sandbox package
/// state actually round-trips (e.g. that switching sandboxes doesn't leak one's package list into
/// another's). `install`/`uninstall` key by sandbox `name` directly; `list` only receives
/// `workspace_host_path`, so it keys off that path's final segment instead — which assumes the
/// workspace basename equals the sandbox name.
///
/// That assumption holds only for sandboxes created without the `existing_path` argument to
/// `resolve_create_workspace` (`backends/mod.rs`), which derives `workspace_root().join(name)`.
/// An `existing_path` sandbox keeps the caller's directory, whose basename may differ, and then
/// `list` reads back empty however many packages `install` recorded. Light-tier package assertions
/// must therefore use sandboxes created without `existing_path`.
pub(super) struct MockRuntime {
    packages: Mutex<HashMap<String, Vec<String>>>,
}

impl MockRuntime {
    pub(super) fn new() -> Self {
        Self { packages: Mutex::new(HashMap::new()) }
    }
}

fn workspace_name(workspace_host_path: &str) -> &str {
    workspace_host_path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(workspace_host_path)
}

#[async_trait::async_trait]
impl RuntimeManager for MockRuntime {
    async fn init(&self, _workspace_host_path: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    async fn install(&self, name: &str, package: &str) -> Result<(), RuntimeError> {
        let mut packages = self.packages.lock().await;
        let installed = packages.entry(name.to_string()).or_default();
        if !installed.iter().any(|p| p == package) {
            installed.push(package.to_string());
        }
        Ok(())
    }

    async fn uninstall(&self, name: &str, package: &str) -> Result<(), RuntimeError> {
        let mut packages = self.packages.lock().await;
        if let Some(installed) = packages.get_mut(name) {
            installed.retain(|p| p != package);
        }
        Ok(())
    }

    async fn list(&self, workspace_host_path: &str) -> Result<Vec<String>, RuntimeError> {
        let name = workspace_name(workspace_host_path);
        Ok(self.packages.lock().await.get(name).cloned().unwrap_or_default())
    }

    async fn reproduce_remote(
        &self,
        _name: &str,
        _target_connection: &str,
        _workspace_host_path: &str,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }

    async fn prepare_terminal(
        &self,
        _name: &str,
        _connection: &str,
        _workspace_host_path: &str,
    ) -> Result<(String, u16), RuntimeError> {
        Ok(("mock-host".to_string(), 0))
    }

    async fn prepare_agent_web(
        &self,
        _name: &str,
        _connection: &str,
        _password: &str,
    ) -> Result<u16, RuntimeError> {
        Ok(0)
    }

    fn local_connection(&self) -> &str {
        "mock"
    }

    async fn registry(&self, _name: &str) -> Result<Vec<RegistryEntry>, RuntimeError> {
        Ok(vec![])
    }

    async fn tool_dir(&self, _name: &str, tool: &str) -> Result<String, RuntimeError> {
        Ok(format!("/mock/tools/{tool}"))
    }

    async fn exec_in_container(&self, _name: &str, _args: &[&str]) -> Result<String, RuntimeError> {
        Ok(String::new())
    }
}
