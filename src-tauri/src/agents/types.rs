use crate::runtime::types::RuntimeError;

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
}

impl From<RuntimeError> for AgentError {
    fn from(e: RuntimeError) -> Self {
        AgentError::InstallFailed(e.to_string())
    }
}

#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind")]
pub enum AgentInstallEvent {
    PackageStart { package: String },
    PackageDone { package: String },
    PostInstallStart { step: String },
    PostInstallDone { step: String },
    Done,
}

#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind")]
pub enum AgentAuthEvent {
    Output { line: String },
    Exit { code: i32 },
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthStatus {
    pub authenticated: bool,
    pub expires_at_ms: Option<u64>,
    pub account_email: Option<String>,
}

pub struct AuthSession {
    pub stdin: tokio::process::ChildStdin,
}

pub type AuthSessionMap =
    std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, AuthSession>>>;
