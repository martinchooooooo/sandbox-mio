//! Per-sandbox proxy sidecar: traffic inspection, egress policy and secret swap
//! (`docs/features/network-proxy/02_dev.md`). Free functions taking `&Arc<dyn MachineProvider>`
//! and `&AppHandle`, modelled on `crate::credentials` rather than the trait+mock subsystem
//! pattern — every external call already crosses the `ProcessRunner` seam through the provider, so
//! the e2e `light` tier is covered with no new trait.
//!
//! A Podman sidecar runs iron-proxy as the only exit of the sandbox's internal network
//! (`docs/features/podman-egress-boundary/02_dev.md`). Its `meta.json` records the engine and the
//! provider; a sidecar without an engine predates the boundary and runs mitmproxy.

pub mod audit;
pub mod flowlog;
pub mod iron;
pub mod iron_config;
pub mod types;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::AppHandle;

use crate::activity::types::TaskKind;
use crate::activity::TaskFailure;
use crate::provider::types::{InstanceNetwork, MachineProvider, ProviderId, SidecarMount, SidecarSpec};
use crate::provider::ProviderRegistry;
use types::{ProxyAttachment, ProxyEngine, ProxyError, ProxyStatus};

pub const PROXY_PORT: u16 = 8080;

/// In-sandbox path the proxy's CA cert is installed at. Referenced by both `inject_ca` and
/// `proxy_env` — the two must name the same path.
pub const CONTAINER_CA_PATH: &str = "/usr/local/share/ca-certificates/mio-proxy.crt";

const SUBNET_INDEX_MAX: u16 = 255;

/// The sidecar image for `provider_id`: iron-proxy
/// (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 9).
pub fn proxy_image(provider_id: ProviderId) -> String {
    match provider_id {
        ProviderId::Podman => iron::iron_proxy_image(),
    }
}

pub fn proxy_container_name(sandbox: &str) -> String {
    format!("{sandbox}{}", types::PROXY_CONTAINER_SUFFIX)
}

pub fn proxy_network_name(sandbox: &str) -> String {
    format!("mio-proxy-{sandbox}")
}

/// `<app_data_root>/proxy/<sandbox>/`. `MIO_APP_DATA_DIR` relocates the whole tree per worktree via
/// `crate::paths::app_data_root` — mandatory here, since this directory holds the sandbox's CA
/// private key.
fn state_dir(app: &AppHandle, sandbox: &str) -> Result<PathBuf, ProxyError> {
    let root = crate::paths::app_data_root(app)
        .map_err(|e| ProxyError::CommandFailed(e.to_string()))?
        .join("proxy")
        .join(sandbox);
    std::fs::create_dir_all(&root)
        .map_err(|e| ProxyError::Io { path: root.to_string_lossy().to_string(), error: e.to_string() })?;
    Ok(root)
}

fn certs_dir(app: &AppHandle, sandbox: &str) -> Result<PathBuf, ProxyError> {
    let dir = state_dir(app, sandbox)?.join("certs");
    std::fs::create_dir_all(&dir)
        .map_err(|e| ProxyError::Io { path: dir.to_string_lossy().to_string(), error: e.to_string() })?;
    Ok(dir)
}

/// The CA the sandbox must trust: the app-generated `ca.pem` an iron-proxy sidecar mounts.
fn ca_pem_path(app: &AppHandle, sandbox: &str, provider_id: ProviderId) -> Result<PathBuf, ProxyError> {
    let file = match provider_id {
        ProviderId::Podman => "ca.pem",
    };
    Ok(certs_dir(app, sandbox)?.join(file))
}

/// Forward-slashed absolute path: `podman -v` rejects backslash Windows paths
/// (`src-tauri/AGENTS.md`).
fn host_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The NDJSON flow log the sidecar appends to and `flowlog::watch_flows` polls, a single-file
/// mount at the state root.
pub fn flows_path(app: &AppHandle, sandbox: &str) -> Result<PathBuf, ProxyError> {
    let dir = state_dir(app, sandbox)?;
    Ok(match sidecar_provider_id(app, sandbox) {
        ProviderId::Podman => dir.join("flows.ndjson"),
    })
}

