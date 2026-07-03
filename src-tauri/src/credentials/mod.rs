//! Host-side credential store for shared agent auth: captures a whole-directory snapshot of an
//! agent's config (the directory holding its credential file, plus its account-source file if any)
//! out of a container once, and re-injects a verbatim copy into other containers, so a user does
//! not need to run the interactive login inside every sandbox. A full copy (rather than a merge of
//! just the OAuth subtree) means the target inherits the source's completed onboarding state too —
//! otherwise an agent's interactive UI can re-run first-run setup (including its own login-method
//! prompt) even though the injected token is already valid. Bundles persist as a single
//! `home.tar.gz` archive at `app_data_dir()/credentials/<service_id>/`, mirroring
//! `inference::models_root`/`model_dir`. Registered via `mod credentials` in `lib.rs`.

pub mod types;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::backends::types::SandboxBackend;
use crate::runtime::types::RuntimeManager;
use types::{CredentialError, CredentialManifest};

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// Root of the credential store, `<app_data>/credentials`. Created if absent.
pub fn store_dir(app: &AppHandle) -> Result<PathBuf, CredentialError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CredentialError::Io(e.to_string()))?
        .join("credentials");
    std::fs::create_dir_all(&dir).map_err(|e| CredentialError::Io(e.to_string()))?;
    Ok(dir)
}

/// Per-service bundle directory `<app_data>/credentials/<service_id>`. Created if absent.
pub fn bundle_dir(app: &AppHandle, service_id: &str) -> Result<PathBuf, CredentialError> {
    let dir = store_dir(app)?.join(service_id);
    std::fs::create_dir_all(&dir).map_err(|e| CredentialError::Io(e.to_string()))?;
    Ok(dir)
}

fn manifest_path(app: &AppHandle, service_id: &str) -> Result<PathBuf, CredentialError> {
    Ok(bundle_dir(app, service_id)?.join("manifest.json"))
}

/// Load the manifest for `service_id`. Errors with [`CredentialError::NotFound`] when no bundle
/// has been captured yet.
pub fn load_manifest(app: &AppHandle, service_id: &str) -> Result<CredentialManifest, CredentialError> {
    let path = manifest_path(app, service_id)?;
    let s = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CredentialError::NotFound(service_id.to_string())
        } else {
            CredentialError::Io(e.to_string())
        }
    })?;
    serde_json::from_str(&s).map_err(|e| CredentialError::Parse(e.to_string()))
}

/// Persist `manifest`, overwriting any existing one for `service_id`.
pub fn save_manifest(
    app: &AppHandle,
    service_id: &str,
    manifest: &CredentialManifest,
) -> Result<(), CredentialError> {
    let path = manifest_path(app, service_id)?;
    let json = serde_json::to_string_pretty(manifest).map_err(|e| CredentialError::Parse(e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| CredentialError::Io(e.to_string()))
}

/// List every service with a persisted bundle, scanning `store_dir` subdirectories.
pub fn list(app: &AppHandle) -> Result<Vec<(String, CredentialManifest)>, CredentialError> {
    let root = store_dir(app)?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).map_err(|e| CredentialError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| CredentialError::Io(e.to_string()))?;
        if !entry.path().is_dir() {
            continue;
        }
        let service_id = entry.file_name().to_string_lossy().to_string();
        if let Ok(manifest) = load_manifest(app, &service_id) {
            out.push((service_id, manifest));
        }
    }
    Ok(out)
}

/// Delete the host bundle for `service_id`. Container copies previously injected are untouched.
pub fn delete(app: &AppHandle, service_id: &str) -> Result<(), CredentialError> {
    let dir = bundle_dir(app, service_id)?;
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CredentialError::Io(e.to_string())),
    }
}

/// Idempotent: create the named auth container (with its mise workspace initialised, mirroring
/// `create_sandbox`) if no container by that name exists yet. Recreate-on-missing, not
/// reuse-forever — the container is destroyed after every capture (see `02_dev.md`'s Resolved
/// Decisions on auth container lifecycle), so this only matters as a safety net when a previous
/// capture failed to clean up.
pub async fn ensure_auth_container(
    name: &str,
    backend: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
) -> Result<(), CredentialError> {
    let exists = backend.list().await?.iter().any(|s| s.name == name);
    if !exists {
        backend.ensure_base_image().await?;
        let sandbox = backend.create(name).await?;
        runtime.init(&sandbox.workspace_path).await?;
    }
    Ok(())
}

