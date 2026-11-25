//! Applies the saved GitHub token to local sandboxes' `gh` and removes it again, driven by one
//! reconcile per sandbox (`docs/features/github-cli-auth/02_dev.md`). Failures never fail the
//! action that triggered a reconcile; they are recorded per sandbox and warn-logged.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::backends::types::SandboxBackend;
use crate::credentials::github::{self, ApplyOutcome, GithubSandboxState};
use crate::credentials::types::CredentialError;
use crate::credentials::{self, GITHUB_SERVICE};
use crate::error::AppError;
use crate::provider::ProviderRegistry;
use crate::proxy::iron::BindingState;
use crate::sync::types::SyncEngine;

/// Serialises every reconcile, so an apply-all after a save cannot race a start hook on the same
/// sandbox (`docs/features/github-cli-auth/02_dev.md`, Resolved Decision 11).
static RECONCILE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Emitted after every reconcile so the Manage card reloads its status. Payload is `{ name }`;
/// mirrored in `src/lib/workspace/GithubAccess.svelte`.
const CHANGED_EVENT: &str = "github-access://changed";

#[derive(Clone, serde::Serialize)]
struct Changed<'a> {
    name: &'a str,
}

/// What a reconcile does to one sandbox.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    None,
    /// Apply the credential captured at this stamp.
    Apply(u64),
    Remove,
    /// Drop a stale outcome that left no token behind, without touching the sandbox.
    Forget,
}

/// Decides the action from the desired state alone. `credential_captured_at_ms` is `None` when no
/// GitHub credential is saved.
/// The placeholder gh holds is fixed, so a rotation (new `captured_at_ms`) needs only the binding
/// refresh and never a re-apply.
fn plan(credential_captured_at_ms: Option<u64>, state: &GithubSandboxState) -> Action {
    let applied = matches!(state.outcome, Some(ApplyOutcome::Applied { .. }));
    // A failed apply can sit over a token gh already wrote (login ok, setup-git failed, or a timeout).
    let may_hold_token =
        matches!(state.outcome, Some(ApplyOutcome::Applied { .. } | ApplyOutcome::Failed { .. }));
    match credential_captured_at_ms {
        Some(captured) if !state.opted_out => {
            if applied {
                Action::None
            } else {
                Action::Apply(captured)
            }
        }
        _ if may_hold_token => Action::Remove,
        _ if state.outcome.is_some() => Action::Forget,
        _ => Action::None,
    }
}

/// Lets gh run with the placeholder only when the sidecar holds the real token, and otherwise
/// gives the Failed reason. Unswapped, GitHub would reject the placeholder with a misleading error.
fn binding_gate(refreshed: Result<BindingState, String>) -> Result<(), String> {
    match refreshed {
        Ok(BindingState::Live) => Ok(()),
        Ok(BindingState::NotLive) => Err("network policy not applied yet".to_string()),
        Err(e) => Err(e.chars().take(github::MAX_REASON_CHARS).collect()),
    }
}

/// Whether the desired state grants GitHub access, which is when the sidecar holds the real token.
fn wants_access(credential_captured_at_ms: Option<u64>, state: &GithubSandboxState) -> bool {
    credential_captured_at_ms.is_some() && !state.opted_out
}

/// The site of `name`'s active sidecar, resolved from its route.
async fn locate_site(app: &AppHandle, name: &str) -> Result<crate::proxy::types::SidecarSite, AppError> {
    let router = app.state::<super::routing::SandboxRouter>();
    super::proxy::locate_sidecar(&router, name).await
}

/// Whether `name` has a Mutagen deploy session, which puts it out of scope. An error is not
/// treated as "local", so a deployed sandbox never receives the token on a Mutagen failure.
async fn is_deployed(app: &AppHandle, name: &str) -> Result<bool, AppError> {
    let engine = app.state::<Arc<dyn SyncEngine>>().inner().clone();
    Ok(engine.sessions().await?.iter().any(|s| s.name == name))
}

