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

pub const AUTH_CONTAINER_PREFIX: &str = "mio-auth-";

/// Only credential metadata crosses the IPC boundary; the actual files never do.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CredentialManifest {
    pub captured_at_ms: u64,
    pub account_email: Option<String>,
    pub expires_at_ms: Option<u64>,
}

/// Skip onboarding wizard for agents that only received injected credentials.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingSeed {
    pub account_source_path: String,
    pub workspace_dir: String,
    pub account_file_patch: serde_json::Value,
    pub project_trust_keys: Vec<String>,
}
