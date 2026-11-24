//! The Podman iron-proxy sidecar: image pin, CA, secret files, and policy apply
//! (`docs/features/podman-egress-boundary/02_dev.md`, Phase D). The sidecar reads `config.yaml`
//! and the secret files at boot, so a restart applies the last written state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use tauri::AppHandle;

use super::types::{ProxyError, SidecarSite};
use super::{io_err, iron_config, write_atomic};
use crate::credentials::github;
use crate::credentials::types::CredentialError;
use crate::credentials::{self, GITHUB_SERVICE};
use crate::net_policy::{self, NetPolicy};
use crate::provider::types::{ExecOpts, MachineProvider};

/// Pinned to the version Phase A ran, never `latest` or an `-rc.N` tag. The tag must not parse as
/// an integer, or `is_sidecar` mistakes the sidecar for a sandbox. An upgrade re-runs the Phase A
/// spikes first (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 10).
pub const IRON_PROXY_IMAGE_DEFAULT: &str = "docker.io/ironsh/iron-proxy:0.56.0";

/// Caps the sidecar's `k8s-file` log, which is truncated to zero at the cap, so it must hold far
/// more than one poll's worth of audit rows (about 1 KB each; Resolved Decision 4).
pub const SIDECAR_LOG_MAX_SIZE: &str = "4mb";

/// The directory the sidecar keeps `config/`, `certs/` and `secrets/` under.
pub const CONTAINER_STATE_DIR: &str = "/etc/iron-proxy";
pub const CONTAINER_CONFIG_DIR: &str = "/etc/iron-proxy/config";

pub fn iron_proxy_image() -> String {
    std::env::var("MIO_IRON_PROXY_IMAGE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| IRON_PROXY_IMAGE_DEFAULT.to_string())
}

/// What an apply could not do without failing it: bindings left out because their secret has no
/// stored value, so their placeholder passes through unswapped (Resolved Decision 15).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    /// The `envVar` of each binding without a stored value.
    pub missing_secrets: Vec<String>,
}

/// A fresh ECDSA P-256 CA with ten-year validity, as `ca.pem` and `ca-key.pem` in `certs_dir`
/// (Resolved Decision 11). The same `ca.pem` is mounted into the sidecar and piped into the
/// sandbox by `inject_ca`.
pub fn generate_ca(certs_dir: &Path) -> Result<(), ProxyError> {
    use rcgen::{
        BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
        KeyUsagePurpose,
    };
    let failed = |e: rcgen::Error| ProxyError::CommandFailed(format!("CA generation failed: {e}"));
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(failed)?;
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Sandbox-mio proxy CA");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    // Backdated a day so a guest clock slightly behind the host still accepts it.
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(3650);
    let cert = params.self_signed(&key).map_err(failed)?;
    write_atomic(&certs_dir.join("ca-key.pem"), key.serialize_pem().as_bytes())?;
    write_atomic(&certs_dir.join("ca.pem"), cert.pem().as_bytes())
}

/// Writes one file per binding whose secret has a stored value into `dir`, holding the value's
/// exact bytes: a `file` source sends the content verbatim, so a trailing newline would reach the
/// upstream (Resolved Decision 15). Returns the variables written and the variables left out.
pub fn materialise_secret_files(
    dir: &Path,
    policy: &NetPolicy,
    values: &HashMap<String, String>,
) -> Result<(Vec<String>, Vec<String>), ProxyError> {
    std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let mut written = Vec::new();
    let mut missing = Vec::new();
    for binding in &policy.secrets {
        if has_value(binding, values) {
            write_atomic(&dir.join(&binding.env_var), values[&binding.secret_id].as_bytes())?;
            written.push(binding.env_var.clone());
        } else {
            missing.push(binding.env_var.clone());
        }
    }
    Ok((written, missing))
}

/// Whether `binding` has a stored value to write. The one filter behind both the files written to
/// disk and the files a render names, so the two cannot drift.
fn has_value(binding: &net_policy::SecretBinding, values: &HashMap<String, String>) -> bool {
    values.contains_key(&binding.secret_id)
}

