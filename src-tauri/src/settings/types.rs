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
    /// Llama-specific: context window size; `None` = `DEFAULT_LLAMA_CTX_SIZE` in `inference/llama.rs`.
    pub llama_ctx_size: Option<u32>,
    /// Whether the user has finished the welcome/onboarding flow. Absent from a settings file
    /// written before the feature, so an existing install deserializes as not yet onboarded.
    pub onboarding_completed: bool,
    /// Which engine the create-sandbox flow selects by default. Absent from an existing settings
    /// file, which therefore keeps creating Podman sandboxes
    /// (`docs/features/microsandbox-provider/02_dev.md`, Phase J).
    pub default_provider: crate::provider::types::ProviderId,
}
