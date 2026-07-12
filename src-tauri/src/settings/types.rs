use crate::inference::types::EngineKind;

/// Errors loading or saving app settings.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings io error: {0}")]
    Io(String),
    #[error("settings parse error: {0}")]
    Parse(String),
}

/// App-global settings persisted at `<app_config>/settings.json`. Only app-wide state besides
/// workspace-scoped `.mio/` files.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// Which local inference engine to use.
    pub inference_engine: EngineKind,
    /// Llama-specific: GPU layer offload count; `None` = auto-detect.
    pub llama_gpu_layers: Option<u32>,
    /// Llama-specific: context window size; `None` = model default.
    pub llama_ctx_size: Option<u32>,
}