/// The file names an apply leaves in the secrets directory and renders into the config: one per
/// workspace binding with a value, then the GitHub token file when `github`. Pure, so read-only
/// paths can ask what a write would produce.
fn planned_files(policy: &NetPolicy, values: &HashMap<String, String>, github: bool) -> Vec<String> {
    let mut files: Vec<String> = policy
        .secrets
        .iter()
        .filter(|b| has_value(b, values))
        .map(|b| b.env_var.clone())
        .collect();
    if github {
        files.push(iron_config::GITHUB_BINDING_FILE.to_string());
    }
    files
}

/// Writes the secret files and the GitHub token file (deleted when `token` is `None`). Returns the
/// files now in place (`planned_files`) and the variables left out. A failure may leave files half
/// written, so callers restore from a snapshot taken beforehand.
fn stage_files(
    dir: &Path,
    policy: &NetPolicy,
    values: &HashMap<String, String>,
    token: Option<&str>,
) -> Result<(Vec<String>, Vec<String>), ProxyError> {
    let (_, missing) = materialise_secret_files(dir, policy, values)?;
    stage_github_file(dir, token)?;
    Ok((planned_files(policy, values, token.is_some()), missing))
}

/// Deletes secret files no rendered binding names, so an unbound value does not stay on disk.
/// Runs only after a successful reload: the previous pipeline may still read them until then.
fn remove_stale_secret_files(dir: &Path, keep: &[String]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !keep.contains(&name) {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                tracing::warn!(target: "proxy", file = %name, error = %e, "failed to remove a stale secret file");
            }
        }
    }
}

/// The secret files in `dir` with their bytes, taken before an apply writes any, so a failed apply
/// can put back exactly what the running pipeline was started or reloaded with.
fn snapshot_secret_files(dir: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, ProxyError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_err(dir, e)),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| io_err(dir, e))?.path();
        let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
        files.push((path, bytes));
    }
    Ok(files)
}

/// Rolls `dir` back to `snapshot`: removes files the failed apply added and rewrites the rest.
/// `skip_github` leaves the GitHub token file as the apply left it (deleted), so a removal is never
/// undone (`docs/features/proxy-credential-secrets/02_dev.md`, Resolved Decision 12).
fn restore_secret_files(dir: &Path, snapshot: &[(PathBuf, Vec<u8>)], skip_github: bool) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for path in entries.flatten().map(|e| e.path()) {
            if !snapshot.iter().any(|(p, _)| *p == path) {
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::error!(target: "proxy", file = %path.display(), error = %e, "failed to remove an unapplied secret file");
                }
            }
        }
    }
    for (path, bytes) in snapshot {
        if skip_github && path.file_name().is_some_and(|n| n == iron_config::GITHUB_BINDING_FILE) {
            continue;
        }
        if let Err(e) = write_atomic(path, bytes) {
            tracing::error!(target: "proxy", file = %path.display(), error = %e, "failed to restore a secret file");
        }
    }
}

fn config_dir(state: &Path) -> PathBuf {
    state.join("config")
}

pub fn secrets_dir(state: &Path) -> PathBuf {
    state.join("secrets")
}

/// Writes the bootstrap config the sidecar boots with (Resolved Decision 18).
pub fn write_bootstrap(state: &Path, proxy_ip: &str) -> Result<(), ProxyError> {
    let dir = config_dir(state);
    std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    write_config(&dir.join("config.yaml"), &iron_config::bootstrap(proxy_ip))
}

/// JSON in a `.yaml` file: YAML 1.2 is a superset of JSON (Resolved Decision 12).
fn write_config(path: &Path, config: &serde_json::Value) -> Result<(), ProxyError> {
    let body = serde_json::to_vec_pretty(config)
        .map_err(|e| ProxyError::CommandFailed(format!("failed to render config: {e}")))?;
    write_atomic(path, &body)
}

