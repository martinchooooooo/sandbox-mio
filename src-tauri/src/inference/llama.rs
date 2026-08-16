//! Bundled llama.cpp (Vulkan) engine. Spawns `llama-server` from its bundled resource directory
//! (the Vulkan build is ~50 DLLs incl. a 48 MB `ggml-vulkan.dll`, so it ships as a resource
//! *directory*, not a single `externalBin` sidecar) and serves an OpenAI-compatible endpoint.
//!
//! The process is spawned with `tokio::process` rather than the shell plugin: the plugin's
//! execute scope is pinned to the Mutagen sidecar (`capabilities/default.json`), and the agents
//! module sets the precedent of spawning host tools (`podman`) directly.

use std::time::Duration;

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use super::types::{
    EngineKind, InferenceEngine, InferenceError, RuntimeSetupEvent, ServerHandle, SetupStep,
};
use crate::process::detached_command;
use crate::settings;

/// Default GPU offload when the user has not pinned `llama_gpu_layers`. 99 offloads every layer
/// that fits; llama-server silently caps it to the model's real layer count.
const DEFAULT_GPU_LAYERS: u32 = 99;

/// Health-poll ceiling. A large model can take tens of seconds to load into RAM/VRAM before
/// `/health` returns 200, so the timeout is generous (gotcha: never treat the first poll as fatal).
const HEALTH_TIMEOUT: Duration = Duration::from_secs(180);

pub struct LlamaServerEngine {
    pub app: AppHandle,
}

impl LlamaServerEngine {
    /// The bundled engine directory (`<resources>/llama`), containing `llama-server.exe` + DLLs.
    fn engine_dir(&self) -> Result<std::path::PathBuf, InferenceError> {
        let dir = self
            .app
            .path()
            .resource_dir()
            .map_err(|e| InferenceError::Sidecar(e.to_string()))?
            .join("llama");
        Ok(dir)
    }

    fn server_exe(&self) -> Result<std::path::PathBuf, InferenceError> {
        let name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
        let exe = self.engine_dir()?.join(name);
        if !exe.exists() {
            return Err(InferenceError::Sidecar(format!(
                "bundled llama-server not found at {}",
                exe.to_string_lossy()
            )));
        }
        Ok(exe)
    }

    /// Resolve the downloaded GGUF under `<app_data>/models/<model_id>/`. The directory holds one
    /// `.gguf`, written by `download_gguf`, so the first match is the model.
    fn model_path(&self, model_id: &str) -> Result<std::path::PathBuf, InferenceError> {
        let dir = super::model_dir(&self.app, model_id)?;
        let gguf = std::fs::read_dir(&dir)
            .ok()
            .and_then(|entries| {
                entries.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
                    p.extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| x.eq_ignore_ascii_case("gguf"))
                })
            })
            .ok_or_else(|| InferenceError::ModelNotFound(model_id.to_string()))?;
        Ok(gguf)
    }

    /// Best-effort device probe: run `llama-server --list-devices` and return the first reported
    /// device line (e.g. `Intel(R) Arc(TM) 140V GPU (16GB)`), or `None` if it cannot be read.
    async fn list_device(&self, exe: &std::path::Path, engine_dir: &std::path::Path) -> Option<String> {
        let out = detached_command(exe)
            .current_dir(engine_dir)
            .arg("--list-devices")
            .output()
            .await
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .map(|l| l.trim())
            .find(|l| l.contains("Vulkan") || l.contains("GPU"))
            .map(|l| l.to_string())
    }
}

#[async_trait::async_trait]
impl InferenceEngine for LlamaServerEngine {
    fn setup_steps(&self) -> Vec<SetupStep> {
        ["detect-device", "engine-ready", "model-ready", "launch-server", "health-check"]
            .iter()
            .map(|id| SetupStep::new(id))
            .collect()
    }

