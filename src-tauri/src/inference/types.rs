//! Types for the host-side local-inference subsystem: the engine trait, the probe result,
//! the streamed setup/download events, and the live-server handle held in Tauri state.

use tauri::ipc::Channel;

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    /// The bundled `llama-server` sidecar/resource could not be resolved or spawned.
    #[error("inference sidecar error: {0}")]
    Sidecar(String),
    #[error("Foundry Local is not installed")]
    FoundryUnavailable,
    #[error("{0}")]
    CommandFailed(String),
    /// The server's health endpoint did not pass within the timeout.
    #[error("inference server not ready: {0}")]
    ServerNotReady(String),
    #[error("model download failed: {0}")]
    Download(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("model not found: {0}")]
    ModelNotFound(String),
}

/// Host capability probe returned by `detect_machine_specs`. Read on the host, never the
/// container VM (whose 2 GiB cap is unrepresentative of what a model can use).
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MachineSpecs {
    pub cpu_cores: usize,
    pub total_ram_bytes: u64,
    pub free_ram_bytes: u64,
    pub gpu_names: Vec<String>,
    /// A Copilot+-class accelerator (e.g. `Intel(R) AI Boost`) is present.
    pub has_npu: bool,
    pub arch: String,
    pub foundry_installed: bool,
    /// The `Microsoft.FoundryLocal` winget package resolves on this machine.
    pub foundry_installable: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    Llama,
    Foundry,
}

impl Default for EngineKind {
    fn default() -> Self {
        EngineKind::Llama
    }
}

/// Streamed from `download_model` to the frontend `Channel`.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind")]
pub enum ModelDownloadEvent {
    Started { total_bytes: u64 },
    Progress { downloaded_bytes: u64, total_bytes: u64 },
    Verifying,
    Done,
}

/// Live server description; backs the `/models` status surface.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InferenceStatus {
    pub running: bool,
    pub engine: Option<EngineKind>,
    pub model_id: Option<String>,
    /// OpenAI-compatible base, e.g. `http://127.0.0.1:PORT/v1`.
    pub base_url: Option<String>,
}

/// One running server. Held in Tauri state behind `Mutex<Option<ServerHandle>>`.
pub struct ServerHandle {
    pub engine: EngineKind,
    /// The model id consumers pass in `POST /v1/chat/completions` — for Foundry this is the
    /// resolved device-specific id (e.g. `qwen2.5-0.5b-instruct-openvino-npu:5`), not the alias.
    pub model_id: String,
    pub port: u16,
    pub base_url: String,
    /// `None` for Foundry, whose service is OS-managed; only its loaded model is unloaded.
    /// Spawned directly (not via the shell plugin, which is scoped to the Mutagen sidecar),
    /// mirroring how `agents::start_auth` spawns `podman`.
    pub child: Option<tokio::process::Child>,
    /// The catalogue alias, retained so Foundry can `model unload <alias>` on stop.
    pub alias: String,
}

impl ServerHandle {
    pub fn status(&self) -> InferenceStatus {
        InferenceStatus {
            running: true,
            engine: Some(self.engine),
            model_id: Some(self.model_id.clone()),
            base_url: Some(self.base_url.clone()),
        }
    }
}

/// One stage of bringing a runtime online. Rust owns only the stable step `id`; the
/// human-readable title and explanation live in Paraglide message files keyed by id, so all
/// copy stays translatable. No prose lives in Rust.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetupStep {
    /// Stable key, e.g. `detect-device`; maps to Paraglide keys
    /// `runtime_step_<id>_title` / `runtime_step_<id>_explain` (with `-`→`_`).
    pub id: String,
}

impl SetupStep {
    pub fn new(id: &str) -> Self {
        SetupStep { id: id.to_string() }
    }
}

/// Streamed per setup step from `InferenceEngine::start` so the UI ticks each checklist row as
/// the runtime comes online. `detail` carries an observed fact (a device name, a port, a model
/// id) shown verbatim — it is backend data, not translatable copy.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuntimeSetupEvent {
    StepStart { id: String },
    /// Intermediate liveness for a long-running step (e.g. a Foundry download/load). `detail`
    /// is a verbatim backend fact — typically the elapsed seconds — so a slow-but-alive step is
    /// visually distinct from a hung one. Emitted repeatedly between `StepStart` and the step's
    /// terminal `StepDone`/`StepFailed`.
    StepProgress { id: String, detail: String },
    StepDone { id: String, detail: Option<String> },
    StepSkipped { id: String, reason: String },
    StepFailed { id: String, error: String },
    Ready { base_url: String, model_id: String },
}

impl RuntimeSetupEvent {
    /// Best-effort emit; a dropped progress event must never fail the start.
    pub fn send(&self, channel: &Channel<serde_json::Value>) {
        channel.send(serde_json::to_value(self).unwrap()).ok();
    }
}

#[async_trait::async_trait]
pub trait InferenceEngine: Send + Sync {
    fn kind(&self) -> EngineKind;

    /// The ordered list of step ids this engine walks through on start. Pure metadata (no side
    /// effects) so the UI can render the full plan before `start` runs the first step.
    fn setup_steps(&self) -> Vec<SetupStep>;

    /// Start serving `model_id` on `port`, emitting a `RuntimeSetupEvent` per `setup_steps`
    /// stage to `on_step`; return a handle once the health check passes.
    async fn start(
        &self,
        model_id: &str,
        port: u16,
        on_step: &Channel<serde_json::Value>,
    ) -> Result<ServerHandle, InferenceError>;

    /// Stop the server owned by `handle`. Idempotent in effect: a dead process is treated as done.
    async fn stop(&self, handle: ServerHandle) -> Result<(), InferenceError>;
}

/// The single app-wide inference server, enforcing the one-model-at-a-time decision.
pub type ActiveServer = std::sync::Arc<tokio::sync::Mutex<Option<ServerHandle>>>;

/// App-lifetime cache of the "static" (hardware/OS-constant) subset of [`MachineSpecs`], so
/// repeat `/models` mounts skip the PowerShell/winget subprocess spawns. Never persisted to disk;
/// cleared only by process restart or an explicit re-detect. `std::sync::Mutex` is correct (not
/// `tokio::sync::Mutex`) because it is only ever locked inside `spawn_blocking` closures.
pub type MachineSpecsCache = std::sync::Arc<std::sync::Mutex<Option<StaticMachineSpecs>>>;

/// The subset of [`MachineSpecs`] that does not change within a single app run.
#[derive(Clone)]
pub struct StaticMachineSpecs {
    pub cpu_cores: usize,
    pub gpu_names: Vec<String>,
    pub has_npu: bool,
    pub arch: String,
    pub foundry_installed: bool,
    pub foundry_installable: bool,
}
