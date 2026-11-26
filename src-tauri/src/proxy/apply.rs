//! The apply pipeline: makes the sidecar run an accepted policy, locally or on the deploy target,
//! restoring the previous files and config when it fails.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use tauri::AppHandle;

use super::github::{fail_closed_binding, github_binding};
use super::iron::{app_bindings, rendered, write_config};
use super::remote_push::{
    push_secret_files, push_swap_reload, remove_stale_remote_files,
};
use super::secret_files::{
    remove_stale_secret_files, restore_secret_files, snapshot_secret_files, stage_files,
};
use super::sidecar::{ensure_sidecar_running, is_running, reload, start_and_confirm};
use super::store::{config_dir, meta_engine, secrets_dir, ProxyStore};
use super::types::{ProxyEngine, ProxyError, SidecarSite};
use super::{io_err, iron_config, proxy_container_name};
use crate::applied_policy::{
    empty_accepted_policy, read_accepted, read_applied, write_applied, AppliedPolicy,
};
use mio_net_policy::schema::NetPolicy;
use mio_net_policy::validate::validate;
use mio_net_policy::NetPolicyError;

/// What an apply could not do without failing it: bindings left out because their secret has no
/// stored value, so their placeholder passes through unswapped (Resolved Decision 15).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    /// The `envVar` of each binding without a stored value.
    pub missing_secrets: Vec<String>,
}

/// A secret-value change that rides on a policy apply. Deliberately not `Debug`, so a value cannot
/// reach a log.
#[derive(Default)]
pub struct ValueEdit {
    /// `(secret id, value)`, stored before the apply and restored if it fails.
    pub store: Option<(String, String)>,
    /// Removed once the apply succeeded.
    pub drop: Option<String>,
}

pub(super) fn policy_err(e: NetPolicyError) -> ProxyError {
    ProxyError::CommandFailed(e.0)
}

/// Makes the sidecar on `site` run `policy`, already validated; remote sites apply live too. A failure
/// restores the previous files and values. A revision other than `expected_revision` is refused
/// (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 10).
pub async fn apply_policy(
    app: &AppHandle,
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
    policy: NetPolicy,
    expected_revision: u64,
    edit: ValueEdit,
) -> Result<ApplyReport, ProxyError> {
    let lock = apply_lock(sandbox);
    let _guard = lock.lock().await;
    let current = read_accepted(app, sandbox).map_err(policy_err)?.revision;
    if current != expected_revision {
        return Err(ProxyError::CommandFailed(format!(
            "the network policy of {sandbox:?} changed since the editor loaded it; reload the network section and redo the edit"
        )));
    }
    // Stored only now, under the lock and after the revision check, so a refused or failed apply
    // never leaves a value behind that a later refresh would put into a secret file.
    let before = match &edit.store {
        Some((id, value)) => {
            let before = store.load_secrets(sandbox)?;
            store.set_secret(sandbox, id, value)?;
            Some(before)
        }
        None => None,
    };
    let result = apply_policy_locked(app, store, site, sandbox, policy).await;
    match (&result, before) {
        (Err(_), Some(before)) => {
            if let Err(e) = store.save_secrets(sandbox, &before) {
                tracing::error!(target: "proxy", %sandbox, error = %e, "failed to restore the previous secret values");
            }
        }
        (Ok(_), _) => {
            if let Some(id) = &edit.drop {
                if let Err(e) = store.delete_secret(sandbox, id) {
                    tracing::warn!(target: "proxy", %sandbox, error = %e, "applied, but failed to delete the dropped secret value");
                }
            }
        }
        (Err(_), None) => {}
    }
    result
}

/// Applies the accepted policy as it stands under the apply lock, so an editor apply landing in
/// between is never reverted. Used by create and deploy.
pub async fn apply_accepted(
    app: &AppHandle,
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
) -> Result<ApplyReport, ProxyError> {
    let lock = apply_lock(sandbox);
    let _guard = lock.lock().await;
    let policy = read_accepted(app, sandbox).map_err(policy_err)?.policy;
    validate(&policy, site.provider.capabilities()).map_err(policy_err)?;
    apply_policy_locked(app, store, site, sandbox, policy).await
}

/// One lock per sandbox name. Every apply shares one `config.prev.yaml` and one secrets directory,
/// so two overlapping applies (two windows, or a Save racing a secret-value save) would back up each
/// other's unconfirmed config and delete each other's secret files.
pub(super) fn apply_lock(sandbox: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        LazyLock::new(Default::default);
    LOCKS.lock().unwrap().entry(sandbox.to_string()).or_default().clone()
}

pub(super) async fn apply_policy_locked(
    app: &AppHandle,
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
    policy: NetPolicy,
) -> Result<ApplyReport, ProxyError> {
    if site.local {
        apply_local_locked(app, store, site, sandbox, policy).await
    } else if store.read_remote_meta(sandbox).is_some() {
        apply_remote_locked(app, store, site, sandbox, policy).await
    } else {
        Err(ProxyError::CommandFailed(format!(
            "{sandbox:?} is deployed without a remote proxy; undeploy it and recreate the sandbox"
        )))
    }
}