/// The provider `sandbox`'s sidecar runs on, from `meta.json`'s `provider` tag; Podman when the
/// file or tag is missing, which every sidecar created before the tag was written lacks.
pub fn sidecar_provider_id(app: &AppHandle, sandbox: &str) -> ProviderId {
    meta_path_if_exists(app, sandbox)
        .and_then(|p| crate::mio_files::read_value(&p))
        .and_then(|meta| meta["provider"].as_str().and_then(|tag| tag.parse().ok()))
        .unwrap_or_default()
}

/// `meta.json`'s path when it exists, without creating the state directory as a side effect.
fn meta_path_if_exists(app: &AppHandle, sandbox: &str) -> Option<PathBuf> {
    let path = crate::paths::app_data_root(app).ok()?.join("proxy").join(sandbox).join("meta.json");
    path.is_file().then_some(path)
}

/// Root of the app-global secret-value store: `<app_data_root>/proxy/secrets.json`. Written by
/// `set_proxy_secret`/`delete_proxy_secret`, never read back over IPC; only the iron-proxy apply
/// reads it.
fn secrets_path(app: &AppHandle) -> Result<PathBuf, ProxyError> {
    let root = crate::paths::app_data_root(app)
        .map_err(|e| ProxyError::CommandFailed(e.to_string()))?
        .join("proxy");
    std::fs::create_dir_all(&root)
        .map_err(|e| ProxyError::Io { path: root.to_string_lossy().to_string(), error: e.to_string() })?;
    Ok(root.join("secrets.json"))
}

pub fn load_secrets(app: &AppHandle) -> Result<HashMap<String, String>, ProxyError> {
    let path = secrets_path(app)?;
    match crate::mio_files::read_value(&path) {
        Some(v) => serde_json::from_value(v).map_err(|e| {
            ProxyError::CommandFailed(format!("failed to parse {}: {e}", path.to_string_lossy()))
        }),
        None => Ok(HashMap::new()),
    }
}

fn save_secrets(app: &AppHandle, secrets: &HashMap<String, String>) -> Result<(), ProxyError> {
    let path = secrets_path(app)?;
    crate::mio_files::write_pretty(&path, secrets).map_err(|e| ProxyError::CommandFailed(e.to_string()))
}

/// Set (or overwrite) the value stored for `secret_id`. Never logged or returned.
pub fn set_secret(app: &AppHandle, secret_id: &str, value: &str) -> Result<(), ProxyError> {
    let mut secrets = load_secrets(app)?;
    secrets.insert(secret_id.to_string(), value.to_string());
    save_secrets(app, &secrets)
}

pub fn delete_secret(app: &AppHandle, secret_id: &str) -> Result<(), ProxyError> {
    let mut secrets = load_secrets(app)?;
    secrets.remove(secret_id);
    save_secrets(app, &secrets)
}

/// Pulls `provider_id`'s sidecar image if absent, reported through `activity::TaskRegistry` — a
/// first pull would otherwise look like a hung sandbox create (Resolved Decision 10).
pub async fn ensure_proxy_image(
    app: &AppHandle,
    provider: &Arc<dyn MachineProvider>,
    provider_id: ProviderId,
) -> Result<(), ProxyError> {
    let image = proxy_image(provider_id);
    crate::activity::scope(app, "proxy-image-pull", TaskKind::ProxyImagePull, Some(image.clone()), |_scope| {
        let provider = provider.clone();
        let image = image.clone();
        async move { provider.pull_image(&image).await.map_err(|e| TaskFailure::same(ProxyError::from(e))) }
    })
    .await
}

