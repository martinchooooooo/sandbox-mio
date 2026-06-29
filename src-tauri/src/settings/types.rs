use crate::inference::types::EngineKind;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings io error: {0}")]
    Io(String),
    #[error("settings parse error: {0}")]
    Parse(String),
}

/// App-global settings, persisted as JSON at `<app_config>/settings.json`. The first persisted
/// app-wide state (workspace-scoped `.mio/` files are the only other persistence).
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub inference_engine: EngineKind,
    /// Advanced: offload N layers to the GPU; `None` = auto (every layer that fits).
    pub llama_gpu_layers: Option<u32>,
    /// Advanced: context window size; `None` = the model's default.
    pub llama_ctx_size: Option<u32>,
}
