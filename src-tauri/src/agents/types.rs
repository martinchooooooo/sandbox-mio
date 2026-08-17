use crate::runtime::types::RuntimeError;

/// Errors during agent package install, auth, or credential capture.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("install failed: {0}")]
    InstallFailed(String),
    #[error("postinstall failed: {0}")]
    PostInstallFailed(String),
    #[error("auth failed: {0}")]
    AuthFailed(String),
    #[error("auth session not found: {0}")]
    AuthSessionNotFound(String),
    #[error("credential read failed: {0}")]
    CredentialReadFailed(String),
    #[error("config write failed: {0}")]
    ConfigWriteFailed(String),
}

impl From<RuntimeError> for AgentError {
    fn from(e: RuntimeError) -> Self {
        AgentError::InstallFailed(e.to_string())
    }
}

/// Streamed progress events during package install. Emitted to frontend channel and/or activity center.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind")]
pub enum AgentInstallEvent {
    PackageStart { package: String },
    PackageDone { package: String },
    PostInstallStart { step: String },
    PostInstallDone { step: String },
    Done,
}

/// Streamed output from an agent auth flow (e.g., PKCE browser redirect). Emitted to `agent-auth-output-<session_id>`.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind")]
pub enum AgentAuthEvent {
    Output { line: String },
    Exit { code: i32 },
}

/// Current auth state for one agent credential. Read from the agent's persisted credential file.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthStatus {
    /// Whether a valid (non-expired) credential exists.
    pub authenticated: bool,
    /// Expiry time if set, allowing the UI to warn before expiration.
    pub expires_at_ms: Option<u64>,
    /// Account email if the credential file stores it (varies by service).
    pub account_email: Option<String>,
}

/// An active auth subprocess. Holds its stdin for one-shot writes (PKCE flows).
pub struct AuthSession {
    pub stdin: tokio::process::ChildStdin,
}

pub type AuthSessionMap =
    std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, AuthSession>>>;