/// Creates the internal `mio-proxy-<sandbox>` network on the first free `10.90.<index>.0/24`
/// subnet (`docs/features/network-proxy/02_dev.md`, Spike Results). Internal, so the sidecar at
/// `.2` is the only exit (`docs/features/podman-egress-boundary/02_dev.md`, Concrete Model).
async fn allocate_network(
    provider: &Arc<dyn MachineProvider>,
    sandbox: &str,
) -> Result<(String, String), ProxyError> {
    let name = proxy_network_name(sandbox);
    let mut last_err = None;
    for index in 0..=SUBNET_INDEX_MAX {
        let subnet = format!("10.90.{index}.0/24");
        match provider.create_network(&name, &subnet, true).await {
            Ok(()) => return Ok((name, format!("10.90.{index}.2"))),
            Err(e) => last_err = Some(e),
        }
    }
    Err(ProxyError::CommandFailed(format!(
        "no free 10.90.x.0/24 subnet for {sandbox}: {}",
        last_err.map(|e| e.to_string()).unwrap_or_default()
    )))
}

pub fn read_ca_pem(
    app: &AppHandle,
    sandbox: &str,
    provider_id: ProviderId,
) -> Result<Vec<u8>, ProxyError> {
    let path = ca_pem_path(app, sandbox, provider_id)?;
    std::fs::read(&path).map_err(|e| ProxyError::Io { path: path.to_string_lossy().to_string(), error: e.to_string() })
}

/// The full `--env` list for a proxy-enabled sandbox: the proxy URL under both casings, `no_proxy`
/// under both spellings (loopback included, which also carries local-LLM inference through the
/// sandbox's own inference forward), and the per-runtime CA trust overrides pointing at
/// `CONTAINER_CA_PATH` (Resolved Decision 8). `CONTAINER_CA_PATH` is also referenced by `inject_ca`;
/// the two must name the same path.
pub fn proxy_env(proxy_ip: &str) -> Vec<(String, String)> {
    let proxy_url = format!("http://{proxy_ip}:{PROXY_PORT}");
    let no_proxy = "localhost,127.0.0.1,::1";
    let mut env = vec![
        ("http_proxy".to_string(), proxy_url.clone()),
        ("https_proxy".to_string(), proxy_url.clone()),
        ("HTTP_PROXY".to_string(), proxy_url.clone()),
        ("HTTPS_PROXY".to_string(), proxy_url),
        ("no_proxy".to_string(), no_proxy.to_string()),
        ("NO_PROXY".to_string(), no_proxy.to_string()),
    ];
    for var in [
        "NODE_EXTRA_CA_CERTS",
        "REQUESTS_CA_BUNDLE",
        "PIP_CERT",
        "SSL_CERT_FILE",
        "CURL_CA_BUNDLE",
        "GIT_SSL_CAINFO",
    ] {
        env.push((var.to_string(), CONTAINER_CA_PATH.to_string()));
    }
    env
}