/// The saved credential's capture stamp, or `None` when no token is saved.
fn credential_stamp(app: &AppHandle) -> Result<Option<u64>, CredentialError> {
    match credentials::load_manifest(app, GITHUB_SERVICE) {
        Ok(manifest) => Ok(Some(manifest.captured_at_ms)),
        Err(CredentialError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

fn state_path(app: &AppHandle, name: &str) -> Result<std::path::PathBuf, AppError> {
    crate::domain::validate_name(name)?;
    crate::paths::sandbox_github_state_path(app, name).map_err(|e| CredentialError::Io(e.to_string()).into())
}

/// Brings sandbox `name` in line with the saved credential and its opt-out, running at most one
/// in-sandbox script. A sandbox that is not local and running keeps its state unchanged.
pub async fn reconcile_sandbox(app: &AppHandle, name: &str) -> Result<GithubSandboxState, AppError> {
    reconcile_with(app, name, None).await
}

/// [`reconcile_sandbox`], first recording a new opt-out when `opted_out` is given. The recorded
/// opt-out is saved even while the sandbox is stopped.
async fn reconcile_with(
    app: &AppHandle,
    name: &str,
    opted_out: Option<bool>,
) -> Result<GithubSandboxState, AppError> {
    let result = {
        let _guard = RECONCILE_LOCK.lock().await;
        reconcile_locked(app, name, opted_out).await
    };
    if let Err(e) = app.emit(CHANGED_EVENT, Changed { name }) {
        tracing::warn!(target: "credentials", sandbox = %name, error = %e, "could not emit the GitHub access change");
    }
    result
}

/// Deletes the state file of a destroyed sandbox. Takes the reconcile lock, so an in-flight
/// reconcile cannot recreate the file afterwards for a same-name sandbox.
pub async fn forget(app: &AppHandle, name: &str) {
    let _guard = RECONCILE_LOCK.lock().await;
    let path = match state_path(app, name) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!(target: "destroy", %name, error = %e, "could not resolve the GitHub access state file");
            return;
        }
    };
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(target: "destroy", %name, error = %e, "could not remove the GitHub access state"),
    }
}

async fn reconcile_locked(
    app: &AppHandle,
    name: &str,
    opted_out: Option<bool>,
) -> Result<GithubSandboxState, AppError> {
    let path = state_path(app, name)?;
    let mut state = github::load_state(&path)?;
    // The one binding refresh of this pass; the Apply gate and the drop-log reuse it.
    let mut refreshed: Option<Result<BindingState, String>> = None;
    if let Some(opted_out) = opted_out {
        state.opted_out = opted_out;
        if !opted_out && !matches!(state.outcome, Some(ApplyOutcome::Applied { .. })) {
            state.outcome = None;
        }
        github::save_state(&path, &state)?;
        // A stopped sidecar boots from its files on disk, so an opt-out must strip the token from them
        // now. This is a security action: a failure is returned, and the saved opt-out makes a retry
        // (the same call) re-run the refresh.
        if opted_out {
            let site = locate_site(app, name).await?;
            refreshed = Some(Ok(crate::proxy::iron::refresh_github_binding(app, &site, name).await?));
        }
    }

    let store = app.state::<Arc<dyn SandboxBackend>>().inner().clone();
    let running = super::sandbox::list_user(&store)
        .await?
        .iter()
        .any(|s| s.name == name && s.status == "running");
    if !running {
        return Ok(state);
    }
    // Stopped sidecars are refreshed by `spawn_reconcile_all` and the opt-out above. A running one is
    // refreshed here even when gh needs no action, which is how a rotation reaches the sidecar.
    if refreshed.is_none() {
        let site = locate_site(app, name).await?;
        refreshed = Some(crate::proxy::iron::refresh_github_binding(app, &site, name).await.map_err(|e| e.to_string()));
    }
    if is_deployed(app, name).await? {
        return Ok(state);
    }

    let stamp = credential_stamp(app)?;
    // A failed drop is only logged: the public placeholder stays valid for as long as the binding
    // does, even when gh holds no login (outcome None or NeedsImage).
    if let (Some(Err(e)), false) = (&refreshed, wants_access(stamp, &state)) {
        tracing::warn!(target: "credentials", sandbox = %name, error = %e, "could not drop the GitHub binding from the sidecar");
    }
    match plan(stamp, &state) {
        Action::None => return Ok(state),
        Action::Forget => state.outcome = None,
        Action::Apply(captured_at_ms) => {
            let registry = app.state::<Arc<ProviderRegistry>>().inner().clone();
            let provider = registry.engine_of(name).await;
            // The binding must be live before gh validates the placeholder online.
            let outcome = match binding_gate(refreshed.unwrap_or(Ok(BindingState::NotLive))) {
                Ok(()) => github::apply(&provider, name, captured_at_ms).await,
                Err(reason) => ApplyOutcome::Failed { reason, captured_at_ms },
            };
            state.outcome = Some(outcome);
        }
        Action::Remove => {
            let registry = app.state::<Arc<ProviderRegistry>>().inner().clone();
            let provider = registry.engine_of(name).await;
            // The outcome is kept on failure, so the next reconcile retries the removal.
            match github::remove(&provider, name).await {
                Ok(()) => state.outcome = None,
                Err(e) => {
                    tracing::warn!(target: "credentials", sandbox = %name, error = %e, "could not remove the GitHub login");
                }
            }
        }
    }
    github::save_state(&path, &state)?;
    Ok(state)
}