/// How long a reload keeps retrying while the management listener is not yet bound: a sidecar
/// started moments ago (the first apply at create) may not have bound it yet.
const MGMT_READY_BUDGET: Duration = Duration::from_secs(10);
const MGMT_READY_STEP: Duration = Duration::from_millis(250);

/// Asks the running sidecar to reload `config.yaml`, retrying while its management listener is not
/// yet bound. The bearer token is read from the sidecar's own `IRON_MANAGEMENT_API_KEY`, so it
/// never enters a command line.
async fn reload(provider: &Arc<dyn MachineProvider>, sidecar: &str) -> Result<(), ProxyError> {
    let script = format!(
        "wget -q -O - --header \"Authorization: Bearer ${env}\" --post-data= \
         http://127.0.0.1:{port}/v1/reload",
        env = iron_config::MGMT_API_KEY_ENV,
        port = iron_config::IRON_MGMT_PORT,
    );
    let deadline = Instant::now() + MGMT_READY_BUDGET;
    loop {
        let out = provider.exec(sidecar, &["sh", "-c", &script], ExecOpts::default()).await?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // busybox wget's wording for a refused connection; any HTTP status means the listener is up.
        let not_bound = stderr.contains("can't connect") || stderr.contains("Connection refused");
        if !not_bound || Instant::now() >= deadline {
            return Err(ProxyError::CommandFailed(format!("the proxy reload failed: {stderr}")));
        }
        tokio::time::sleep(MGMT_READY_STEP).await;
    }
}

async fn is_running(
    provider: &Arc<dyn MachineProvider>,
    sidecar: &str,
) -> Result<bool, ProxyError> {
    Ok(provider.list_instances().await?.iter().any(|s| s.name == sidecar && s.status == "running"))
}

/// Starts a stopped sidecar and confirms it stays up (see [`confirm_started`]).
async fn start_and_confirm(
    provider: &Arc<dyn MachineProvider>,
    sidecar: &str,
) -> Result<(), ProxyError> {
    provider.start(sidecar).await?;
    confirm_started(provider, sidecar).await
}

/// `podman start` succeeds even when iron-proxy exits at once on a config it refuses, so a start is
/// followed by a reload: it answers only once the process has booted with the config, and fails if
/// the process has exited.
async fn confirm_started(provider: &Arc<dyn MachineProvider>, sidecar: &str) -> Result<(), ProxyError> {
    match reload(provider, sidecar).await {
        Ok(()) => Ok(()),
        Err(_) if !is_running(provider, sidecar).await.unwrap_or(true) => {
            Err(ProxyError::CommandFailed(
                "the proxy exited right after starting, so it refused the new config".to_string(),
            ))
        }
        Err(e) => Err(e),
    }
}

/// Whether `name` is safe to splice into the `sh -c` scripts below: secret file names come from a
/// validated environment variable name or the app's own constant, never from the sandbox.
fn safe_file_name(name: &str) -> Result<(), ProxyError> {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
        Ok(())
    } else {
        Err(ProxyError::CommandFailed(format!("unsafe secret file name {name:?}")))
    }
}

/// The remote secrets directory's path for one file.
fn remote_secret_path(name: &str) -> String {
    format!("{}/{name}", iron_config::CONTAINER_SECRETS_DIR)
}

/// The shell script that writes stdin to `name` in the sidecar's secrets directory through a
/// rename, so iron-proxy never reads a partial file.
fn push_script(name: &str) -> String {
    let target = remote_secret_path(name);
    format!("cat > {target}.tmp && mv {target}.tmp {target}")
}

/// The shell script that deletes every file in the secrets directory not named in `keep`.
fn stale_script(keep: &[String]) -> String {
    format!(
        "cd {dir} && for f in $(ls -A); do case \" {keep} \" in *\" $f \"*) ;; *) rm -f -- \"$f\" ;; esac; done",
        dir = iron_config::CONTAINER_SECRETS_DIR,
        keep = keep.join(" "),
    )
}

