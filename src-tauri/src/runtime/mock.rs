//! In-memory mock of [`RuntimeManager`], gated by `MIO_MOCK_BACKEND=1` (see `default_runtime`).
//! Lets the e2e "light" test tier drive real Tauri commands without shelling out to `podman`/`mise`.

use super::types::{RegistryEntry, RuntimeError, RuntimeManager};

/// Stateless stand-in: every method returns a canned success, since the light tier only needs
/// the Tauri command surface to behave, not real tool installs.
pub(super) struct MockRuntime;

#[async_trait::async_trait]
impl RuntimeManager for MockRuntime {
    async fn init(&self, _workspace_host_path: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    async fn install(&self, _name: &str, _package: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    async fn uninstall(&self, _name: &str, _package: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    async fn list(&self, _workspace_host_path: &str) -> Result<Vec<String>, RuntimeError> {
        Ok(vec![])
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
