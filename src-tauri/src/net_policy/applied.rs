//! The accepted policy of a sandbox, stored under the app data root.

use std::path::PathBuf;

use tauri::{AppHandle, Runtime};

use super::schema::{placeholder, NetPolicy};
use super::NetPolicyError;
use crate::mio_files;
use crate::provider::types::EgressMode;

/// The accepted policy of one sandbox: the only policy any sidecar render enforces, kept in the app
/// data root where code inside the sandbox cannot write it
/// (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 1).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPolicy {
    pub policy: NetPolicy,
    /// Counts policy changes; an apply that started from an older revision is refused
    /// (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 10).
    #[serde(default)]
    pub revision: u64,
    /// The binding variables a Podman sandbox received placeholders for at create; kept across
    /// applies, since placeholders exist only from create
    /// (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guest_env: Option<Vec<String>>,
}

/// A binding whose variable the sandbox never received, with the value to export by hand.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnstampedBinding {
    pub env_var: String,
    pub placeholder: String,
}

/// The bindings of `policy` that `applied` did not stamp into the guest at create. Empty for a
/// sandbox that records no create-time variables (one predating them).
pub fn unstamped_bindings(policy: &NetPolicy, applied: &AppliedPolicy) -> Vec<UnstampedBinding> {
    let Some(stamped) = &applied.guest_env else {
        return Vec::new();
    };
    policy
        .secrets
        .iter()
        .filter(|b| !stamped.contains(&b.env_var))
        .map(|b| UnstampedBinding {
            env_var: b.env_var.clone(),
            placeholder: placeholder(&b.env_var),
        })
        .collect()
}

fn applied_path<R: Runtime>(app: &AppHandle<R>, sandbox: &str) -> Result<PathBuf, NetPolicyError> {
    let root = crate::paths::app_data_root(app)
        .map_err(|e| NetPolicyError(format!("app data directory unavailable: {e}")))?;
    Ok(root.join("net-policy").join(sandbox).join("applied.json"))
}

pub fn write_applied<R: Runtime>(
    app: &AppHandle<R>,
    sandbox: &str,
    applied: &AppliedPolicy,
) -> Result<(), NetPolicyError> {
    Ok(mio_files::write_pretty(&applied_path(app, sandbox)?, applied)?)
}

/// The applied snapshot, or `None` for a sandbox created without one (sidecars, sandboxes
/// predating this feature).
pub fn read_applied<R: Runtime>(
    app: &AppHandle<R>,
    sandbox: &str,
) -> Result<Option<AppliedPolicy>, NetPolicyError> {
    let path = applied_path(app, sandbox)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).map(Some).map_err(|e| {
            NetPolicyError(format!("failed to parse {}: {e}", path.to_string_lossy()))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(NetPolicyError(format!("failed to read {}: {e}", path.to_string_lossy()))),
    }
}

/// The accepted policy of `sandbox`. One with no snapshot (predating this feature) is an empty
/// allowlist, so its workspace file shows as a proposal
/// (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 9).
pub fn read_accepted<R: Runtime>(app: &AppHandle<R>, sandbox: &str) -> Result<AppliedPolicy, NetPolicyError> {
    Ok(read_applied(app, sandbox)?.unwrap_or_else(|| AppliedPolicy {
        policy: empty_accepted_policy(),
        revision: 0,
        guest_env: None,
    }))
}

/// The accepted policy of a sandbox with no snapshot: an allowlist with no rules.
pub fn empty_accepted_policy() -> NetPolicy {
    NetPolicy { egress: EgressMode::Allowlist, ..NetPolicy::default() }
}

/// Best-effort: a leftover snapshot only matters if a sandbox of the same name is created again,
/// and that create overwrites it.
pub fn remove_applied<R: Runtime>(app: &AppHandle<R>, sandbox: &str) {
    if let Ok(path) = applied_path(app, sandbox) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
