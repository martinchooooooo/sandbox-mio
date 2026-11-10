//! Agent install, interactive auth sessions, and the OpenCode/pi host-side config writes.

use std::sync::Arc;

use tauri::{ipc::Channel, State};

use crate::agents;
use crate::agents::types::AuthSessionMap;
use crate::credentials;
use crate::error::AppError;
use crate::provider::ProviderRegistry;
use crate::runtime::types::RuntimeManager;

/// Install agent packages and run postinstall steps. Auto-injects credentials if available.
#[tauri::command]
pub async fn install_agent(
    name: String,
    packages: Vec<String>,
    postinstall_npm: Option<(String, String)>,
    credential_service: Option<String>,
    onboarding: Option<credentials::types::OnboardingSeed>,
    on_progress: Channel<serde_json::Value>,
    app: tauri::AppHandle,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    registry: State<'_, Arc<ProviderRegistry>>,
) -> Result<(), AppError> {
    agents::install_packages(&name, &packages, &on_progress, &runtime).await?;
    if let Some((npm_package, node_tool)) = postinstall_npm {
        agents::run_postinstall_npm(&name, &npm_package, &node_tool, &on_progress, &runtime).await?;
    }
    if let Some(service_id) = credential_service {
        if credentials::load_manifest(&app, &service_id).is_ok() {
            // Resolved here rather than up front: `engine_of` costs a list call per provider, and
            // an install without stored credentials never needs one.
            let provider = registry.engine_of(&name).await;
            credentials::inject(&app, &name, &service_id, &provider).await?;
            if let Some(seed) = &onboarding {
                credentials::seed_onboarding(&name, seed, &provider).await?;
            }
        }
    }
    on_progress.send(serde_json::to_value(&agents::types::AgentInstallEvent::Done).unwrap()).ok();
    Ok(())
}

/// Write/merge OpenCode provider config host-side (preserves existing user config).
#[tauri::command]
pub async fn provision_opencode(
    workspace_path: String,
    relative_path: String,
    provider_id: String,
    provider_json: serde_json::Value,
    default_model: Option<String>,
) -> Result<(), AppError> {
    Ok(agents::config::provision_opencode(
        &workspace_path,
        &relative_path,
        provider_id,
        provider_json,
        default_model,
    )?)
}

/// Write pi's local-llm provider config, default model and env redirect host-side
/// (container-state independent).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn provision_pi(
    workspace_path: String,
    relative_path: String,
    provider_id: String,
    provider_json: serde_json::Value,
    default_provider: Option<String>,
    default_model: Option<String>,
    config_env: String,
    config_dir_container: String,
) -> Result<(), AppError> {
    Ok(agents::config::provision_pi(
        &workspace_path,
        &relative_path,
        provider_id,
        provider_json,
        default_provider,
        default_model,
        config_env,
        config_dir_container,
    )?)
}

/// Spawn agent auth command with piped I/O. Emits `AgentAuthEvent` to `agent-auth-output-<session_id>`.
#[tauri::command]
pub async fn start_agent_auth(
    name: String,
    auth_command: Vec<String>,
    app: tauri::AppHandle,
    sessions: State<'_, AuthSessionMap>,
    registry: State<'_, Arc<ProviderRegistry>>,
) -> Result<String, AppError> {
    let provider = registry.engine_of(&name).await;
    Ok(agents::start_auth(&name, &auth_command, &app, &sessions, &provider).await?)
}

/// Write to auth process stdin. One-shot for PKCE flows.
#[tauri::command]
pub async fn send_agent_auth_input(
    session_id: String,
    input: String,
    sessions: State<'_, AuthSessionMap>,
) -> Result<(), AppError> {
    Ok(agents::send_auth_input(&session_id, &input, &sessions).await?)
}

/// Cancel auth session by closing stdin.
#[tauri::command]
pub async fn cancel_agent_auth(
    session_id: String,
    sessions: State<'_, AuthSessionMap>,
) -> Result<(), AppError> {
    Ok(agents::cancel_auth(&session_id, &sessions).await?)
}

/// Read agent credential file and return auth status.
#[tauri::command]
pub async fn get_agent_auth_status(
    name: String,
    credential_path: String,
    expires_at_json_key: String,
    account_source_path: Option<String>,
    account_email_json_key: Option<String>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<agents::types::AgentAuthStatus, AppError> {
    Ok(agents::get_auth_status(
        &name,
        &credential_path,
        &expires_at_json_key,
        account_source_path.as_deref(),
        account_email_json_key.as_deref(),
        &runtime,
    )
    .await?)
}
