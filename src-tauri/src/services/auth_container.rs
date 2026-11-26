//! The throwaway container an interactive agent login runs in.

use std::sync::Arc;

use crate::backends::types::SandboxBackend;
use crate::credentials::types::CredentialError;
use mio_domain::types::ProviderId;
use crate::runtime::types::RuntimeManager;

/// Create ephemeral auth container if absent (idempotent; destroyed after capture).
pub async fn ensure(
    name: &str,
    backend: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
) -> Result<(), CredentialError> {
    let exists = backend.list().await?.iter().any(|s| s.name == name);
    if !exists {
        // An auth container is a throwaway the app creates and destroys around one login.
        backend.ensure_base_image(ProviderId::Podman).await?;
        // Never proxied.
        let sandbox = backend.create(name, None, ProviderId::Podman, None, None).await?;
        runtime
            .init(&sandbox.workspace_path)
            .await
            .map_err(|e| CredentialError::ExecFailed(e.to_string()))?;
    }
    Ok(())
}
