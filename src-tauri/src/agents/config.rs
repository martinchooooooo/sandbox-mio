//! Host-side agent configuration files. Pure filesystem merges over the workspace directory: they
//! run whether or not the sandbox's container is up.
//!
//! Errors stay `BackendError` because the two commands wrapping these functions already surface as
//! IPC kind `Backend`, and the decomposition freezes the IPC surface
//! (`docs/features/lib-decomposition/02_dev.md`, Resolved Decision 6).

use std::path::Path;

use crate::backends::types::BackendError;
use crate::mio_files;

/// Write/merge OpenCode provider config, preserving existing user config.
pub fn provision_opencode(
    workspace_path: &str,
    relative_path: &str,
    provider_id: String,
    provider_json: serde_json::Value,
    default_model: Option<String>,
) -> Result<(), BackendError> {
    let path = Path::new(workspace_path).join(relative_path);
    let mut obj = mio_files::read_object(&path)?;

    obj.insert(
        "$schema".into(),
        serde_json::Value::String("https://opencode.ai/config.json".into()),
    );
    let provider = obj
        .entry("provider")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let Some(map) = provider.as_object_mut() {
        map.insert(provider_id, provider_json);
    }
    if let Some(model) = default_model {
        obj.entry("model").or_insert(serde_json::Value::String(model));
    }

    Ok(mio_files::write_pretty(&path, &obj)?)
}

/// Write pi's local-llm provider config and the `mise.toml` env redirect that points pi at it.
pub fn provision_pi(
    workspace_path: &str,
    relative_path: &str,
    provider_id: String,
    provider_json: serde_json::Value,
    config_env: String,
    config_dir_container: String,
) -> Result<(), BackendError> {
    let models_path = Path::new(workspace_path).join(relative_path);
    let mut models = mio_files::read_object(&models_path)?;
    let providers = models
        .entry("providers")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let Some(map) = providers.as_object_mut() {
        map.insert(provider_id, provider_json);
    }
    mio_files::write_pretty(&models_path, &models)?;

    // mise.toml is TOML rather than JSON, so it stays hand-rolled.
    let manifest_path = Path::new(workspace_path).join("mise.toml");
    let manifest_src = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "[tools]\n".to_string(),
        Err(e) => {
            return Err(BackendError::CommandFailed(format!("failed to read {}: {e}", manifest_path.to_string_lossy())))
        }
    };
    let mut manifest: toml::Table = manifest_src
        .parse()
        .map_err(|e| BackendError::CommandFailed(format!("failed to parse {}: {e}", manifest_path.to_string_lossy())))?;
    let env = manifest
        .entry("env")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let Some(table) = env.as_table_mut() {
        table.insert(config_env, toml::Value::String(config_dir_container));
    }
    let manifest_out = toml::to_string(&manifest)
        .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
    Ok(mio_files::write_text(&manifest_path, &manifest_out)?)
}