    async fn start(
        &self,
        model_id: &str,
        port: u16,
        on_step: &Channel<serde_json::Value>,
    ) -> Result<ServerHandle, InferenceError> {
        let cfg = settings::load(&self.app).unwrap_or_default();
        let gpu_layers = cfg.llama_gpu_layers.unwrap_or(DEFAULT_GPU_LAYERS);
        let exe = self.server_exe()?;
        let engine_dir = self.engine_dir()?;

        // detect-device: best-effort `--list-devices` so the checklist shows the real GPU.
        RuntimeSetupEvent::StepStart { id: "detect-device".into() }.send(on_step);
        let device = self.list_device(&exe, &engine_dir).await;
        RuntimeSetupEvent::StepDone { id: "detect-device".into(), detail: device }.send(on_step);

        // engine-ready: the binary ships inside the app; nothing is installed.
        RuntimeSetupEvent::StepStart { id: "engine-ready".into() }.send(on_step);
        RuntimeSetupEvent::StepSkipped { id: "engine-ready".into(), reason: "bundled".into() }
            .send(on_step);

        // model-ready: the GGUF must already be downloaded (the /models UI downloads first).
        RuntimeSetupEvent::StepStart { id: "model-ready".into() }.send(on_step);
        let model_path = match self.model_path(model_id) {
            Ok(p) => p,
            Err(e) => {
                RuntimeSetupEvent::StepFailed { id: "model-ready".into(), error: e.to_string() }
                    .send(on_step);
                return Err(e);
            }
        };
        RuntimeSetupEvent::StepSkipped { id: "model-ready".into(), reason: "cached".into() }
            .send(on_step);

        // launch-server: spawn from the engine dir so its DLLs resolve.
        RuntimeSetupEvent::StepStart { id: "launch-server".into() }.send(on_step);
        let mut cmd = detached_command(&exe);
        cmd.current_dir(&engine_dir)
            .arg("--model")
            .arg(&model_path)
            .args(["--port", &port.to_string()])
            .args(["--host", "127.0.0.1"])
            .args(["-ngl", &gpu_layers.to_string()]);
        if let Some(ctx) = cfg.llama_ctx_size {
            cmd.args(["--ctx-size", &ctx.to_string()]);
        }
        let child = cmd.spawn().map_err(|e| InferenceError::Sidecar(e.to_string()))?;
        RuntimeSetupEvent::StepDone {
            id: "launch-server".into(),
            detail: Some(format!("GPU layers: {gpu_layers}")),
        }
        .send(on_step);

        // health-check: poll /health until ready, then go live.
        let base_url = format!("http://127.0.0.1:{port}/v1");
        RuntimeSetupEvent::StepStart { id: "health-check".into() }.send(on_step);
        let mut child = child;
        if let Err(e) = wait_for_health(port).await {
            let _ = child.kill().await; // reap the orphan before surfacing the failure
            RuntimeSetupEvent::StepFailed { id: "health-check".into(), error: e.to_string() }
                .send(on_step);
            return Err(e);
        }
        RuntimeSetupEvent::StepDone { id: "health-check".into(), detail: None }.send(on_step);
        RuntimeSetupEvent::Ready { base_url: base_url.clone(), model_id: model_id.to_string() }
            .send(on_step);

        Ok(ServerHandle {
            engine: EngineKind::Llama,
            model_id: model_id.to_string(),
            alias: model_id.to_string(),
            port,
            base_url,
            child: Some(child),
        })
    }

    async fn stop(&self, handle: ServerHandle) -> Result<(), InferenceError> {
        if let Some(mut child) = handle.child {
            child.kill().await.map_err(|e| InferenceError::CommandFailed(e.to_string()))?;
        }
        Ok(())
    }
}

/// Poll `GET http://127.0.0.1:<port>/health` until it returns 200 or [`HEALTH_TIMEOUT`] elapses.
/// A connection refusal early on is expected while the model loads, so failures are retried.
async fn wait_for_health(port: u16) -> Result<(), InferenceError> {
    let url = format!("http://127.0.0.1:{port}/health");
    let client = reqwest::Client::new();
    let deadline = std::time::Instant::now() + HEALTH_TIMEOUT;
    loop {
        if let Ok(resp) = client.get(&url).timeout(Duration::from_secs(2)).send().await {
            if resp.status().is_success() {
                return Ok(());
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(InferenceError::ServerNotReady(format!(
                "/health did not pass within {}s",
                HEALTH_TIMEOUT.as_secs()
            )));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
