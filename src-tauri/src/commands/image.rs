//! Base-image build and readiness, the dismissed-version markers, and provider capabilities.

use std::sync::Arc;

use tauri::State;

use crate::activity;
use crate::activity::types::TaskKind;
use crate::backends;
use crate::backends::types::SandboxBackend;
use crate::error::AppError;
use crate::provider::types::{ProviderCapabilities, ProviderId};
use crate::provider::ProviderRegistry;

#[tauri::command]
pub fn base_image_version() -> u32 {
    backends::BASE_IMAGE_VERSION
}

/// Stages the base image on its own, without creating a sandbox: the onboarding flow's "download
/// necessities" step. Deliberately skips `create_sandbox`'s `runtime.init` — no workspace exists yet.
/// `provider` defaults to Podman, whose image every provider's base image is built from
/// (`docs/features/microsandbox-provider/02_dev.md`, Resolved Decision 2).
/// Reported as an activity task: the first build takes minutes and may run in the background.
#[tauri::command]
pub async fn build_base_image(
    provider: Option<ProviderId>,
    store: State<'_, Arc<dyn SandboxBackend>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let provider = provider.unwrap_or(ProviderId::Podman);
    activity::scope(&app, "base-image-build", TaskKind::BaseImageBuild, None, |_task| async {
        store.ensure_base_image(provider).await.map_err(|e| {
            let message = e.to_string();
            activity::TaskFailure::new(message, AppError::from(e))
        })
    })
    .await
}

#[tauri::command]
pub async fn base_image_ready(
    provider: Option<ProviderId>,
    store: State<'_, Arc<dyn SandboxBackend>>,
) -> Result<bool, AppError> {
    Ok(store.base_image_ready(provider.unwrap_or(ProviderId::Podman)).await?)
}

#[tauri::command]
pub fn get_dismissed_image_version(workspace_path: String) -> Option<u32> {
    backends::dismissed_image_version(&workspace_path)
}

#[tauri::command]
pub fn dismiss_image_update(workspace_path: String, version: u32) -> Result<(), AppError> {
    Ok(backends::dismiss_image_update(&workspace_path, version)?)
}

/// Feature differences between provider engines, for UI that must hide what a provider cannot do
/// (the deploy picker, per `ProviderCapabilities.remote_targets`).
#[tauri::command]
pub fn provider_capabilities(
    provider: ProviderId,
    registry: State<'_, Arc<ProviderRegistry>>,
) -> Result<ProviderCapabilities, AppError> {
    Ok(registry.local(provider).capabilities())
}
