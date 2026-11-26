//! The GitHub token binding of the sidecar: which token to render, the refresh that carries a
//! credential change to the sidecar, and the read-only check that it is live.

use std::path::Path;

use tauri::AppHandle;

use super::apply::{apply_lock, apply_policy_locked, policy_err, write_offline};
use super::iron::rendered;
use super::secret_files::{planned_files, remove_github_file};
use super::sidecar::is_running;
use super::store::{config_dir, meta_engine, secrets_dir, ProxyStore};
use super::types::{ProxyEngine, ProxyError, SidecarSite};
use super::{iron_config, proxy_container_name};
use crate::credentials::github;
use crate::credentials::types::CredentialError;
use crate::credentials::{self, GITHUB_SERVICE};
use crate::applied_policy::read_applied;

/// The GitHub token the sidecar should swap the placeholder for, or `None` when no credential is
/// saved or the sandbox is opted out (`docs/features/proxy-credential-secrets/02_dev.md`, "When the
/// binding is rendered").
pub fn github_binding(app: &AppHandle, sandbox: &str) -> Result<Option<String>, ProxyError> {
    let token = match credentials::load_secret(app, GITHUB_SERVICE) {
        Ok(token) if !token.is_empty() => token,
        Ok(_) | Err(CredentialError::NotFound(_)) => return Ok(None),
        Err(e) => return Err(ProxyError::CommandFailed(e.to_string())),
    };
    let path = crate::paths::sandbox_github_state_path(app, sandbox)
        .map_err(|e| ProxyError::CommandFailed(e.to_string()))?;
    let state = github::load_state(&path).map_err(|e| ProxyError::CommandFailed(e.to_string()))?;
    Ok((!state.opted_out).then_some(token))
}

/// Turns a failed binding lookup into "no binding", so an unreadable GitHub state or credential
/// store never blocks the workspace policy apply and never leaves a real token rendered.
pub(super) fn fail_closed_binding(
    sandbox: &str,
    binding: Result<Option<String>, ProxyError>,
) -> Option<String> {
    binding.unwrap_or_else(|e| {
        tracing::warn!(target: "proxy", %sandbox, error = %e, "GitHub binding unavailable; rendering the policy without it");
        None
    })
}

/// What a binding refresh left in the sidecar.
#[derive(Debug, PartialEq, Eq)]
pub enum BindingState {
    /// The real token is rendered, so the placeholder gets swapped.
    Live,
    /// No token is rendered: no applied policy to render onto, no iron-proxy sidecar, or no grant.
    NotLive,
}

/// Re-renders one sandbox's GitHub binding so a credential change reaches the sidecar without a
/// policy edit; `NotLive` means no token is rendered (`docs/features/proxy-credential-secrets/02_dev.md`,
/// "Refresh paths"). A revocation always unstages the laptop's token file, even when the sidecar
/// is unreachable, so a later start never re-pushes it.
pub async fn refresh_github_binding(
    app: &AppHandle,
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
) -> Result<BindingState, ProxyError> {
    let lock = apply_lock(sandbox);
    let _guard = lock.lock().await;
    let Some(meta) = store.read_meta(sandbox).filter(|m| meta_engine(m) == ProxyEngine::IronProxy) else {
        return Ok(BindingState::NotLive);
    };
    let state = store.state_dir(sandbox)?;
    let Some(applied) = read_applied(app, sandbox).map_err(policy_err)? else {
        // Nothing to render onto, but a failed first apply may have left the token file behind.
        remove_github_file(&secrets_dir(&state))?;
        return Ok(BindingState::NotLive);
    };
    let proxy_ip = meta["proxyIp"].as_str().unwrap_or_default().to_string();
    let binding = github_binding(app, sandbox);
    let lookup_error = binding.as_ref().err().map(ToString::to_string);
    let token = fail_closed_binding(sandbox, binding);
    let values = store.load_secrets(sandbox)?;
    let config = rendered(&applied.policy, &proxy_ip, &values, token.is_some());
    if !github_files_current(&state, &config, token.as_deref()) {
        let sidecar = proxy_container_name(sandbox);
        let policy = applied.policy.clone();
        let refreshed: Result<(), ProxyError> = async {
            if is_running(&site.provider, &sidecar).await? {
                apply_policy_locked(app, store, site, sandbox, policy).await.map(drop)
            } else {
                write_offline(&state, &applied.policy, &proxy_ip, &values, token.as_deref()).map(drop)
            }
        }
        .await;
        if let Err(e) = refreshed {
            if token.is_none() {
                if let Err(offline) = write_offline(&state, &applied.policy, &proxy_ip, &values, None) {
                    tracing::error!(target: "proxy", %sandbox, error = %offline, "failed to unstage the revoked GitHub token");
                }
            }
            return Err(e);
        }
    }
    if let Some(e) = lookup_error {
        return Err(ProxyError::CommandFailed(e));
    }
    Ok(if token.is_some() { BindingState::Live } else { BindingState::NotLive })
}