/// Records the Manage switch and reconciles `name` now, returning the resulting state. Calling it
/// with the current value re-runs the reconcile, which is the retry for a failed apply.
pub async fn set_access(app: &AppHandle, name: &str, enabled: bool) -> Result<GithubSandboxState, AppError> {
    reconcile_with(app, name, Some(!enabled)).await
}

/// [`reconcile_sandbox`] off the caller's path, warn-logging a failure.
pub fn spawn_reconcile(app: &AppHandle, name: &str) {
    let app = app.clone();
    let name = name.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = reconcile_sandbox(&app, &name).await {
            tracing::warn!(target: "credentials", sandbox = %name, error = %e, "GitHub access reconcile failed");
        }
    });
}

/// Refreshes the GitHub binding of every stopped sandbox, then reconciles the running ones, which
/// refresh their own, off the caller's path.
pub fn spawn_reconcile_all(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let store = app.state::<Arc<dyn SandboxBackend>>().inner().clone();
        let sandboxes = match super::sandbox::list_user(&store).await {
            Ok(sandboxes) => sandboxes,
            Err(e) => {
                tracing::warn!(target: "credentials", error = %e, "could not list sandboxes for the GitHub reconcile");
                return;
            }
        };
        for sandbox in sandboxes.iter().filter(|s| s.status != "running") {
            let refreshed = match locate_site(&app, &sandbox.name).await {
                Ok(site) => crate::proxy::iron::refresh_github_binding(&app, &site, &sandbox.name)
                    .await
                    .map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            if let Err(e) = refreshed {
                tracing::warn!(target: "credentials", sandbox = %sandbox.name, error = %e, "could not refresh the GitHub binding");
            }
        }
        for sandbox in sandboxes.iter().filter(|s| s.status == "running") {
            if let Err(e) = reconcile_sandbox(&app, &sandbox.name).await {
                tracing::warn!(target: "credentials", sandbox = %sandbox.name, error = %e, "GitHub access reconcile failed");
            }
        }
    });
}

/// Everything the Manage card needs, read host-side with no exec.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubSandboxStatus {
    pub opted_out: bool,
    pub outcome: Option<ApplyOutcome>,
    pub credential_captured_at_ms: Option<u64>,
    pub account_login: Option<String>,
    pub deployed: bool,
}

