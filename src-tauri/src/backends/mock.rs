//! In-memory mock of [`SandboxBackend`], gated by `MIO_MOCK_BACKEND=1` (see `default_backend`).
//! Lets the e2e "light" test tier drive real Tauri commands without shelling out to `podman`.

use tauri::ipc::Channel;

use super::types::{BackendError, RemoteDeployment, Sandbox, SandboxBackend};

/// Plain in-memory state machine standing in for a real Podman connection. State is a `Vec` of
/// [`Sandbox`] guarded by a `Mutex`; no persistence, no external process, no events.
pub(super) struct MockBackend {
    state: tokio::sync::Mutex<Vec<Sandbox>>,
}

impl MockBackend {
    pub(super) fn new() -> Self {
        Self { state: tokio::sync::Mutex::new(Vec::new()) }
    }
}

#[async_trait::async_trait]
impl SandboxBackend for MockBackend {
    async fn ensure_base_image(&self) -> Result<(), BackendError> {
        Ok(())
    }

    async fn create(&self, name: &str, existing_path: Option<&str>) -> Result<Sandbox, BackendError> {
        let sandbox = Sandbox {
            id: name.to_string(),
            name: name.to_string(),
            status: "created".to_string(),
            workspace_path: existing_path
                .map(str::to_string)
                .unwrap_or_else(|| format!("/mock/workspace/{name}")),
            image_version: Some(1),
        };
        let mut state = self.state.lock().await;
        state.push(sandbox.clone());
        Ok(sandbox)
    }

    async fn list(&self) -> Result<Vec<Sandbox>, BackendError> {
        Ok(self.state.lock().await.clone())
    }

    async fn start(&self, name: &str) -> Result<(), BackendError> {
        let mut state = self.state.lock().await;
        let sandbox = state
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or_else(|| BackendError::CommandFailed(format!("mock: no such sandbox {name}")))?;
        sandbox.status = "running".to_string();
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<(), BackendError> {
        let mut state = self.state.lock().await;
        let sandbox = state
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or_else(|| BackendError::CommandFailed(format!("mock: no such sandbox {name}")))?;
        sandbox.status = "exited".to_string();
        Ok(())
    }

    async fn destroy(&self, name: &str) -> Result<(), BackendError> {
        // Idempotent: a missing sandbox is not an error.
        self.state.lock().await.retain(|s| s.name != name);
        Ok(())
    }

    async fn deploy(
        &self,
        name: &str,
        target_connection: &str,
    ) -> Result<RemoteDeployment, BackendError> {
        Ok(RemoteDeployment {
            alpha: format!("/mock/workspace/{name}"),
            beta: format!("{target_connection}:/mock/remote/{name}"),
        })
    }

    async fn remove_remote(&self, _name: &str, _connection: &str) -> Result<(), BackendError> {
        Ok(())
    }

    async fn listen_events(&self, _on_event: Channel<serde_json::Value>) -> Result<(), BackendError> {
        // No events to stream. Light specs that need synthetic events use the
        // `plugin:event|emit` Tauri bridge directly (see `e2e/AGENTS.md`), not this stream.
        Ok(())
    }

    async fn list_hosts(&self) -> Result<Vec<String>, BackendError> {
        Ok(vec![])
    }

    async fn add_host(
        &self,
        _name: &str,
        _ssh_url: &str,
        _identity: Option<&str>,
    ) -> Result<(), BackendError> {
        Ok(())
    }

    async fn prepare_host(&self, _ssh_url: &str, _identity_path: &str) -> Result<(), BackendError> {
        Ok(())
    }

    async fn provision_host(
        &self,
        _name: &str,
        _ssh_url: &str,
        _identity_path: &str,
        _on_progress: Channel<serde_json::Value>,
    ) -> Result<(), BackendError> {
        Ok(())
    }
}
