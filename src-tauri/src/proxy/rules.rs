//! Secret-injection rule definitions and their materialisation into the proxy container's own
//! `rules.json`. Rule *definitions* (`.mio/proxy-rules.json`) carry a secret *id* only, since the
//! workspace is bind-mounted into the sandbox; the *materialised* copy with real values is written
//! only under `<app_data_root>/proxy/<sandbox>/` and mounted only into the proxy container
//! (`docs/features/network-proxy/02_dev.md`, Resolved Decision 4).

use tauri::AppHandle;

use crate::mio_files;

use super::types::ProxyError;

const RULES_FILE: &str = "proxy-rules.json";

/// One secret-injection rule: match outbound requests to `host` (and, if set, a path prefix), then
/// set `header` from the credential named `secret_id`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRule {
    pub host: String,
    #[serde(default)]
    pub path_prefix: String,
    pub header: String,
    pub secret_id: String,
}

/// Reads `.mio/proxy-rules.json`, treating an absent file as no rules.
pub fn load_rules(workspace_path: &str) -> Result<Vec<ProxyRule>, ProxyError> {
    let path = mio_files::mio_path(workspace_path, RULES_FILE);
    match mio_files::read_value(&path) {
        Some(v) => serde_json::from_value(v).map_err(|e| {
            ProxyError::CommandFailed(format!("failed to parse {}: {e}", path.to_string_lossy()))
        }),
        None => Ok(Vec::new()),
    }
}

/// Overwrites `.mio/proxy-rules.json` with `rules`.
pub fn save_rules(workspace_path: &str, rules: &[ProxyRule]) -> Result<(), ProxyError> {
    let path = mio_files::mio_path(workspace_path, RULES_FILE);
    mio_files::write_pretty(&path, &rules).map_err(|e| ProxyError::CommandFailed(e.to_string()))
}

/// Resolves each rule's `secret_id` against `<app_data_root>/proxy/secrets.json` into the
/// proxy-side JSON the addon reads. A rule whose secret has no stored value is dropped rather than
/// materialised with a placeholder — the addon then leaves that header untouched, same as no rule
/// existing. Header names are lowercased: HTTP/2 rejects uppercase names
/// (`docs/features/network-proxy/02_dev.md`, Common Gotchas).
pub fn materialise(app: &AppHandle, rules: &[ProxyRule]) -> Result<serde_json::Value, ProxyError> {
    let secrets = super::load_secrets(app)?;
    let entries: Vec<serde_json::Value> = rules
        .iter()
        .filter_map(|r| {
            let value = secrets.get(&r.secret_id)?;
            Some(serde_json::json!({
                "host": r.host,
                "path_prefix": r.path_prefix,
                "header": r.header.to_lowercase(),
                "value": value,
            }))
        })
        .collect();
    Ok(serde_json::Value::Array(entries))
}