/// Pipes the proxy's CA cert into the sandbox and runs `update-ca-certificates`, the transport
/// `credentials::tar_inject` already established. Starts the sandbox first, as
/// `credentials::write_file_via_stdin` does. `CONTAINER_CA_PATH` is also referenced by `proxy_env`;
/// the two must name the same path.
pub async fn inject_ca(
    sandbox_provider: &Arc<dyn MachineProvider>,
    sandbox_name: &str,
    pem: &[u8],
) -> Result<(), ProxyError> {
    sandbox_provider.start(sandbox_name).await?;
    let script = format!("cat >{CONTAINER_CA_PATH} && update-ca-certificates");
    let out = sandbox_provider.exec_piped(sandbox_name, &["sh", "-c", &script], pem).await?;
    if !out.status.success() {
        return Err(ProxyError::CommandFailed(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(())
}

/// Creates the proxy sidecar and its state directory on `provider_id` and starts it. Returns the
/// `ProxyAttachment` `SandboxBackend::create` needs to build the sandbox's own `InstanceSpec`.
/// Caller must have already run `ensure_proxy_image`.
///
/// Policy starts empty: this runs *before* `SandboxBackend::create`, so the sandbox's workspace (and
/// therefore its `.mio/net-policy.json`) does not exist yet. A caller applies it afterward through
/// `iron::apply_policy`.
pub async fn create_proxy(
    app: &AppHandle,
    provider: &Arc<dyn MachineProvider>,
    provider_id: ProviderId,
    sandbox: &str,
) -> Result<ProxyAttachment, ProxyError> {
    match provider_id {
        ProviderId::Podman => create_podman_proxy(app, provider, sandbox).await,
    }
}

/// The dual-homed iron-proxy sidecar: pinned on the internal network, joined to Podman's default
/// `podman` network as its route out, booted with the deny-all bootstrap config until the first
/// `iron::apply_policy` (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decisions 3, 18).
async fn create_podman_proxy(
    app: &AppHandle,
    provider: &Arc<dyn MachineProvider>,
    sandbox: &str,
) -> Result<ProxyAttachment, ProxyError> {
    let (network, proxy_ip) = allocate_network(provider, sandbox).await?;
    let dir = state_dir(app, sandbox)?;
    let certs = certs_dir(app, sandbox)?;
    iron::generate_ca(&certs)?;
    iron::write_bootstrap(&dir, &proxy_ip)?;
    let secrets = iron::secrets_dir(&dir);
    std::fs::create_dir_all(&secrets).map_err(|e| io_err(&secrets, e))?;
    // Create-if-absent: a re-run against an existing state directory must not wipe the captured
    // history (`docs/features/proxy-traffic-viewer/02_dev.md`, Resolved Decision 5).
    let flows_path = flows_path(app, sandbox)?;
    if !flows_path.exists() {
        std::fs::write(&flows_path, b"").map_err(|e| io_err(&flows_path, e))?;
    }
    let token = uuid::Uuid::new_v4().simple().to_string();

    let container_name = proxy_container_name(sandbox);
    let mount = |host: &std::path::Path, dest: &str| SidecarMount {
        host_path: host_path(host),
        dest: dest.to_string(),
        read_only: true,
    };
    let spec = SidecarSpec {
        name: container_name.clone(),
        image: iron::iron_proxy_image(),
        network: Some(InstanceNetwork { name: network.clone(), ip: Some(proxy_ip.clone()), dns: None }),
        extra_networks: vec!["podman".to_string()],
        log_max_size: Some(iron::SIDECAR_LOG_MAX_SIZE.to_string()),
        publish: Vec::new(),
        // Not a user secret: it guards only the sidecar-loopback management listener (Resolved
        // Decision 13).
        env: vec![(iron_config::MGMT_API_KEY_ENV.to_string(), token)],
        mounts: vec![
            mount(&certs, iron_config::CONTAINER_CERTS_DIR),
            mount(&dir.join("config"), iron::CONTAINER_CONFIG_DIR),
            mount(&secrets, iron_config::CONTAINER_SECRETS_DIR),
        ],
        command: vec!["-config".to_string(), format!("{}/config.yaml", iron::CONTAINER_CONFIG_DIR)],
        labels: Vec::new(),
    };
    provider.create_sidecar(&spec).await?;
    provider.start(&container_name).await?;
    write_meta(
        app,
        sandbox,
        &serde_json::json!({
            "engine": ProxyEngine::IronProxy,
            "provider": ProviderId::Podman.to_string(),
            "network": network,
            "proxyIp": proxy_ip,
        }),
    )?;

    Ok(ProxyAttachment::PodmanBridge { network, env: proxy_env(&proxy_ip), dns: proxy_ip })
}

fn io_err(path: &std::path::Path, e: std::io::Error) -> ProxyError {
    ProxyError::Io { path: path.to_string_lossy().to_string(), error: e.to_string() }
}

/// Writes `bytes` beside `path` and renames over it, so a reader never sees a partial file.
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), ProxyError> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}

/// The proxy's own network name and pinned IP, cached beside its state so `status` can report them
/// without a `podman network inspect` round trip — the container's presence, not this file, remains
/// the source of truth for whether the proxy is enabled (Resolved Decision 3).
fn meta_path(app: &AppHandle, sandbox: &str) -> Result<PathBuf, ProxyError> {
    Ok(state_dir(app, sandbox)?.join("meta.json"))
}

fn write_meta(app: &AppHandle, sandbox: &str, body: &serde_json::Value) -> Result<(), ProxyError> {
    let path = meta_path(app, sandbox)?;
    crate::mio_files::write_compact(&path, body).map_err(|e| ProxyError::CommandFailed(e.to_string()))
}

fn read_meta(app: &AppHandle, sandbox: &str) -> Option<serde_json::Value> {
    crate::mio_files::read_value(&meta_path_if_exists(app, sandbox)?)
}

/// Mitmproxy when `meta` records no engine: only iron-proxy sidecars write one.
fn meta_engine(meta: &serde_json::Value) -> ProxyEngine {
    serde_json::from_value(meta["engine"].clone()).unwrap_or(ProxyEngine::Mitmproxy)
}

pub fn sidecar_engine(app: &AppHandle, sandbox: &str) -> ProxyEngine {
    read_meta(app, sandbox).map_or(ProxyEngine::Mitmproxy, |m| meta_engine(&m))
}

/// The sidecar's provider id and handle, resolved from `meta.json`.
pub fn sidecar_provider(
    app: &AppHandle,
    registry: &Arc<ProviderRegistry>,
    sandbox: &str,
) -> (ProviderId, Arc<dyn MachineProvider>) {
    let id = sidecar_provider_id(app, sandbox);
    (id, registry.local(id))
}

/// Whether `instance` is a proxy sidecar rather than a sandbox. The name suffix alone is not enough:
/// a sandbox created before the suffix was reserved can be called `x-proxy`. Sandboxes run the
/// versioned `sandbox-mio/base:<n>` image and so carry an `image_version`; the sidecar image tags
/// (iron-proxy `0.56.0`, or mitmproxy `11.1.3` on a pre-boundary sidecar) must never parse as one
/// (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 10).
pub fn is_sidecar(instance: &crate::backends::types::Sandbox) -> bool {
    instance.name.ends_with(types::PROXY_CONTAINER_SUFFIX) && instance.image_version.is_none()
}

/// Whether `sandbox` has a proxy sidecar right now — derived from the `<sandbox>-proxy` sidecar's
/// presence on the provider its `meta.json` names, never persisted (Resolved Decision 3).
pub async fn exists(app: &AppHandle, registry: &Arc<ProviderRegistry>, sandbox: &str) -> bool {
    let (_, provider) = sidecar_provider(app, registry, sandbox);
    provider
        .list_instances()
        .await
        .map(|instances| instances.iter().any(|s| s.name == proxy_container_name(sandbox) && is_sidecar(s)))
        .unwrap_or(false)
}

/// Restarts the proxy sidecar so it re-reads its mounted `config.yaml`.
pub async fn restart_proxy_container(
    app: &AppHandle,
    registry: &Arc<ProviderRegistry>,
    sandbox: &str,
) -> Result<(), ProxyError> {
    let (_, provider) = sidecar_provider(app, registry, sandbox);
    let name = proxy_container_name(sandbox);
    // A crashed sidecar may refuse the stop, so only the start decides the outcome.
    let stopped = provider.stop_sidecar(&name).await;
    match provider.start(&name).await {
        Ok(()) => Ok(()),
        Err(e) => Err(stopped.err().unwrap_or(e).into()),
    }
}

/// Removes the proxy container, its network, and its state directory, in that order — Podman
/// refuses to remove a network still holding an attached container, and the state directory holds
/// the CA private key, so it is deleted last and unconditionally. Best-effort throughout: called
/// both from the sandbox teardown path and as a rollback after a failed create, where the resources
/// that exist so far are the ones worth retrying to remove, not a reason to fail louder.
pub async fn remove_proxy(
    app: &AppHandle,
    provider: &Arc<dyn MachineProvider>,
    provider_id: ProviderId,
    sandbox: &str,
) {
    let container_name = proxy_container_name(sandbox);
    let is_a_sandbox = provider
        .list_instances()
        .await
        .map(|instances| instances.iter().any(|s| s.name == container_name && !is_sidecar(s)))
        .unwrap_or(false);
    if is_a_sandbox {
        tracing::warn!(target: "proxy", %sandbox, %container_name, "not removing: it is a sandbox, not a proxy sidecar");
    } else if let Err(e) = provider.remove(&container_name).await {
        tracing::warn!(target: "proxy", %sandbox, error = %e, "failed to remove proxy container");
    }
    if provider_id == ProviderId::Podman {
        let network = proxy_network_name(sandbox);
        if let Err(e) = provider.remove_network(&network).await {
            tracing::warn!(target: "proxy", %sandbox, error = %e, "failed to remove proxy network");
        }
    }
    if let Ok(dir) = state_dir(app, sandbox) {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!(target: "proxy", %sandbox, error = %e, "failed to remove proxy state directory");
        }
    }
}