/// Read the agent's credential file and (if given) the account-source file out of container
/// `name` (for the `CredentialManifest`'s `account_email`/`expires_at_ms`), tar the two paths (see
/// module doc for why the whole directory, not just the credential file), and persist the archive
/// plus a manifest under `bundle_dir(app, service_id)`. Works identically for an ephemeral auth
/// container and an already-authenticated user sandbox (the "save from this sandbox" path).
pub async fn capture(
    app: &AppHandle,
    name: &str,
    service_id: &str,
    credential_path: &str,
    expires_at_json_key: &str,
    account_source_path: Option<&str>,
    account_email_json_key: Option<&str>,
    runtime: &Arc<dyn RuntimeManager>,
) -> Result<CredentialManifest, CredentialError> {
    let cred_json = runtime.exec_in_container(name, &["cat", credential_path]).await?;
    let cred: serde_json::Value =
        serde_json::from_str(cred_json.trim()).map_err(|e| CredentialError::Parse(e.to_string()))?;
    let expires_at_ms = crate::agents::json_path_get(&cred, expires_at_json_key).and_then(|v| v.as_u64());

    let mut account_email = None;
    if let Some(src_path) = account_source_path {
        let src_json = runtime.exec_in_container(name, &["cat", src_path]).await?;
        let src: serde_json::Value =
            serde_json::from_str(src_json.trim()).map_err(|e| CredentialError::Parse(e.to_string()))?;
        if let Some(email_key) = account_email_json_key {
            account_email = crate::agents::json_path_get(&src, email_key)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }

    let mut paths = vec![credential_dir(credential_path)];
    if let Some(src_path) = account_source_path {
        paths.push(src_path.to_string());
    }
    let connection = runtime.local_connection();
    let archive = tar_capture(connection, name, &paths).await?;

    let dir = bundle_dir(app, service_id)?;
    std::fs::write(dir.join("home.tar.gz"), &archive).map_err(|e| CredentialError::Io(e.to_string()))?;

    let manifest = CredentialManifest { captured_at_ms: now_ms(), account_email, expires_at_ms };
    save_manifest(app, service_id, &manifest)?;
    Ok(manifest)
}

/// Write the stored `service_id` bundle into container `name`, extracting the captured archive
/// verbatim over the target's own copies (see module doc for why). Never uses `podman cp` — see
/// `tar_inject`.
pub async fn inject(
    app: &AppHandle,
    name: &str,
    service_id: &str,
    runtime: &Arc<dyn RuntimeManager>,
) -> Result<(), CredentialError> {
    let dir = bundle_dir(app, service_id)?;
    let archive = std::fs::read(dir.join("home.tar.gz")).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CredentialError::NotFound(service_id.to_string())
        } else {
            CredentialError::Io(e.to_string())
        }
    })?;

    let connection = runtime.local_connection();
    tar_inject(connection, name, &archive).await
}

/// Parent directory of `path` (e.g. `/root/.claude/.credentials.json` -> `/root/.claude`) — the
/// unit `capture`/`inject` copy wholesale.
fn credential_dir(path: &str) -> String {
    path[..path.rfind('/').unwrap_or(path.len())].to_string()
}

/// Strip a leading `/` so the path is safe to pass to `tar -C /` as a relative entry (both on
/// capture and on extract, so archive members round-trip to the same absolute location).
fn tar_relative(path: &str) -> &str {
    path.strip_prefix('/').unwrap_or(path)
}

/// `podman exec <name> tar -C / -czf - <paths...>`, run directly via `tokio::process::Command`
/// (not `RuntimeManager::exec_in_container`, whose `String::from_utf8_lossy` output would corrupt
/// binary gzip data). Returns the archive bytes. `podman cp` is rejected outright — the
/// 2026-07-02 spike (`01_ideas.md`) confirmed it lands files at mode `0666` regardless of source;
/// `tar` inside the container preserves original modes in the archive instead.
async fn tar_capture(connection: &str, name: &str, paths: &[String]) -> Result<Vec<u8>, CredentialError> {
    tokio::process::Command::new("podman")
        .args(["--connection", connection, "start", name])
        .output()
        .await
        .map_err(|e| CredentialError::ExecFailed(e.to_string()))?;

    let rel: Vec<&str> = paths.iter().map(|p| tar_relative(p)).collect();
    let mut args = vec!["--connection", connection, "exec", name, "tar", "-C", "/", "-czf", "-"];
    args.extend(rel.iter().copied());

    let output = tokio::process::Command::new("podman")
        .args(&args)
        .output()
        .await
        .map_err(|e| CredentialError::ExecFailed(e.to_string()))?;
    if !output.status.success() {
        return Err(CredentialError::ExecFailed(String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(output.stdout)
}

/// `podman exec -i <name> tar -C / -xzpf -` with `archive` piped to stdin, restoring every member
/// at its original absolute path and mode (`-p` preserves permissions, notably the credential
/// file's `0600`).
async fn tar_inject(connection: &str, name: &str, archive: &[u8]) -> Result<(), CredentialError> {
    use tokio::io::AsyncWriteExt;

    tokio::process::Command::new("podman")
        .args(["--connection", connection, "start", name])
        .output()
        .await
        .map_err(|e| CredentialError::ExecFailed(e.to_string()))?;

    let mut cmd = tokio::process::Command::new("podman");
    cmd.args(["--connection", connection, "exec", "-i", name, "tar", "-C", "/", "-xzpf", "-"]);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| CredentialError::ExecFailed(format!("spawn failed: {e}")))?;
    let mut stdin = child.stdin.take().ok_or_else(|| CredentialError::ExecFailed("stdin not piped".into()))?;
    stdin.write_all(archive).await.map_err(|e| CredentialError::ExecFailed(e.to_string()))?;
    drop(stdin); // close the pipe so the container-side `tar` sees EOF

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| CredentialError::ExecFailed(e.to_string()))?;
    if !output.status.success() {
        return Err(CredentialError::ExecFailed(String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(())
}
