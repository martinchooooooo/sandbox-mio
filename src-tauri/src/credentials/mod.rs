//! Host-side credential store: capture once, inject into any sandbox to skip interactive login.
//! Stores full directory snapshot (preserves onboarding state) as `home.tar.gz` per service.

pub mod types;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::AppHandle;

use crate::backends::types::SandboxBackend;
use crate::env_ext::env_dir;
use crate::provider::types::{ExecOpts, MachineProvider, ProviderId};
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

/// Service id of the GitHub token: a single-value credential, not a directory snapshot.
pub const GITHUB_SERVICE: &str = "github";

/// Variables a GitHub token is exported as inside a sandbox: `mise`'s GitHub backends read both.
const GITHUB_TOKEN_ENV: [&str; 2] = ["GITHUB_TOKEN", "MISE_GITHUB_TOKEN"];

/// File holding a single-value credential inside its bundle directory.
const SECRET_FILE: &str = "secret";

/// Writes a single-value credential into `dir` beside its manifest. The value never leaves the
/// host store through IPC; only the returned manifest does.
fn write_secret(dir: &Path, value: &str) -> Result<CredentialManifest, CredentialError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CredentialError::InvalidValue("the value is empty".to_string()));
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(CredentialError::InvalidValue(
            "the value contains whitespace or control characters".to_string(),
        ));
    }
    std::fs::write(dir.join(SECRET_FILE), value).map_err(|e| CredentialError::Io(e.to_string()))?;
    let manifest = CredentialManifest { captured_at_ms: now_ms(), account_email: None, expires_at_ms: None };
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| CredentialError::Parse(e.to_string()))?;
    std::fs::write(dir.join("manifest.json"), json).map_err(|e| CredentialError::Io(e.to_string()))?;
    Ok(manifest)
}

fn read_secret(dir: &Path, service_id: &str) -> Result<String, CredentialError> {
    match std::fs::read_to_string(dir.join(SECRET_FILE)) {
        Ok(v) => Ok(v.trim().to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(CredentialError::NotFound(service_id.to_string()))
        }
        Err(e) => Err(CredentialError::Io(e.to_string())),
    }
}

/// Stores `value` as the single-value credential `service_id`, replacing any previous one.
pub fn save_secret(
    app: &AppHandle,
    service_id: &str,
    value: &str,
) -> Result<CredentialManifest, CredentialError> {
    write_secret(&bundle_dir(app, service_id)?, value)
}

/// Reads back the single-value credential `service_id` for host-side use only.
pub fn load_secret(app: &AppHandle, service_id: &str) -> Result<String, CredentialError> {
    read_secret(&store_dir(app)?.join(service_id), service_id)
}

fn github_env(token: &str) -> Vec<(String, String)> {
    GITHUB_TOKEN_ENV.iter().map(|name| (name.to_string(), token.to_string())).collect()
}

/// The secret environment a sandbox-side `mise` call carries to lift GitHub's unauthenticated rate
/// limit: empty when no token is saved, so behaviour is unchanged (`docs/features/techdebt-sweep/
/// 02_dev.md`, Resolved Decision 3). A store that cannot be read is logged and treated as empty,
/// because a missing token must never fail a package install.
pub fn github_secret_env(app: &AppHandle) -> Vec<(String, String)> {
    match load_secret(app, GITHUB_SERVICE) {
        Ok(token) if !token.is_empty() => github_env(&token),
        Ok(_) | Err(CredentialError::NotFound(_)) => Vec::new(),
        Err(e) => {
            tracing::warn!(target: "credentials", error = %e, "GitHub token unreadable");
            Vec::new()
        }
    }
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
        // Pinned to Podman rather than the user's `default_provider`: an auth container is a
        // throwaway the app creates and destroys around one login, so it has no reason to pay a
        // microsandbox sandbox's volume-plus-sync setup.
        backend.ensure_base_image(ProviderId::Podman).await?;
        // An auth container is a throwaway the app creates and destroys around one login; it is
        // never proxied.
        let sandbox = backend.create(name, None, ProviderId::Podman, None, None).await?;
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
    provider: &Arc<dyn MachineProvider>,
) -> Result<CredentialManifest, CredentialError> {
    let cred_json = exec_read(provider, name, &["cat", credential_path]).await?;
    let cred: serde_json::Value =
        serde_json::from_str(cred_json.trim()).map_err(|e| CredentialError::Parse(e.to_string()))?;
    let expires_at_ms = crate::agents::json_path_get(&cred, expires_at_json_key).and_then(|v| v.as_u64());

    let mut account_email = None;
    if let Some(src_path) = account_source_path {
        let src_json = exec_read(provider, name, &["cat", src_path]).await?;
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
    let archive = tar_capture(provider, name, &paths).await?;

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
    provider: &Arc<dyn MachineProvider>,
) -> Result<(), CredentialError> {
    let dir = bundle_dir(app, service_id)?;
    let archive = std::fs::read(dir.join("home.tar.gz")).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CredentialError::NotFound(service_id.to_string())
        } else {
            CredentialError::Io(e.to_string())
        }
    })?;

    tar_inject(provider, name, &archive).await
}