/// The shell script that swaps the copied-in `config.next.yaml` over `config.yaml`, keeping the
/// running config as `config.prev.yaml`; [`swap_back_script`] undoes it
/// (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 14).
fn swap_script() -> String {
    let dir = CONTAINER_CONFIG_DIR;
    format!("cd {dir} && mv -f config.yaml config.prev.yaml && mv -f config.next.yaml config.yaml")
}

fn swap_back_script() -> String {
    format!("cd {CONTAINER_CONFIG_DIR} && mv -f config.prev.yaml config.yaml")
}

/// Runs `script` in the remote sidecar and turns a non-zero exit into an error.
async fn exec_script(site: &SidecarSite, sidecar: &str, script: &str) -> Result<(), ProxyError> {
    let out = site.provider.exec(sidecar, &["sh", "-c", script], ExecOpts::default()).await?;
    if out.status.success() {
        return Ok(());
    }
    Err(ProxyError::CommandFailed(String::from_utf8_lossy(&out.stderr).trim().to_string()))
}

/// Writes each of `names` from the laptop's `secrets` directory into the remote sidecar's tmpfs. The
/// value travels on stdin only, never argv, the container environment, logs or IPC.
async fn push_secret_files(
    site: &SidecarSite,
    sidecar: &str,
    secrets: &Path,
    names: &[String],
) -> Result<(), ProxyError> {
    for name in names {
        safe_file_name(name)?;
        let path = secrets.join(name);
        let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
        let script = push_script(name);
        let out = site.provider.exec_piped(sidecar, &["sh", "-c", &script], &bytes).await?;
        if !out.status.success() {
            return Err(ProxyError::CommandFailed(format!(
                "could not push a secret file to the remote proxy: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    }
    Ok(())
}

/// Deletes the remote secret files not in `keep`. Runs only after a successful reload, for the reason
/// [`remove_stale_secret_files`] does.
async fn remove_stale_remote_files(
    site: &SidecarSite,
    sidecar: &str,
    keep: &[String],
) -> Result<(), ProxyError> {
    for name in keep {
        safe_file_name(name)?;
    }
    exec_script(site, sidecar, &stale_script(keep)).await
}

/// The names of the files in the laptop's `secrets` directory.
fn staged_secret_names(secrets: &Path) -> Vec<String> {
    std::fs::read_dir(secrets)
        .map(|entries| {
            entries.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()
        })
        .unwrap_or_default()
}

/// Makes sure `sandbox`'s sidecar on `site` runs, so a sandbox about to start has its network.
/// A remote sidecar gets the staged secret files pushed on every call, so a failed earlier push
/// self-heals (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decisions 1, 5).
/// A missing sidecar is a no-op, which the mock backend relies on.
pub async fn ensure_sidecar_running(
    app: &AppHandle,
    site: &SidecarSite,
    sandbox: &str,
) -> Result<(), ProxyError> {
    let sidecar = super::proxy_container_name(sandbox);
    let instances = site.provider.list_instances().await?;
    let Some(instance) = instances.iter().find(|s| s.name == sidecar && super::is_sidecar(s)) else {
        return Ok(());
    };
    let running = instance.status == "running";
    if site.local {
        return if running { Ok(()) } else { start_and_confirm(&site.provider, &sidecar).await };
    }
    let secrets = secrets_dir(&super::state_dir(app, sandbox)?);
    if !running {
        site.provider.start(&sidecar).await?;
    }
    push_secret_files(site, &sidecar, &secrets, &staged_secret_names(&secrets)).await?;
    if running { Ok(()) } else { confirm_started(&site.provider, &sidecar).await }
}

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
fn fail_closed_binding(sandbox: &str, binding: Result<Option<String>, ProxyError>) -> Option<String> {
    binding.unwrap_or_else(|e| {
        tracing::warn!(target: "proxy", %sandbox, error = %e, "GitHub binding unavailable; rendering the policy without it");
        None
    })
}

/// Writes the GitHub token file with the token's exact bytes, or deletes it when `token` is `None`.
fn stage_github_file(dir: &Path, token: Option<&str>) -> Result<(), ProxyError> {
    let path = dir.join(iron_config::GITHUB_BINDING_FILE);
    match token {
        Some(token) => {
            std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
            write_atomic(&path, token.as_bytes())
        }
        None => match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_err(&path, e)),
        },
    }
}

/// The app-owned bindings to render for a sandbox.
fn app_bindings(github: bool) -> Vec<iron_config::AppBinding> {
    if github { vec![iron_config::github_app_binding()] } else { Vec::new() }
}

/// Makes the sidecar on `site` run `policy`, already validated: reload when running, start when
/// stopped (Resolved Decision 13). A failure restores the previous files, and `applied.json` changes
/// only once the new policy runs. A deployed sandbox is applied live on the remote sidecar
/// (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decisions 3, 4, 12).
pub async fn apply_policy(
    app: &AppHandle,
    site: &SidecarSite,
    sandbox: &str,
    policy: NetPolicy,
) -> Result<ApplyReport, ProxyError> {
    let lock = apply_lock(sandbox);
    let _guard = lock.lock().await;
    apply_policy_locked(app, site, sandbox, policy).await
}

/// One lock per sandbox name. Every apply shares one `config.prev.yaml` and one secrets directory,
/// so two overlapping applies (two windows, or a Save racing a secret-value save) would back up each
/// other's unconfirmed config and delete each other's secret files.
fn apply_lock(sandbox: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        LazyLock::new(Default::default);
    LOCKS.lock().unwrap().entry(sandbox.to_string()).or_default().clone()
}

async fn apply_policy_locked(
    app: &AppHandle,
    site: &SidecarSite,
    sandbox: &str,
    policy: NetPolicy,
) -> Result<ApplyReport, ProxyError> {
    if site.local {
        apply_local_locked(app, site, sandbox, policy).await
    } else if super::read_remote_meta(app, sandbox).is_some() {
        apply_remote_locked(app, site, sandbox, policy).await
    } else {
        Err(ProxyError::CommandFailed(format!(
            "{sandbox:?} is deployed without a remote proxy; undeploy it and recreate the sandbox"
        )))
    }
}

/// The iron-proxy `meta.json` of `sandbox`'s local sidecar, or the error naming the missing sidecar.
fn local_meta(app: &AppHandle, sandbox: &str) -> Result<serde_json::Value, ProxyError> {
    super::read_meta(app, sandbox)
        .filter(|m| super::meta_engine(m) == super::types::ProxyEngine::IronProxy)
        .ok_or_else(|| {
            ProxyError::CommandFailed(format!(
                "{sandbox:?} has no iron-proxy sidecar; recreate the sandbox to get the network boundary"
            ))
        })
}

fn policy_err(e: net_policy::NetPolicyError) -> ProxyError {
    ProxyError::CommandFailed(e.0)
}

async fn apply_local_locked(
    app: &AppHandle,
    site: &SidecarSite,
    sandbox: &str,
    policy: NetPolicy,
) -> Result<ApplyReport, ProxyError> {
    let state = super::state_dir(app, sandbox)?;
    let meta = local_meta(app, sandbox)?;
    let proxy_ip = meta["proxyIp"].as_str().unwrap_or_default().to_string();
    let provider = &site.provider;
    // Read before anything changes: the rewrite below must carry `guest_env` forward, and losing it
    // would hide every unstamped binding.
    let guest_env = net_policy::read_applied(app, sandbox).map_err(policy_err)?.and_then(|a| a.guest_env);

    let secrets = secrets_dir(&state);
    let values = super::load_secrets(app)?;
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

    let sidecar = super::proxy_container_name(sandbox);
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

/// Records the applied snapshot once the new policy runs, and reports what had no stored value.
fn finish_apply(
    app: &AppHandle,
    sandbox: &str,
    policy: NetPolicy,
    guest_env: Option<Vec<String>>,
    missing_secrets: Vec<String>,
) -> Result<ApplyReport, ProxyError> {
    let applied = net_policy::AppliedPolicy { policy, guest_env };
    net_policy::write_applied(app, sandbox, &applied).map_err(policy_err)?;
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
    site: &SidecarSite,
    sandbox: &str,
    policy: NetPolicy,
) -> Result<ApplyReport, ProxyError> {
    let state = super::state_dir(app, sandbox)?;
    let local_ip = local_meta(app, sandbox)?["proxyIp"].as_str().unwrap_or_default().to_string();
    let remote_ip = super::read_remote_meta(app, sandbox)
        .and_then(|m| m["proxyIp"].as_str().map(str::to_string))
        .unwrap_or_default();
    let sidecar = super::proxy_container_name(sandbox);
    // A stopped remote sidecar (host reboot) is started with its last-applied files first; an
    // unreachable target fails here, before anything on the laptop changed.
    ensure_sidecar_running(app, site, sandbox).await?;
    let guest_env = net_policy::read_applied(app, sandbox).map_err(policy_err)?.and_then(|a| a.guest_env);

    let secrets = secrets_dir(&state);
    let values = super::load_secrets(app)?;
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
    let remote_dir = config_dir(&super::remote_dir(app, sandbox)?);
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

/// The remote half of an apply: pushes `names`, copies the rendered config in as `config.next.yaml`,
/// swaps it over `config.yaml` with one exec and reloads. A rejected reload swaps the old config back;
/// the sidecar is never restarted as a fallback.
async fn push_swap_reload(
    site: &SidecarSite,
    sidecar: &str,
    secrets: &Path,
    names: &[String],
    rendered_config: &Path,
) -> Result<(), ProxyError> {
    push_secret_files(site, sidecar, secrets, names).await?;
    let next = format!("{CONTAINER_CONFIG_DIR}/config.next.yaml");
    site.provider.copy_in(sidecar, &rendered_config.to_string_lossy(), &next).await?;
    exec_script(site, sidecar, &swap_script()).await?;
    if let Err(e) = reload(&site.provider, sidecar).await {
        if let Err(back) = exec_script(site, sidecar, &swap_back_script()).await {
            tracing::error!(target: "proxy", %sidecar, error = %back, "failed to swap the previous remote config back");
        }
        return Err(e);
    }
    Ok(())
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
    site: &SidecarSite,
    sandbox: &str,
) -> Result<BindingState, ProxyError> {
    let lock = apply_lock(sandbox);
    let _guard = lock.lock().await;
    let Some(meta) = super::read_meta(app, sandbox)
        .filter(|m| super::meta_engine(m) == super::types::ProxyEngine::IronProxy)
    else {
        return Ok(BindingState::NotLive);
    };
    let state = super::state_dir(app, sandbox)?;
    let Some(applied) = net_policy::read_applied(app, sandbox).map_err(policy_err)? else {
        // Nothing to render onto, but a failed first apply may have left the token file behind.
        remove_github_file(&secrets_dir(&state))?;
        return Ok(BindingState::NotLive);
    };
    let proxy_ip = meta["proxyIp"].as_str().unwrap_or_default().to_string();
    let binding = github_binding(app, sandbox);
    let lookup_error = binding.as_ref().err().map(ToString::to_string);
    let token = fail_closed_binding(sandbox, binding);
    let values = super::load_secrets(app)?;
    let config = rendered(&applied.policy, &proxy_ip, &values, token.is_some());
    if !github_files_current(&state, &config, token.as_deref()) {
        let sidecar = super::proxy_container_name(sandbox);
        let policy = applied.policy.clone();
        let refreshed: Result<(), ProxyError> = async {
            if is_running(&site.provider, &sidecar).await? {
                apply_policy_locked(app, site, sandbox, policy).await.map(drop)
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

/// How many secret values a deploy would send to the target: workspace bindings with a stored value
/// plus the live GitHub binding. A count only, so no value or secret id crosses IPC
/// (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 2).
pub fn deploy_secret_count(app: &AppHandle, sandbox: &str, workspace_path: &str) -> Result<usize, ProxyError> {
    let policy = net_policy::load(workspace_path).map_err(policy_err)?;
    let values = super::load_secrets(app)?;
    let github = fail_closed_binding(sandbox, github_binding(app, sandbox)).is_some();
    Ok(planned_files(&policy, &values, github).len())
}

/// Deletes the GitHub token file when it exists.
fn remove_github_file(secrets: &Path) -> Result<(), ProxyError> {
    let path = secrets.join(iron_config::GITHUB_BINDING_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err(&path, e)),
    }
}

/// The config an apply would write, with the workspace bindings that have a stored value.
fn rendered(
    policy: &NetPolicy,
    proxy_ip: &str,
    values: &HashMap<String, String>,
    github: bool,
) -> serde_json::Value {
    let written = planned_files(policy, values, github);
    iron_config::render(policy, proxy_ip, &written, &app_bindings(github))
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
pub fn github_binding_live(app: &AppHandle, sandbox: &str) -> Result<bool, ProxyError> {
    let Some(token) = github_binding(app, sandbox)? else {
        return Ok(false);
    };
    let policy_err = |e: net_policy::NetPolicyError| ProxyError::CommandFailed(e.0);
    let Some(meta) = super::read_meta(app, sandbox)
        .filter(|m| super::meta_engine(m) == super::types::ProxyEngine::IronProxy)
    else {
        return Ok(false);
    };
    let Some(applied) = net_policy::read_applied(app, sandbox).map_err(policy_err)? else {
        return Ok(false);
    };
    let proxy_ip = meta["proxyIp"].as_str().unwrap_or_default();
    let values = super::load_secrets(app)?;
    let config = rendered(&applied.policy, proxy_ip, &values, true);
    Ok(github_files_current(&super::state_dir(app, sandbox)?, &config, Some(&token)))
}

/// Writes the secret files and `config.yaml` of a stopped sidecar, so it boots with the current
/// state. Nothing runs, so a write failure restores the secret files and leaves the old config.
/// Returns the variables left out for want of a stored value.
fn write_offline(
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
    fn remove_github_file_deletes_a_stale_token_and_tolerates_none() {
        let dir = std::env::temp_dir().join(format!("mio-rmgh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mio-github"), b"ghp_stale").unwrap();
        remove_github_file(&dir).unwrap();
        assert!(!dir.join("mio-github").exists());
        remove_github_file(&dir).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A GitHub lookup error must drop the binding, never fail the apply or keep a token.
    #[test]
    fn binding_error_renders_without_the_token() {
        let err = Err(ProxyError::CommandFailed("corrupt state".to_string()));
        assert_eq!(fail_closed_binding("s", err), None);
        assert_eq!(fail_closed_binding("s", Ok(Some("t".to_string()))), Some("t".to_string()));
        assert_eq!(fail_closed_binding("s", Ok(None)), None);
    }

    /// A trailing newline in a secret file would be sent upstream inside the credential.
    #[test]
    fn secret_files_hold_exact_bytes_and_skip_missing_values() {
        let dir = std::env::temp_dir().join(format!("mio-iron-test-{}", uuid::Uuid::new_v4()));
        let policy: NetPolicy = serde_json::from_str(
            r#"{"secrets":[{"secretId":"a","envVar":"A_KEY","hosts":["a.example.com"]},
                           {"secretId":"b","envVar":"B_KEY","hosts":["b.example.com"]}]}"#,
        )
        .unwrap();
        let values = HashMap::from([("a".to_string(), "sk-123".to_string())]);
        let (written, missing) = materialise_secret_files(&dir, &policy, &values).unwrap();
        assert_eq!(std::fs::read(dir.join("A_KEY")).unwrap(), b"sk-123");
        assert_eq!(written, ["A_KEY"]);
        assert_eq!(missing, ["B_KEY"]);
        assert!(!dir.join("B_KEY").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A failed apply must leave the files the running pipeline was started with.
    #[test]
    fn restore_undoes_a_rotated_value_and_an_added_binding() {
        let dir = std::env::temp_dir().join(format!("mio-iron-test-{}", uuid::Uuid::new_v4()));
        let policy: NetPolicy = serde_json::from_str(
            r#"{"secrets":[{"secretId":"a","envVar":"A_KEY","hosts":["a.example.com"]},
                           {"secretId":"b","envVar":"B_KEY","hosts":["b.example.com"]}]}"#,
        )
        .unwrap();
        let old = HashMap::from([("a".to_string(), "old".to_string())]);
        materialise_secret_files(&dir, &policy, &old).unwrap();
        let snapshot = snapshot_secret_files(&dir).unwrap();
        let new = HashMap::from([("a".to_string(), "new".to_string()), ("b".to_string(), "b".to_string())]);
        materialise_secret_files(&dir, &policy, &new).unwrap();
        restore_secret_files(&dir, &snapshot, false);
        assert_eq!(std::fs::read(dir.join("A_KEY")).unwrap(), b"old");
        assert!(!dir.join("B_KEY").exists());
        std::fs::remove_dir_all(&dir).unwrap();
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

    #[test]
    fn the_github_file_holds_exact_bytes_and_survives_the_stale_sweep() {
        let dir = std::env::temp_dir().join(format!("mio-iron-test-{}", uuid::Uuid::new_v4()));
        stage_github_file(&dir, Some("ghp_realtoken")).unwrap();
        std::fs::write(dir.join("STALE_KEY"), b"x").unwrap();
        remove_stale_secret_files(&dir, &[iron_config::GITHUB_BINDING_FILE.to_string()]);
        assert_eq!(std::fs::read(dir.join("mio-github")).unwrap(), b"ghp_realtoken");
        assert!(!dir.join("STALE_KEY").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A removal must not be undone by the rollback of a failed reload.
    #[test]
    fn a_removed_github_file_stays_deleted_after_a_failed_apply() {
        let dir = std::env::temp_dir().join(format!("mio-iron-test-{}", uuid::Uuid::new_v4()));
        stage_github_file(&dir, Some("ghp_realtoken")).unwrap();
        let snapshot = snapshot_secret_files(&dir).unwrap();
        stage_github_file(&dir, None).unwrap();
        assert!(!dir.join("mio-github").exists());
        restore_secret_files(&dir, &snapshot, true);
        assert!(!dir.join("mio-github").exists());
        restore_secret_files(&dir, &snapshot, false);
        assert!(dir.join("mio-github").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remote_scripts_write_through_a_rename_and_swap_in_one_command() {
        assert_eq!(
            push_script("A_KEY"),
            "cat > /etc/iron-proxy/secrets/A_KEY.tmp && mv /etc/iron-proxy/secrets/A_KEY.tmp /etc/iron-proxy/secrets/A_KEY"
        );
        let swap = swap_script();
        assert!(swap.contains("mv -f config.yaml config.prev.yaml && mv -f config.next.yaml config.yaml"));
        assert!(swap_back_script().ends_with("mv -f config.prev.yaml config.yaml"));
        let stale = stale_script(&["A_KEY".to_string(), "mio-github".to_string()]);
        assert!(stale.contains("\" A_KEY mio-github \""), "{stale}");
    }

    /// File names reach `sh -c`, so anything outside the environment-variable alphabet is refused.
    #[test]
    fn unsafe_secret_file_names_are_refused() {
        assert!(safe_file_name("OPENAI_API_KEY").is_ok());
        assert!(safe_file_name("mio-github").is_ok());
        for bad in ["", "a b", "a;rm", "../x", "a$b", "a'b"] {
            assert!(safe_file_name(bad).is_err(), "{bad}");
        }
    }

    /// The two sites render one policy, differing only in the sidecar address the DNS answers with.
    #[test]
    fn two_sites_render_configs_that_differ_only_in_the_proxy_address() {
        let policy: NetPolicy = serde_json::from_str("{}").unwrap();
        let mut local = rendered(&policy, "10.90.1.2", &HashMap::new(), false);
        let mut remote = rendered(&policy, "10.90.7.2", &HashMap::new(), false);
        assert_ne!(local, remote);
        local["dns"]["proxy_ip"] = serde_json::Value::Null;
        remote["dns"]["proxy_ip"] = serde_json::Value::Null;
        assert_eq!(local, remote);
    }
}
