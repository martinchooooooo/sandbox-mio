use crate::backends::types::BackendError;
use crate::runtime::types::RuntimeError;

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("credential io error: {0}")]
    Io(String),
    #[error("credential parse error: {0}")]
    Parse(String),
    #[error("failed to exec in container: {0}")]
    ExecFailed(String),
    #[error("no credential bundle for service {0:?}")]
    NotFound(String),
}

impl From<RuntimeError> for CredentialError {
    fn from(e: RuntimeError) -> Self {
        CredentialError::ExecFailed(e.to_string())
    }
}

impl From<BackendError> for CredentialError {
    fn from(e: BackendError) -> Self {
        CredentialError::ExecFailed(e.to_string())
    }
}

/// Reserved container-name prefix marking a container as an ephemeral auth container rather than
/// a user sandbox; `list_sandboxes` (`lib.rs`) filters these out before returning to the frontend.
/// Must start with a letter or digit — podman rejects names starting with `_`.
pub const AUTH_CONTAINER_PREFIX: &str = "mio-auth-";

/// Host-side record of a captured credential bundle. The only credential-adjacent data that
/// crosses the Tauri IPC boundary — the credential and account-merge files themselves never do.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CredentialManifest {
    pub captured_at_ms: u64,
    pub account_email: Option<String>,
    pub expires_at_ms: Option<u64>,
}