/// Patch account-source file to skip onboarding wizard and trust the project (idempotent).
pub async fn seed_onboarding(
    name: &str,
    seed: &OnboardingSeed,
    provider: &Arc<dyn MachineProvider>,
) -> Result<(), CredentialError> {
    let mut root = provider
        .exec(name, &["cat", &seed.account_source_path], ExecOpts::default())
        .await
        .ok()
        .and_then(|out| {
            if out.status.success() {
                serde_json::from_str::<serde_json::Value>(
                    String::from_utf8_lossy(&out.stdout).trim(),
                )
                .ok()
            } else {
                None
            }
        })
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
    write_file_via_stdin(provider, name, &seed.account_source_path, serialized.as_bytes()).await
}

/// Exec `argv` inside `name`, check success, return stdout as a string.
async fn exec_read(
    provider: &Arc<dyn MachineProvider>,
    name: &str,
    argv: &[&str],
) -> Result<String, CredentialError> {
    let out = provider.exec(name, argv, ExecOpts::default()).await?;
    if !out.status.success() {
        return Err(CredentialError::ExecFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Write file via stdin to preserve permissions (path is constant, never user input).
async fn write_file_via_stdin(
    provider: &Arc<dyn MachineProvider>,
    name: &str,
    path: &str,
    bytes: &[u8],
) -> Result<(), CredentialError> {
    provider.start(name).await?;
    let script = format!("cat >{path} && chmod 600 {path}");
    let out = provider.exec_piped(name, &["sh", "-c", &script], bytes).await?;
    if !out.status.success() {
        return Err(CredentialError::ExecFailed(
            String::from_utf8_lossy(&out.stderr).to_string(),
        ));
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
async fn tar_capture(
    provider: &Arc<dyn MachineProvider>,
    name: &str,
    paths: &[String],
) -> Result<Vec<u8>, CredentialError> {
    provider.start(name).await?;
    let rel: Vec<&str> = paths.iter().map(|p| tar_relative(p)).collect();
    let mut argv = vec!["tar", "-C", "/", "-czf", "-"];
    argv.extend_from_slice(&rel);
    let out = provider.exec_piped(name, &argv, &[]).await?;
    if !out.status.success() {
        return Err(CredentialError::ExecFailed(
            String::from_utf8_lossy(&out.stderr).to_string(),
        ));
    }
    Ok(out.stdout)
}

/// Extract tar archive at original absolute paths and permissions via stdin.
async fn tar_inject(
    provider: &Arc<dyn MachineProvider>,
    name: &str,
    archive: &[u8],
) -> Result<(), CredentialError> {
    provider.start(name).await?;
    let out = provider
        .exec_piped(name, &["tar", "-C", "/", "-xzpf", "-"], archive)
        .await?;
    if !out.status.success() {
        return Err(CredentialError::ExecFailed(
            String::from_utf8_lossy(&out.stderr).to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mio-cred-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn a_saved_secret_reads_back_trimmed_beside_a_manifest() {
        let dir = scratch_dir("roundtrip");
        let manifest = write_secret(&dir, "  ghp_example  ").expect("saves");
        assert!(manifest.account_email.is_none());
        assert_eq!(read_secret(&dir, GITHUB_SERVICE).expect("reads"), "ghp_example");
        let on_disk: CredentialManifest =
            serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).expect("manifest"))
                .expect("parses");
        assert_eq!(on_disk.captured_at_ms, manifest.captured_at_ms);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_or_multiline_secret_is_refused() {
        let dir = scratch_dir("invalid");
        assert!(matches!(write_secret(&dir, "   "), Err(CredentialError::InvalidValue(_))));
        assert!(matches!(write_secret(&dir, "a\nb"), Err(CredentialError::InvalidValue(_))));
        assert!(!dir.join(SECRET_FILE).exists(), "nothing written for a refused value");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_secret_is_not_found() {
        let dir = scratch_dir("missing");
        assert!(matches!(read_secret(&dir, GITHUB_SERVICE), Err(CredentialError::NotFound(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_token_is_exported_under_both_names_mise_reads() {
        let env = github_env("tok");
        let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["GITHUB_TOKEN", "MISE_GITHUB_TOKEN"]);
        assert!(env.iter().all(|(_, v)| v == "tok"));
    }
}
