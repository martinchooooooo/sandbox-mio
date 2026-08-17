//! Machine-spec probing, model downloads, and local inference server lifecycle.

use tauri::{ipc::Channel, State};

use crate::activity;
use crate::activity::types::TaskKind;
use crate::error::AppError;
use crate::inference;
use crate::inference::types::{
    ActiveServer, EngineKind, InferenceStatus, MachineSpecs, MachineSpecsCache, SetupStep,
};
use crate::sync;

/// Probe host CPU/RAM/GPU/NPU and Foundry availability (runs on host, uses cache after first call).
#[tauri::command]
pub async fn detect_machine_specs(cache: State<'_, MachineSpecsCache>) -> Result<MachineSpecs, AppError> {
    let cache = cache.inner().clone();
    let specs = tauri::async_runtime::spawn_blocking(move || inference::detect_specs(&cache))
        .await
        .map_err(|e| inference::types::InferenceError::CommandFailed(e.to_string()))?;
    Ok(specs)
}

/// Force full re-probe (clears cache).
#[tauri::command]
pub async fn redetect_machine_specs(cache: State<'_, MachineSpecsCache>) -> Result<MachineSpecs, AppError> {
    let cache = cache.inner().clone();
    let specs = tauri::async_runtime::spawn_blocking(move || inference::redetect_specs(&cache))
        .await
        .map_err(|e| inference::types::InferenceError::CommandFailed(e.to_string()))?;
    Ok(specs)
}

/// Download a GGUF model (no-op if present). Enrolled in activity center; failure recorded before propagating.
#[tauri::command]
pub async fn download_model(
    model_id: String,
    repo: String,
    file: String,
    sha256: String,
    on_progress: Channel<serde_json::Value>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let id = format!("model-download:{model_id}");
    activity::scope(&app, &id, TaskKind::ModelDownload, Some(model_id.clone()), |_| async {
        inference::download_gguf(&app, &model_id, &repo, &file, &sha256, &on_progress)
            .await
            .map_err(activity::TaskFailure::same)
    })
    .await?;
    Ok(())
}

/// List downloaded models.
#[tauri::command]
pub fn downloaded_models(app: tauri::AppHandle) -> Result<Vec<String>, AppError> {
    Ok(inference::downloaded(&app)?)
}

/// Return engine's ordered setup checklist (no-op).
#[tauri::command]
pub fn inference_setup_steps(engine: EngineKind, app: tauri::AppHandle) -> Result<Vec<SetupStep>, AppError> {
    Ok(inference::engine_for(engine, &app).setup_steps())
}

/// Start app-wide inference server (stops prior server first). Streams setup progress.
#[tauri::command]
pub async fn start_inference(
    engine: EngineKind,
    model_id: String,
    on_progress: Channel<serde_json::Value>,
    active: State<'_, ActiveServer>,
    app: tauri::AppHandle,
) -> Result<InferenceStatus, AppError> {
    tracing::info!(target: "inference", ?engine, %model_id, "start_inference requested");
    let mut guard = active.lock().await;
    if let Some(existing) = guard.take() {
        tracing::info!(target: "inference", model_id = %existing.model_id, "stopping prior server");
        let prev = inference::engine_for(existing.engine, &app);
        let _ = prev.stop(existing).await;
    }
    let port = inference::ephemeral_port()?;
    let engine_impl = inference::engine_for(engine, &app);

    let total_steps = engine_impl.setup_steps().len();
    let app_for_setup = app.clone();
    let handle = activity::scope(
        &app,
        "inference-setup",
        TaskKind::InferenceSetup,
        Some(model_id.clone()),
        |task| async move {
            let channel = activity::step_progress_channel(
                &app_for_setup,
                "inference-setup",
                total_steps,
                on_progress,
            );
            let handle = engine_impl.start(&model_id, port, &channel).await.map_err(|e| {
                tracing::error!(target: "inference", ?engine, %model_id, error = %e, "start failed");
                activity::TaskFailure::same(e)
            })?;
            task.set_success_detail(handle.base_url.clone());
            Ok(handle)
        },
    )
    .await?;
    tracing::info!(target: "inference", base_url = %handle.base_url, model_id = %handle.model_id, "server ready");

    // The inference server is reached from inside the Podman VM regardless of which provider a
    // sandbox runs on; microsandbox instances dial the same host loopback through their own netns.
    let connection = crate::backends::connection_name();
    match crate::backends::ensure_connection_alias(&connection).await {
        Ok(()) => {
            if let Err(e) = sync::forward_create_reverse(
                &app,
                inference::INFERENCE_FORWARD,
                &connection,
                sync::INFERENCE_VM_PORT,
                handle.port,
            )
            .await
            {
                tracing::warn!(target: "inference", error = %e, "inference reverse forward failed");
            }
        }
        Err(e) => {
            tracing::warn!(target: "inference", error = %e, "could not resolve connection alias for inference forward");
        }
    }

    let status = handle.status();
    *guard = Some(handle);
    Ok(status)
}

#[tauri::command]
pub async fn stop_inference(active: State<'_, ActiveServer>, app: tauri::AppHandle) -> Result<(), AppError> {
    let handle = active.lock().await.take();
    if let Some(handle) = handle {
        let engine = inference::engine_for(handle.engine, &app);
        engine.stop(handle).await?;
    }
    // Spawned off the critical path: `forward terminate` can take seconds and the frontend
    // awaits this command; a briefly dangling forward just gets connection-refused.
    let app_for_forward = app.clone();
    tauri::async_runtime::spawn(async move {
        sync::forward_terminate(&app_for_forward, inference::INFERENCE_FORWARD).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn inference_status(active: State<'_, ActiveServer>) -> Result<InferenceStatus, AppError> {
    let guard = active.lock().await;
    Ok(match guard.as_ref() {
        Some(handle) => handle.status(),
        None => InferenceStatus { running: false, engine: None, model_id: None, base_url: None },
    })
}

/// Install Foundry Local via winget. Streams CLI output.
#[tauri::command]
pub async fn install_foundry_local(on_progress: Channel<serde_json::Value>) -> Result<(), AppError> {
    inference::install_foundry(&on_progress).await?;
    Ok(())
}

/// Read `MIO_TEST_MODEL` env var (test-only GGUF descriptor).
#[tauri::command]
pub fn test_model() -> Option<String> {
    std::env::var("MIO_TEST_MODEL").ok().filter(|s| !s.trim().is_empty())
}
