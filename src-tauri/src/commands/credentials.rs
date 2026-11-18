//! The ephemeral auth container, credential capture and injection, and bundle listing.

use std::sync::Arc;

use tauri::State;

use crate::backends::types::SandboxBackend;
use crate::credentials;
use crate::error::AppError;
use crate::provider::types::ExecOpts;
use crate::provider::ProviderRegistry;
use crate::runtime::types::RuntimeManager;

/// Create ephemeral auth container if absent. Idempotent.
#[tauri::command]
pub async fn ensure_auth_container(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<(), AppError> {
    Ok(credentials::ensure_auth_container(&name, &store, &runtime).await?)
}

/// Capture agent credentials from container into host store.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn capture_agent_credentials(
    name: String,
    service_id: String,
    credential_path: String,
    expires_at_json_key: String,
    account_source_path: Option<String>,
    account_email_json_key: Option<String>,
    app: tauri::AppHandle,
    registry: State<'_, Arc<ProviderRegistry>>,
) -> Result<credentials::types::CredentialManifest, AppError> {
    let provider = registry.engine_of(&name).await;
    Ok(credentials::capture(
        &app,
        &name,
        &service_id,
        &credential_path,
        &expires_at_json_key,
        account_source_path.as_deref(),
        account_email_json_key.as_deref(),
        &provider,
    )
    .await?)
}

/// Inject stored credentials into container. Seeds onboarding to skip wizard if given.
#[tauri::command]
pub async fn inject_agent_credentials(
    name: String,
    service_id: String,
    onboarding: Option<credentials::types::OnboardingSeed>,
    app: tauri::AppHandle,
    registry: State<'_, Arc<ProviderRegistry>>,
) -> Result<(), AppError> {
    let provider = registry.engine_of(&name).await;
    credentials::inject(&app, &name, &service_id, &provider).await?;
    if let Some(seed) = onboarding {
        credentials::seed_onboarding(&name, &seed, &provider).await?;
    }
    Ok(())
}

/// Mark agent first-run complete to skip onboarding wizard in terminal.
#[tauri::command]
pub async fn seed_agent_onboarding(
    name: String,
    onboarding: credentials::types::OnboardingSeed,
    registry: State<'_, Arc<ProviderRegistry>>,
) -> Result<(), AppError> {
    let provider = registry.engine_of(&name).await;
    Ok(credentials::seed_onboarding(&name, &onboarding, &provider).await?)
}

/// List every service with persisted credentials.
#[tauri::command]
pub fn list_credential_bundles(
    app: tauri::AppHandle,
) -> Result<Vec<(String, credentials::types::CredentialManifest)>, AppError> {
    Ok(credentials::list(&app)?)
}

/// Delete host credential bundle (container copies are untouched). Deleting the GitHub token also
/// removes it from running sandboxes now, and from stopped ones on their next start.
#[tauri::command]
pub fn delete_credential_bundle(service_id: String, app: tauri::AppHandle) -> Result<(), AppError> {
    credentials::delete(&app, &service_id)?;
    if service_id == credentials::GITHUB_SERVICE {
        crate::services::github_access::spawn_reconcile_all(&app);
    }
    Ok(())
}

/// Save the GitHub token used by in-sandbox `mise` and `gh`. The value is write-only over IPC: only
/// the manifest comes back, and `delete_credential_bundle("github")` removes it.
#[tauri::command]
pub async fn set_github_token(
    token: String,
    app: tauri::AppHandle,
) -> Result<credentials::types::CredentialManifest, AppError> {
    let manifest = credentials::github::save_github_token(&app, &token).await?;
    crate::services::github_access::spawn_reconcile_all(&app);
    Ok(manifest)
}

/// Read the token a finished `gh auth login` left in the GitHub auth container and save it as the
/// GitHub credential. The container is fixed, so a user sandbox's files stay unreadable.
#[tauri::command]
pub async fn capture_github_login(
    app: tauri::AppHandle,
    registry: State<'_, Arc<ProviderRegistry>>,
) -> Result<credentials::types::CredentialManifest, AppError> {
    let name = credentials::types::GITHUB_AUTH_CONTAINER;
    let provider = registry.engine_of(name).await;
    let out = provider
        .exec(name, &["gh", "auth", "token"], ExecOpts::default())
        .await
        .map_err(credentials::types::CredentialError::from)?;
    if !out.status.success() {
        return Err(credentials::types::CredentialError::ExecFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        )
        .into());
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let manifest = credentials::github::save_github_token(&app, &token).await?;
    crate::services::github_access::spawn_reconcile_all(&app);
    Ok(manifest)
}

/// What the Manage card shows for `name`: derived host-side, with no exec.
#[tauri::command]
pub async fn github_sandbox_status(
    name: String,
    app: tauri::AppHandle,
) -> Result<crate::services::github_access::GithubSandboxStatus, AppError> {
    crate::services::github_access::status(&app, &name).await
}

/// Turn the GitHub login on or off for one sandbox and apply it now. Re-sending the current value
/// retries a failed apply.
#[tauri::command]
pub async fn set_github_sandbox_access(
    name: String,
    enabled: bool,
    app: tauri::AppHandle,
) -> Result<crate::services::github_access::GithubSandboxStatus, AppError> {
    crate::services::github_access::set_access(&app, &name, enabled).await?;
    crate::services::github_access::status(&app, &name).await
}
