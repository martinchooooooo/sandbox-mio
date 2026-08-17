//! The ephemeral auth container, credential capture and injection, and bundle listing.

use std::sync::Arc;

use tauri::State;

use crate::backends::types::SandboxBackend;
use crate::credentials;
use crate::error::AppError;
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

/// Delete host credential bundle (container copies are untouched).
#[tauri::command]
pub fn delete_credential_bundle(service_id: String, app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(credentials::delete(&app, &service_id)?)
}