/// The iron-proxy `meta.json` of `sandbox`'s local sidecar, or the error naming the missing sidecar.
fn local_meta(store: &ProxyStore, sandbox: &str) -> Result<serde_json::Value, ProxyError> {
    store
        .read_meta(sandbox)
        .filter(|m| meta_engine(m) == ProxyEngine::IronProxy)
        .ok_or_else(|| {
            ProxyError::CommandFailed(format!(
                "{sandbox:?} has no iron-proxy sidecar; recreate the sandbox to get the network boundary"
            ))
        })
}

async fn apply_local_locked(
    app: &AppHandle,
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
    policy: NetPolicy,
) -> Result<ApplyReport, ProxyError> {
    let state = store.state_dir(sandbox)?;
    let meta = local_meta(store, sandbox)?;
    let proxy_ip = meta["proxyIp"].as_str().unwrap_or_default().to_string();
    let provider = &site.provider;
    // Read before anything changes: the rewrite below must carry `guest_env` forward, and losing it
    // would hide every unstamped binding.
    let guest_env = read_applied(app, sandbox).map_err(policy_err)?.and_then(|a| a.guest_env);

    let secrets = secrets_dir(&state);
    let values = store.load_secrets(sandbox)?;
    let github_token = fail_closed_binding(sandbox, github_binding(app, sandbox));
    let snapshot = snapshot_secret_files(&secrets)?;
    // Every failure from here on puts the secret files back, so a value from a policy that never ran
    // neither stays on disk nor reaches the pipeline at its next restart.
    let skip_github = github_token.is_none();
    let fail = |e: ProxyError| {
        restore_secret_files(&secrets, &snapshot, skip_github);
        e
    };
    let (written, missing_secrets) =
        stage_files(&secrets, &policy, &values, github_token.as_deref()).map_err(fail)?;
    let dir = config_dir(&state);
    let current = dir.join("config.yaml");
    let prev = dir.join("config.prev.yaml");
    std::fs::copy(&current, &prev).map_err(|e| fail(io_err(&current, e)))?;
    let bindings = app_bindings(github_token.is_some());
    write_config(&current, &iron_config::render(&policy, &proxy_ip, &written, &bindings)).map_err(fail)?;

    let sidecar = proxy_container_name(sandbox);
    let outcome = if is_running(provider, &sidecar).await? {
        reload(provider, &sidecar).await
    } else {
        start_and_confirm(provider, &sidecar).await
    };
    if let Err(e) = outcome {
        if let Err(restore) = std::fs::rename(&prev, &current) {
            tracing::error!(target: "proxy", %sandbox, error = %restore, "failed to restore the previous proxy config");
        }
        return Err(fail(e));
    }
    remove_stale_secret_files(&secrets, &written);
    finish_apply(app, sandbox, policy, guest_env, missing_secrets)
}

/// A first apply that differs from the implicit empty allowlist moves to revision 1, so two editors
/// loaded at revision 0 cannot both apply.
fn next_revision(prev: Option<&AppliedPolicy>, policy: &NetPolicy) -> u64 {
    match prev {
        Some(p) if p.policy == *policy => p.revision,
        Some(p) => p.revision + 1,
        None if *policy == empty_accepted_policy() => 0,
        None => 1,
    }
}

/// Records the applied snapshot once the new policy runs, and reports what had no stored value.
fn finish_apply(
    app: &AppHandle,
    sandbox: &str,
    policy: NetPolicy,
    guest_env: Option<Vec<String>>,
    missing_secrets: Vec<String>,
) -> Result<ApplyReport, ProxyError> {
    // The revision moves only when the accepted policy does, so a background re-render never
    // invalidates an open editor.
    let prev = read_applied(app, sandbox).map_err(policy_err)?;
    let revision = next_revision(prev.as_ref(), &policy);
    let applied = AppliedPolicy { policy, revision, guest_env };
    write_applied(app, sandbox, &applied).map_err(policy_err)?;
    if !missing_secrets.is_empty() {
        tracing::warn!(target: "proxy", %sandbox, ?missing_secrets, "bindings without a stored secret were not applied");
    }
    Ok(ApplyReport { missing_secrets })
}

