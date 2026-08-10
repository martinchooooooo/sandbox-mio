use crate::backends::types::BackendError;
use crate::provider::types::ProviderError;
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

impl From<ProviderError> for CredentialError {
    fn from(e: ProviderError) -> Self {
        CredentialError::ExecFailed(e.to_string())
    }
}

/// Prefix for ephemeral auth containers created to capture credentials.
pub const AUTH_CONTAINER_PREFIX: &str = "mio-auth-";

/// Credential metadata exposed to the frontend. Files stay host-side; only metadata crosses IPC.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CredentialManifest {
    /// When the credential was last captured from the container.
    pub captured_at_ms: u64,
    /// Account email if the credential file stored it.
    pub account_email: Option<String>,
    /// Expiry time if the credential has one.
    pub expires_at_ms: Option<u64>,
}

/// Instructions to skip agent onboarding after credential injection.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingSeed {
    /// Path in the container's account metadata file (e.g., `~/.paseo/config`).
    pub account_source_path: String,
    pub workspace_dir: String,
    /// JSON patch to apply to the account metadata for account linking.
    pub account_file_patch: serde_json::Value,
    pub project_trust_keys: Vec<String>,
}
