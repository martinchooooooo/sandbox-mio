//! In-memory mock of [`RuntimeManager`], gated by `MIO_MOCK_BACKEND=1` (see `default_runtime`).
//! Lets the e2e "light" test tier drive real Tauri commands without shelling out to `podman`/`mise`.

use std::collections::HashMap;

use tokio::sync::Mutex;

use super::types::{RegistryEntry, RuntimeError, RuntimeManager};

/// Tracks installed packages per sandbox so the light e2e tier can prove per-sandbox package
/// state actually round-trips (e.g. that switching sandboxes doesn't leak one's package list into
/// another's). `install`/`uninstall` key by sandbox `name`; `list` keys by `workspace_host_path` —
/// mirrored here via [`MockBackend`](crate::backends::mock::MockBackend)'s
/// `/mock/workspace/{name}` convention, since the mock has no other way to relate the two.
pub(super) struct MockRuntime {
    packages: Mutex<HashMap<String, Vec<String>>>,
}

impl MockRuntime {
    pub(super) fn new() -> Self {
        Self { packages: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl RuntimeManager for MockRuntime {
    async fn init(&self, _workspace_host_path: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    async fn install(&self, name: &str, package: &str) -> Result<(), RuntimeError> {
        let mut packages = self.packages.lock().await;
        let installed = packages.entry(format!("/mock/workspace/{name}")).or_default();
        if !installed.iter().any(|p| p == package) {
            installed.push(package.to_string());
        }
        Ok(())
    }

    async fn uninstall(&self, name: &str, package: &str) -> Result<(), RuntimeError> {
        let mut packages = self.packages.lock().await;
        if let Some(installed) = packages.get_mut(&format!("/mock/workspace/{name}")) {
            installed.retain(|p| p != package);
        }
        Ok(())
    }

    async fn list(&self, workspace_host_path: &str) -> Result<Vec<String>, RuntimeError> {
        Ok(self.packages.lock().await.get(workspace_host_path).cloned().unwrap_or_default())
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