/// Removes every `<name>-proxy` sidecar (and its network/state directory) whose base sandbox name
/// is absent from `live`.
pub async fn sweep(app: &AppHandle, registry: &Arc<ProviderRegistry>, live: &[String]) {
    for (id, provider) in registry.all() {
        let Ok(instances) = provider.list_instances().await else {
            continue;
        };
        for instance in instances.iter().filter(|s| is_sidecar(s)) {
            let Some(base) = instance.name.strip_suffix(types::PROXY_CONTAINER_SUFFIX) else {
                continue;
            };
            if live.iter().any(|n| n == base) {
                continue;
            }
            remove_proxy(app, &provider, id, base).await;
        }
    }
}

/// Derives proxy status from the `<sandbox>-proxy` container's presence — never persisted
/// (Resolved Decision 3).
pub async fn status(
    app: &AppHandle,
    registry: &Arc<ProviderRegistry>,
    sandbox: &str,
) -> Result<ProxyStatus, ProxyError> {
    let (id, provider) = sidecar_provider(app, registry, sandbox);
    let container_name = proxy_container_name(sandbox);
    let instances = provider.list_instances().await?;
    let network = proxy_network_name(sandbox);
    let meta = read_meta(app, sandbox).unwrap_or_default();
    let engine = meta_engine(&meta);
    let Some(instance) = instances.into_iter().find(|s| s.name == container_name && is_sidecar(s)) else {
        return Ok(ProxyStatus {
            exists: false,
            running: false,
            proxy_ip: String::new(),
            network,
            provider: id,
            engine,
        });
    };
    let running = instance.status == "running";
    Ok(ProxyStatus {
        exists: true,
        running,
        proxy_ip: meta["proxyIp"].as_str().unwrap_or_default().to_string(),
        network,
        provider: id,
        engine,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(name: &str, image_version: Option<u32>) -> crate::backends::types::Sandbox {
        crate::backends::types::Sandbox {
            id: name.to_string(),
            name: name.to_string(),
            status: "running".to_string(),
            workspace_path: String::new(),
            image_version,
            provider: ProviderId::Podman,
        }
    }

    /// A sandbox named `x-proxy` (allowed before the suffix was reserved) is not `x`'s sidecar.
    #[test]
    fn sidecar_needs_the_suffix_and_an_unversioned_image() {
        assert!(is_sidecar(&instance("micro-proxy", None)));
        assert!(!is_sidecar(&instance("micro-proxy", Some(6))));
        assert!(!is_sidecar(&instance("micro", None)));
        assert!(!is_sidecar(&instance("micro", Some(6))));
    }

    #[test]
    fn new_names_may_not_end_in_the_sidecar_suffix() {
        assert!(matches!(
            crate::backends::validate_new_name("micro-proxy"),
            Err(crate::backends::types::BackendError::ReservedName { .. })
        ));
        assert!(crate::backends::validate_new_name("proxy-micro").is_ok());
        assert!(crate::backends::validate_new_name("micro").is_ok());
    }
}