/// The apply of a deployed sandbox: renders both configs, pushes the secret files, swaps the remote
/// config in and reloads. A failure restores the laptop files and the remote sidecar, so
/// `applied.json` always describes what it runs (`docs/features/proxy-remote-deploy/02_dev.md`,
/// Resolved Decisions 4, 14).
async fn apply_remote_locked(
    app: &AppHandle,
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
    policy: NetPolicy,
) -> Result<ApplyReport, ProxyError> {
    let state = store.state_dir(sandbox)?;
    let local_ip = local_meta(store, sandbox)?["proxyIp"].as_str().unwrap_or_default().to_string();
    let remote_ip = store
        .read_remote_meta(sandbox)
        .and_then(|m| m["proxyIp"].as_str().map(str::to_string))
        .unwrap_or_default();
    let sidecar = proxy_container_name(sandbox);
    // A stopped remote sidecar (host reboot) is started with its last-applied files first; an
    // unreachable target fails here, before anything on the laptop changed.
    ensure_sidecar_running(store, site, sandbox).await?;
    let guest_env = read_applied(app, sandbox).map_err(policy_err)?.and_then(|a| a.guest_env);

    let secrets = secrets_dir(&state);
    let values = store.load_secrets(sandbox)?;
    let github_token = fail_closed_binding(sandbox, github_binding(app, sandbox));
    let snapshot = snapshot_secret_files(&secrets)?;
    let snapshot_names: Vec<String> = snapshot
        .iter()
        .filter_map(|(p, _)| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();
    let skip_github = github_token.is_none();
    let (written, missing_secrets) = stage_files(&secrets, &policy, &values, github_token.as_deref())
        .map_err(|e| {
            restore_secret_files(&secrets, &snapshot, skip_github);
            e
        })?;

    let bindings = app_bindings(github_token.is_some());
    let remote_dir = config_dir(&store.remote_dir(sandbox)?);
    let sites = [(config_dir(&state), &local_ip), (remote_dir.clone(), &remote_ip)];
    // A stale `config.prev.yaml` from an earlier apply must never be restored, so only sites backed
    // up in this apply count.
    let backed_up = Mutex::new(Vec::<PathBuf>::new());
    let restore_configs = || {
        for dir in backed_up.lock().unwrap().iter() {
            if let Err(e) = std::fs::rename(dir.join("config.prev.yaml"), dir.join("config.yaml")) {
                tracing::error!(target: "proxy", %sandbox, error = %e, "failed to restore a proxy config");
            }
        }
    };
    let rendered_files: Result<(), ProxyError> = sites.iter().try_for_each(|(dir, ip)| {
        std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
        let current = dir.join("config.yaml");
        std::fs::copy(&current, dir.join("config.prev.yaml")).map_err(|e| io_err(&current, e))?;
        backed_up.lock().unwrap().push(dir.clone());
        write_config(&current, &iron_config::render(&policy, ip, &written, &bindings))
    });
    if let Err(e) = rendered_files {
        restore_configs();
        restore_secret_files(&secrets, &snapshot, skip_github);
        return Err(e);
    }

    let outcome = push_swap_reload(site, &sidecar, &secrets, &written, &remote_dir.join("config.yaml")).await;
    if let Err(e) = outcome {
        restore_configs();
        restore_secret_files(&secrets, &snapshot, skip_github);
        // Best effort: put the remote tmpfs back to the restored files.
        let repushed = push_secret_files(site, &sidecar, &secrets, &snapshot_names).await;
        let cleaned = remove_stale_remote_files(site, &sidecar, &snapshot_names).await;
        if let Some(err) = repushed.err().or(cleaned.err()) {
            tracing::error!(target: "proxy", %sandbox, error = %err, "failed to restore the remote secret files");
        }
        return Err(e);
    }
    remove_stale_secret_files(&secrets, &written);
    if let Err(e) = remove_stale_remote_files(site, &sidecar, &written).await {
        tracing::warn!(target: "proxy", %sandbox, error = %e, "failed to remove stale remote secret files");
    }
    finish_apply(app, sandbox, policy, guest_env, missing_secrets)
}

/// Writes the secret files and `config.yaml` of a stopped sidecar, so it boots with the current
/// state. Nothing runs, so a write failure restores the secret files and leaves the old config.
/// Returns the variables left out for want of a stored value.
pub(super) fn write_offline(
    state: &Path,
    policy: &NetPolicy,
    proxy_ip: &str,
    values: &HashMap<String, String>,
    token: Option<&str>,
) -> Result<Vec<String>, ProxyError> {
    let secrets = secrets_dir(state);
    let snapshot = snapshot_secret_files(&secrets)?;
    let skip_github = token.is_none();
    let fail = |e: ProxyError| {
        restore_secret_files(&secrets, &snapshot, skip_github);
        e
    };
    let (written, missing) = stage_files(&secrets, policy, values, token).map_err(fail)?;
    let config = rendered(policy, proxy_ip, values, token.is_some());
    let dir = config_dir(state);
    std::fs::create_dir_all(&dir).map_err(|e| fail(io_err(&dir, e)))?;
    write_config(&dir.join("config.yaml"), &config).map_err(fail)?;
    remove_stale_secret_files(&secrets, &written);
    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_apply_of_a_changed_policy_moves_past_revision_zero() {
        let empty = empty_accepted_policy();
        let open = NetPolicy::default();
        let applied = |policy: &NetPolicy, revision| AppliedPolicy {
            policy: policy.clone(),
            revision,
            guest_env: None,
        };
        assert_eq!(next_revision(None, &empty), 0, "a re-render of the implicit default keeps revision 0");
        assert_eq!(next_revision(None, &open), 1);
        assert_eq!(next_revision(Some(&applied(&open, 4)), &open), 4);
        assert_eq!(next_revision(Some(&applied(&open, 4)), &empty), 5);
    }
}
