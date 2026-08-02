//! Host-side credential store: capture once, inject into any sandbox to skip interactive login.
//! Stores full directory snapshot (preserves onboarding state) as `home.tar.gz` per service.

pub mod types;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::AppHandle;

use crate::backends::types::SandboxBackend;
use crate::env_ext::env_dir;
use crate::process_ext::NoWindow;
use crate::runtime::types::RuntimeManager;
use types::{CredentialError, CredentialManifest, OnboardingSeed};

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// Root of the bundle store. `MIO_CREDENTIALS_DIR` relocates it wholesale (see
/// [`crate::env_ext::env_dir`]), else `MIO_APP_DATA_DIR` (see [`crate::paths::app_data_root`]):
/// without an override the e2e suite would operate on the developer's real captured logins, and
/// clearing one to reach a known-unauthenticated state costs a re-auth.
pub fn store_dir(app: &AppHandle) -> Result<PathBuf, CredentialError> {
    let dir = match env_dir("MIO_CREDENTIALS_DIR") {
        Some(root) => root,
        None => crate::paths::app_data_root(app)
            .map_err(|e| CredentialError::Io(e.to_string()))?
            .join("credentials"),
    };
    std::fs::create_dir_all(&dir).map_err(|e| CredentialError::Io(e.to_string()))?;
    Ok(dir)
}

pub fn bundle_dir(app: &AppHandle, service_id: &str) -> Result<PathBuf, CredentialError> {
    let dir = store_dir(app)?.join(service_id);
    std::fs::create_dir_all(&dir).map_err(|e| CredentialError::Io(e.to_string()))?;
    Ok(dir)
}

fn manifest_path(app: &AppHandle, service_id: &str) -> Result<PathBuf, CredentialError> {
    Ok(bundle_dir(app, service_id)?.join("manifest.json"))
}

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

pub fn save_manifest(
    app: &AppHandle,
    service_id: &str,
    manifest: &CredentialManifest,
) -> Result<(), CredentialError> {
    let path = manifest_path(app, service_id)?;
    let json = serde_json::to_string_pretty(manifest).map_err(|e| CredentialError::Parse(e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| CredentialError::Io(e.to_string()))
}

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

pub fn delete(app: &AppHandle, service_id: &str) -> Result<(), CredentialError> {
    let dir = bundle_dir(app, service_id)?;
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CredentialError::Io(e.to_string())),
    }
}

/// Create ephemeral auth container if absent (idempotent; destroyed after capture).
pub async fn ensure_auth_container(
    name: &str,
    backend: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
) -> Result<(), CredentialError> {
    let exists = backend.list().await?.iter().any(|s| s.name == name);
    if !exists {
        backend.ensure_base_image().await?;
        let sandbox = backend.create(name, None).await?;
        runtime.init(&sandbox.workspace_path).await?;
    }
    Ok(())
}

/// Capture credential and account-source files into a tar archive. Works for both auth containers and user sandboxes.
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

/// Extract stored credential archive into container at original paths and permissions.
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

/// Patch account-source file to skip onboarding wizard and trust the project (idempotent).
pub async fn seed_onboarding(
    name: &str,
    seed: &OnboardingSeed,
    runtime: &Arc<dyn RuntimeManager>,
) -> Result<(), CredentialError> {
    let mut root = runtime
        .exec_in_container(name, &["cat", &seed.account_source_path])
        .await
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s.trim()).ok())
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));

    let obj = root
        .as_object_mut()
        .ok_or_else(|| CredentialError::Parse("account-source file is not a JSON object".into()))?;

    if let Some(patch) = seed.account_file_patch.as_object() {
        for (key, value) in patch {
            obj.insert(key.clone(), value.clone());
        }
    }

    if !seed.project_trust_keys.is_empty() {
        let projects = obj
            .entry("projects")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if let Some(projects) = projects.as_object_mut() {
            let project = projects
                .entry(seed.workspace_dir.clone())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let Some(project) = project.as_object_mut() {
                for key in &seed.project_trust_keys {
                    project.insert(key.clone(), serde_json::Value::Bool(true));
                }
            }
        }
    }

    let serialized = serde_json::to_string(&root).map_err(|e| CredentialError::Parse(e.to_string()))?;
    write_file_via_stdin(
        runtime.local_connection(),
        name,
        &seed.account_source_path,
        serialized.as_bytes(),
    )
    .await
}

/// Write file via stdin to preserve permissions (path is constant, never user input).
async fn write_file_via_stdin(
    connection: &str,
    name: &str,
    path: &str,
    bytes: &[u8],
) -> Result<(), CredentialError> {
    use tokio::io::AsyncWriteExt;

    tokio::process::Command::new("podman")
        .no_window()
        .args(["--connection", connection, "start", name])
        .output()
        .await
        .map_err(|e| CredentialError::ExecFailed(e.to_string()))?;

    let script = format!("cat >{path} && chmod 600 {path}");
    let mut cmd = tokio::process::Command::new("podman");
    cmd.no_window();
    cmd.args(["--connection", connection, "exec", "-i", name, "sh", "-c", &script]);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| CredentialError::ExecFailed(format!("spawn failed: {e}")))?;
    let mut stdin = child.stdin.take().ok_or_else(|| CredentialError::ExecFailed("stdin not piped".into()))?;
    stdin.write_all(bytes).await.map_err(|e| CredentialError::ExecFailed(e.to_string()))?;
    drop(stdin); // close the pipe so the container-side `cat` sees EOF

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| CredentialError::ExecFailed(e.to_string()))?;
    if !output.status.success() {
        return Err(CredentialError::ExecFailed(String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(())
}

fn credential_dir(path: &str) -> String {
    path[..path.rfind('/').unwrap_or(path.len())].to_string()
}

fn tar_relative(path: &str) -> &str {
    path.strip_prefix('/').unwrap_or(path)
}

/// Capture files via tar to preserve permissions and binary data integrity.
async fn tar_capture(connection: &str, name: &str, paths: &[String]) -> Result<Vec<u8>, CredentialError> {
    tokio::process::Command::new("podman")
        .no_window()
        .args(["--connection", connection, "start", name])
        .output()
        .await
        .map_err(|e| CredentialError::ExecFailed(e.to_string()))?;

    let rel: Vec<&str> = paths.iter().map(|p| tar_relative(p)).collect();
    let mut args = vec!["--connection", connection, "exec", name, "tar", "-C", "/", "-czf", "-"];
    args.extend(rel.iter().copied());

    let output = tokio::process::Command::new("podman")
        .no_window()
        .args(&args)
        .output()
        .await
        .map_err(|e| CredentialError::ExecFailed(e.to_string()))?;
    if !output.status.success() {
        return Err(CredentialError::ExecFailed(String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(output.stdout)
}

/// Extract tar archive at original absolute paths and permissions via stdin.
async fn tar_inject(connection: &str, name: &str, archive: &[u8]) -> Result<(), CredentialError> {
    use tokio::io::AsyncWriteExt;

    tokio::process::Command::new("podman")
        .no_window()
        .args(["--connection", connection, "start", name])
        .output()
        .await
        .map_err(|e| CredentialError::ExecFailed(e.to_string()))?;

    let mut cmd = tokio::process::Command::new("podman");
    cmd.no_window();
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