/// How many secret values a deploy would send to the target: accepted-policy bindings with a stored value
/// plus the live GitHub binding. A count only, so no value or secret id crosses IPC
/// (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 2).
pub fn deploy_secret_count(app: &AppHandle, store: &ProxyStore, sandbox: &str) -> Result<usize, ProxyError> {
    let policy = read_applied(app, sandbox).map_err(policy_err)?.map(|a| a.policy).unwrap_or_default();
    let values = store.load_secrets(sandbox)?;
    let github = fail_closed_binding(sandbox, github_binding(app, sandbox)).is_some();
    Ok(planned_files(&policy, &values, github).len())
}

/// Whether `config.yaml` and the token file on disk already equal the desired render.
fn github_files_current(state: &Path, config: &serde_json::Value, token: Option<&str>) -> bool {
    let Ok(want) = serde_json::to_vec_pretty(config) else {
        return false;
    };
    let config_ok = std::fs::read(config_dir(state).join("config.yaml")).is_ok_and(|b| b == want);
    let file = std::fs::read(secrets_dir(state).join(iron_config::GITHUB_BINDING_FILE));
    let file_ok = match token {
        Some(t) => file.is_ok_and(|b| b == t.as_bytes()),
        None => file.is_err(),
    };
    config_ok && file_ok
}

/// Whether the sidecar's staged `config.yaml` and token file already carry the GitHub binding the
/// credential store and opt-out call for. Read-only: no reload, no write, so a caller about to hand
/// out the placeholder can check cheaply. False when nothing is applied, the sandbox has no grant,
/// or a refresh failed and left the files without the token; a lookup failure is an error.
pub fn github_binding_live(app: &AppHandle, store: &ProxyStore, sandbox: &str) -> Result<bool, ProxyError> {
    let Some(token) = github_binding(app, sandbox)? else {
        return Ok(false);
    };
    let Some(meta) = store.read_meta(sandbox).filter(|m| meta_engine(m) == ProxyEngine::IronProxy) else {
        return Ok(false);
    };
    let Some(applied) = read_applied(app, sandbox).map_err(policy_err)? else {
        return Ok(false);
    };
    let proxy_ip = meta["proxyIp"].as_str().unwrap_or_default();
    let values = store.load_secrets(sandbox)?;
    let config = rendered(&applied.policy, proxy_ip, &values, true);
    Ok(github_files_current(&store.state_dir(sandbox)?, &config, Some(&token)))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use mio_net_policy::schema::NetPolicy;

    /// A GitHub lookup error must drop the binding, never fail the apply or keep a token.
    #[test]
    fn binding_error_renders_without_the_token() {
        let err = Err(ProxyError::CommandFailed("corrupt state".to_string()));
        assert_eq!(fail_closed_binding("s", err), None);
        assert_eq!(fail_closed_binding("s", Ok(Some("t".to_string()))), Some("t".to_string()));
        assert_eq!(fail_closed_binding("s", Ok(None)), None);
    }

    /// The live check compares disk to the desired render, so a missing token file or a config
    /// rendered without the binding must read as not live.
    #[test]
    fn staged_files_are_current_only_with_the_binding_and_token_file() {
        let state = std::env::temp_dir().join(format!("mio-iron-test-{}", uuid::Uuid::new_v4()));
        let policy: NetPolicy = serde_json::from_str("{}").unwrap();
        let values = HashMap::new();
        write_offline(&state, &policy, "10.0.0.2", &values, Some("ghp_real")).unwrap();
        let with = rendered(&policy, "10.0.0.2", &values, true);
        let without = rendered(&policy, "10.0.0.2", &values, false);
        assert!(github_files_current(&state, &with, Some("ghp_real")));
        assert!(!github_files_current(&state, &without, None));
        std::fs::remove_file(secrets_dir(&state).join("mio-github")).unwrap();
        assert!(!github_files_current(&state, &with, Some("ghp_real")));
        std::fs::remove_dir_all(&state).unwrap();
    }
}
