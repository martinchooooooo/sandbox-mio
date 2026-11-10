//! Host-side agent configuration files. Pure filesystem merges over the workspace directory: they
//! run whether or not the sandbox's container is up.

use std::path::Path;

use crate::agents::types::AgentError;
use crate::mio_files;

/// Write/merge OpenCode provider config, preserving existing user config.
pub fn provision_opencode(
    workspace_path: &str,
    relative_path: &str,
    provider_id: String,
    provider_json: serde_json::Value,
    default_model: Option<String>,
) -> Result<(), AgentError> {
    let path = Path::new(workspace_path).join(relative_path);
    let mut obj = mio_files::read_object(&path)?;

    obj.insert(
        "$schema".into(),
        serde_json::Value::String("https://opencode.ai/config.json".into()),
    );
    if let Some(model) = default_model {
        merge_opencode_model(&mut obj, &provider_id, model);
    }
    let provider = obj
        .entry("provider")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let Some(map) = provider.as_object_mut() {
        map.insert(provider_id, provider_json);
    }

    Ok(mio_files::write_pretty(&path, &obj)?)
}

/// Sets OpenCode's `model` unless the user chose a model from another provider
/// (`docs/features/local-llm-reachability/02_dev.md`, Resolved Decision 4).
fn merge_opencode_model(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    provider_id: &str,
    model: String,
) {
    let ours = match obj.get("model") {
        None => true,
        Some(serde_json::Value::String(current)) => current.starts_with(&format!("{provider_id}/")),
        Some(_) => false,
    };
    if ours {
        obj.insert("model".into(), serde_json::Value::String(model));
    }
}

/// Sets pi's `defaultProvider` and `defaultModel` unless the user chose another provider
/// (`docs/features/local-llm-reachability/02_dev.md`, Resolved Decision 4).
fn merge_pi_defaults(
    settings: &mut serde_json::Map<String, serde_json::Value>,
    provider: String,
    model: String,
) {
    let ours = match settings.get("defaultProvider") {
        None => true,
        Some(serde_json::Value::String(current)) => *current == provider,
        Some(_) => false,
    };
    if ours {
        settings.insert("defaultProvider".into(), serde_json::Value::String(provider));
        settings.insert("defaultModel".into(), serde_json::Value::String(model));
    }
}

/// Write pi's local-llm provider config, its default provider and model in `settings.json` beside
/// `relative_path`, and the `mise.toml` env redirect that points pi at both.
#[allow(clippy::too_many_arguments)]
pub fn provision_pi(
    workspace_path: &str,
    relative_path: &str,
    provider_id: String,
    provider_json: serde_json::Value,
    default_provider: Option<String>,
    default_model: Option<String>,
    config_env: String,
    config_dir_container: String,
) -> Result<(), AgentError> {
    let models_path = Path::new(workspace_path).join(relative_path);
    let mut models = mio_files::read_object(&models_path)?;
    let providers = models
        .entry("providers")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let Some(map) = providers.as_object_mut() {
        map.insert(provider_id, provider_json);
    }
    mio_files::write_pretty(&models_path, &models)?;

    if let (Some(provider), Some(model)) = (default_provider, default_model) {
        let settings_path = models_path.with_file_name("settings.json");
        let mut settings = mio_files::read_object(&settings_path)?;
        merge_pi_defaults(&mut settings, provider, model);
        mio_files::write_pretty(&settings_path, &settings)?;
    }

    // mise.toml is TOML rather than JSON, so it stays hand-rolled.
    let manifest_path = Path::new(workspace_path).join("mise.toml");
    let manifest_src = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "[tools]\n".to_string(),
        Err(e) => {
            return Err(AgentError::ConfigWriteFailed(format!("failed to read {}: {e}", manifest_path.to_string_lossy())))
        }
    };
    let mut manifest: toml::Table = manifest_src
        .parse()
        .map_err(|e| AgentError::ConfigWriteFailed(format!("failed to parse {}: {e}", manifest_path.to_string_lossy())))?;
    let env = manifest
        .entry("env")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let Some(table) = env.as_table_mut() {
        table.insert(config_env, toml::Value::String(config_dir_container));
    }
    let manifest_out = toml::to_string(&manifest)
        .map_err(|e| AgentError::ConfigWriteFailed(e.to_string()))?;
    Ok(mio_files::write_text(&manifest_path, &manifest_out)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn opencode_model_written_when_absent() {
        let mut o = obj(serde_json::json!({ "theme": "dark" }));
        merge_opencode_model(&mut o, "local-llm", "local-llm/qwen3.5-2b".into());
        assert_eq!(o["model"], "local-llm/qwen3.5-2b");
        assert_eq!(o["theme"], "dark");
    }

    #[test]
    fn opencode_model_follows_server_when_app_written() {
        let mut o = obj(serde_json::json!({ "model": "local-llm/qwen3.5-4b" }));
        merge_opencode_model(&mut o, "local-llm", "local-llm/qwen3.5-2b".into());
        assert_eq!(o["model"], "local-llm/qwen3.5-2b");
    }

    #[test]
    fn opencode_model_kept_when_user_chose_another_provider() {
        let mut o = obj(serde_json::json!({ "model": "anthropic/claude-sonnet-5-5" }));
        merge_opencode_model(&mut o, "local-llm", "local-llm/qwen3.5-2b".into());
        assert_eq!(o["model"], "anthropic/claude-sonnet-5-5");
    }

    #[test]
    fn opencode_model_prefix_match_needs_the_separator() {
        let mut o = obj(serde_json::json!({ "model": "local-llm-cloud/x" }));
        merge_opencode_model(&mut o, "local-llm", "local-llm/qwen3.5-2b".into());
        assert_eq!(o["model"], "local-llm-cloud/x");
    }

    #[test]
    fn pi_defaults_written_when_absent() {
        let mut s = obj(serde_json::json!({ "theme": "dark" }));
        merge_pi_defaults(&mut s, "local-llm".into(), "qwen3.5-2b".into());
        assert_eq!(s["defaultProvider"], "local-llm");
        assert_eq!(s["defaultModel"], "qwen3.5-2b");
        assert_eq!(s["theme"], "dark");
    }

    #[test]
    fn pi_defaults_follow_server_when_app_written() {
        let mut s = obj(serde_json::json!({ "defaultProvider": "local-llm", "defaultModel": "qwen3.5-4b" }));
        merge_pi_defaults(&mut s, "local-llm".into(), "qwen3.5-2b".into());
        assert_eq!(s["defaultModel"], "qwen3.5-2b");
    }

    #[test]
    fn pi_defaults_kept_when_user_chose_another_provider() {
        let mut s = obj(serde_json::json!({ "defaultProvider": "anthropic", "defaultModel": "claude-sonnet-5-5" }));
        merge_pi_defaults(&mut s, "local-llm".into(), "qwen3.5-2b".into());
        assert_eq!(s["defaultProvider"], "anthropic");
        assert_eq!(s["defaultModel"], "claude-sonnet-5-5");
    }
}