pub async fn status(app: &AppHandle, name: &str) -> Result<GithubSandboxStatus, AppError> {
    let state = github::load_state(&state_path(app, name)?)?;
    let manifest = match credentials::load_manifest(app, GITHUB_SERVICE) {
        Ok(manifest) => Some(manifest),
        Err(CredentialError::NotFound(_)) => None,
        Err(e) => return Err(e.into()),
    };
    // Display only: a failed probe shows the sandbox as local and triggers no action.
    let deployed = is_deployed(app, name).await.unwrap_or_else(|e| {
        tracing::warn!(target: "credentials", sandbox = %name, error = %e, "could not tell whether the sandbox is deployed");
        false
    });
    Ok(GithubSandboxStatus {
        opted_out: state.opted_out,
        outcome: state.outcome,
        credential_captured_at_ms: manifest.as_ref().map(|m| m.captured_at_ms),
        account_login: manifest.and_then(|m| m.account_login),
        deployed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(opted_out: bool, outcome: Option<ApplyOutcome>) -> GithubSandboxState {
        GithubSandboxState { opted_out, outcome }
    }

    #[test]
    fn gh_runs_with_the_placeholder_only_when_the_binding_is_live() {
        assert_eq!(binding_gate(Ok(BindingState::Live)), Ok(()));
        assert_eq!(binding_gate(Ok(BindingState::NotLive)), Err("network policy not applied yet".to_string()));
        assert_eq!(binding_gate(Err("x".repeat(300))).unwrap_err().len(), github::MAX_REASON_CHARS);
    }

    #[test]
    fn opting_out_or_losing_the_credential_drops_the_sidecar_binding() {
        // Covers the outcomes plan() answers with Forget or None, where no gh-side removal runs.
        for outcome in [None, Some(ApplyOutcome::NeedsImage)] {
            assert!(!wants_access(Some(5), &state(true, outcome.clone())));
            assert!(!wants_access(None, &state(false, outcome)));
        }
        assert!(wants_access(Some(5), &state(false, None)));
    }

    #[test]
    fn a_saved_credential_is_applied_when_nothing_is() {
        assert_eq!(plan(Some(5), &state(false, None)), Action::Apply(5));
    }

    #[test]
    fn a_current_apply_is_left_alone() {
        let applied = Some(ApplyOutcome::Applied { captured_at_ms: 5 });
        assert_eq!(plan(Some(5), &state(false, applied)), Action::None);
    }

    #[test]
    fn a_rotation_needs_no_reapply() {
        let applied = Some(ApplyOutcome::Applied { captured_at_ms: 5 });
        assert_eq!(plan(Some(9), &state(false, applied)), Action::None);
    }

    #[test]
    fn a_failed_or_imageless_apply_is_retried() {
        assert_eq!(plan(Some(5), &state(false, Some(ApplyOutcome::NeedsImage))), Action::Apply(5));
        let failed = Some(ApplyOutcome::Failed { reason: "x".into(), captured_at_ms: 5 });
        assert_eq!(plan(Some(5), &state(false, failed)), Action::Apply(5));
    }

    #[test]
    fn opting_out_or_deleting_the_credential_removes_an_applied_login() {
        let applied = Some(ApplyOutcome::Applied { captured_at_ms: 5 });
        assert_eq!(plan(Some(5), &state(true, applied.clone())), Action::Remove);
        assert_eq!(plan(None, &state(false, applied)), Action::Remove);
    }

    #[test]
    fn a_failed_apply_may_hold_a_login_so_it_is_removed_too() {
        let failed = Some(ApplyOutcome::Failed { reason: "x".into(), captured_at_ms: 5 });
        assert_eq!(plan(Some(5), &state(true, failed.clone())), Action::Remove);
        assert_eq!(plan(None, &state(false, failed)), Action::Remove);
    }

    #[test]
    fn an_imageless_outcome_is_forgotten_without_a_script() {
        assert_eq!(plan(None, &state(false, Some(ApplyOutcome::NeedsImage))), Action::Forget);
        assert_eq!(plan(Some(5), &state(true, Some(ApplyOutcome::NeedsImage))), Action::Forget);
        assert_eq!(plan(None, &state(false, None)), Action::None);
        assert_eq!(plan(Some(5), &state(true, None)), Action::None);
    }
}
