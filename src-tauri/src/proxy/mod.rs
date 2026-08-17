//! Per-sandbox mitmproxy sidecar: traffic inspection and secret injection
//! (`docs/features/network-proxy/02_dev.md`). Free functions taking `&Arc<dyn MachineProvider>`
//! and `&AppHandle`, modelled on `crate::credentials` rather than the trait+mock subsystem
//! pattern — every external call already crosses the `ProcessRunner` seam through the provider, so
//! the e2e `light` tier is covered with no new trait.
//!
//! Podman-only for v1 (Resolved Decision 9): every caller here is refused earlier, at
//! `CliBackend::create`/`deploy`, for a microsandbox-provider sandbox.

pub mod rules;
pub mod types;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::AppHandle;

use crate::activity::types::TaskKind;
use crate::activity::TaskFailure;
use crate::provider::types::{
    InstanceNetwork, MachineProvider, ProviderId, SidecarMount, SidecarSpec,
};
use crate::provider::ProviderRegistry;
use types::{ProxyAttachment, ProxyError, ProxyStatus};

/// Pinned mitmproxy release, never `latest` (Resolved Decision 10). Overridable for tests.
const MITMPROXY_IMAGE_DEFAULT: &str = "docker.io/mitmproxy/mitmproxy:11.1.3";

pub const PROXY_PORT: u16 = 8080;

/// In-sandbox path the proxy's CA cert is installed at. Referenced by both `inject_ca` and
/// `proxy_env` — the two must name the same path.
pub const CONTAINER_CA_PATH: &str = "/usr/local/share/ca-certificates/mio-proxy.crt";

const SUBNET_INDEX_MAX: u16 = 255;

pub fn mitmproxy_image() -> String {
    std::env::var("MIO_PROXY_IMAGE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| MITMPROXY_IMAGE_DEFAULT.to_string())
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

fn ca_pem_path(app: &AppHandle, sandbox: &str) -> Result<PathBuf, ProxyError> {
    Ok(certs_dir(app, sandbox)?.join("mitmproxy-ca-cert.pem"))
}

/// Forward-slashed absolute path: `podman -v` rejects backslash Windows paths
/// (`src-tauri/AGENTS.md`).
fn host_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The mitmproxy addon script this project owns, embedded so an app upgrade refreshes the copy
/// written to `<state>/addon.py` at every `create_proxy` with no image rebuild needed.
const ADDON_SCRIPT: &str = include_str!("../../resources/mitm_addon.py");

/// Root of the app-global secret-value store: `<app_data_root>/proxy/secrets.json`. Written by
/// `set_proxy_secret`/`delete_proxy_secret`, never read back over IPC — only `materialise` reads it,
/// into the proxy-only `rules.json`.
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

/// Materialises `workspace_path`'s `.mio/proxy-rules.json` into `<state>/rules.json`, the copy
/// mounted into the proxy container.
fn write_materialised_rules(
    app: &AppHandle,
    sandbox: &str,
    workspace_path: &str,
) -> Result<(), ProxyError> {
    let defs = rules::load_rules(workspace_path)?;
    let materialised = rules::materialise(app, &defs)?;
    let path = state_dir(app, sandbox)?.join("rules.json");
    crate::mio_files::write_compact(&path, &materialised)
        .map_err(|e| ProxyError::CommandFailed(e.to_string()))
}

/// Pulls the mitmproxy image if absent, reported through `activity::TaskRegistry` — the first pull
/// is ~200 MB and would otherwise look like a hung sandbox create (Resolved Decision 10).
pub async fn ensure_proxy_image(
    app: &AppHandle,
    provider: &Arc<dyn MachineProvider>,
) -> Result<(), ProxyError> {
    let image = mitmproxy_image();
    crate::activity::scope(app, "proxy-image-pull", TaskKind::ProxyImagePull, Some(image.clone()), |_scope| {
        let provider = provider.clone();
        let image = image.clone();
        async move { provider.pull_image(&image).await.map_err(|e| TaskFailure::same(ProxyError::from(e))) }
    })
    .await
}

/// Creates `mio-proxy-<sandbox>` on the first `10.90.<index>.0/24` subnet Podman accepts, walking
/// to the next index on an overlap failure (`docs/features/network-proxy/02_dev.md`, Spike
/// Results). Returns the network name and the proxy's pinned `.2` address.
async fn allocate_network(
    provider: &Arc<dyn MachineProvider>,
    sandbox: &str,
) -> Result<(String, String), ProxyError> {
    let name = proxy_network_name(sandbox);
    let mut last_err = None;
    for index in 0..=SUBNET_INDEX_MAX {
        let subnet = format!("10.90.{index}.0/24");
        match provider.create_network(&name, &subnet).await {
            Ok(()) => return Ok((name, format!("10.90.{index}.2"))),
            Err(e) => last_err = Some(e),
        }
    }
    Err(ProxyError::CommandFailed(format!(
        "no free 10.90.x.0/24 subnet for {sandbox}: {}",
        last_err.map(|e| e.to_string()).unwrap_or_default()
    )))
}

/// Polls for `certs/mitmproxy-ca-cert.pem` to appear — `mitmdump` writes its CA keypair into the
/// bind-mounted confdir on first run, and `inject_ca` must never run before it exists.
async fn wait_for_ca(app: &AppHandle, sandbox: &str) -> Result<(), ProxyError> {
    let path = ca_pem_path(app, sandbox)?;
    const ATTEMPTS: u32 = 100;
    const INTERVAL_MS: u64 = 100;
    for _ in 0..ATTEMPTS {
        if path.exists() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(INTERVAL_MS)).await;
    }
    Err(ProxyError::CommandFailed(format!(
        "timed out waiting for {} to appear",
        path.to_string_lossy()
    )))
}

pub fn read_ca_pem(app: &AppHandle, sandbox: &str) -> Result<Vec<u8>, ProxyError> {
    let path = ca_pem_path(app, sandbox)?;
    std::fs::read(&path).map_err(|e| ProxyError::Io { path: path.to_string_lossy().to_string(), error: e.to_string() })
}

/// The full `--env` list for a proxy-enabled sandbox: the proxy URL under both casings, `no_proxy`
/// under both spellings (`host.containers.internal` included — load-bearing, or local-LLM inference
/// gets routed through the proxy for no benefit), and the per-runtime CA trust overrides pointing at
/// `CONTAINER_CA_PATH` (Resolved Decision 8). `CONTAINER_CA_PATH` is also referenced by `inject_ca`;
/// the two must name the same path.
pub fn proxy_env(proxy_ip: &str) -> Vec<(String, String)> {
    let proxy_url = format!("http://{proxy_ip}:{PROXY_PORT}");
    let no_proxy = "localhost,127.0.0.1,::1,host.containers.internal";
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

/// Creates the proxy's network, container, and state directory, starts the container, and waits
/// for its CA to appear. Returns the `ProxyAttachment` `SandboxBackend::create` needs to build the
/// sandbox's own `InstanceSpec`. Caller must have already run `ensure_proxy_image`.
///
/// Rules start empty: this runs *before* `SandboxBackend::create`, so the sandbox's workspace (and
/// therefore its `.mio/proxy-rules.json`) does not exist yet. A caller adds rules afterward through
/// `apply_rules`, which re-materialises against the now-existing workspace and restarts the
/// container.
pub async fn create_proxy(
    app: &AppHandle,
    provider: &Arc<dyn MachineProvider>,
    sandbox: &str,
) -> Result<ProxyAttachment, ProxyError> {
    let (network, proxy_ip) = allocate_network(provider, sandbox).await?;
    let certs = certs_dir(app, sandbox)?;
    let dir = state_dir(app, sandbox)?;
    let addon_path = dir.join("addon.py");
    std::fs::write(&addon_path, ADDON_SCRIPT)
        .map_err(|e| ProxyError::Io { path: addon_path.to_string_lossy().to_string(), error: e.to_string() })?;
    let rules_path = dir.join("rules.json");
    crate::mio_files::write_compact(&rules_path, &serde_json::Value::Array(Vec::new()))
        .map_err(|e| ProxyError::CommandFailed(e.to_string()))?;

    let container_name = proxy_container_name(sandbox);
    let spec = SidecarSpec {
        name: container_name.clone(),
        image: mitmproxy_image(),
        network: InstanceNetwork { name: network.clone(), ip: Some(proxy_ip.clone()) },
        mounts: vec![
            SidecarMount {
                host_path: host_path(&certs),
                dest: "/home/mitmproxy/.mitmproxy".to_string(),
                read_only: false,
            },
            SidecarMount {
                host_path: host_path(&addon_path),
                dest: "/addons/addon.py".to_string(),
                read_only: true,
            },
            SidecarMount {
                host_path: host_path(&rules_path),
                dest: "/addons/rules.json".to_string(),
                read_only: true,
            },
        ],
        command: vec![
            "mitmdump".to_string(),
            "--listen-host".to_string(),
            "0.0.0.0".to_string(),
            "--listen-port".to_string(),
            PROXY_PORT.to_string(),
            "-s".to_string(),
            "/addons/addon.py".to_string(),
        ],
        labels: Vec::new(),
    };
    provider.create_sidecar(&spec).await?;
    provider.start(&container_name).await?;
    wait_for_ca(app, sandbox).await?;
    write_meta(app, sandbox, &network, &proxy_ip)?;

    Ok(ProxyAttachment { network, env: proxy_env(&proxy_ip) })
}

/// The proxy's own network name and pinned IP, cached beside its state so `status` can report them
/// without a `podman network inspect` round trip — the container's presence, not this file, remains
/// the source of truth for whether the proxy is enabled (Resolved Decision 3).
fn meta_path(app: &AppHandle, sandbox: &str) -> Result<PathBuf, ProxyError> {
    Ok(state_dir(app, sandbox)?.join("meta.json"))
}

fn write_meta(app: &AppHandle, sandbox: &str, network: &str, proxy_ip: &str) -> Result<(), ProxyError> {
    let path = meta_path(app, sandbox)?;
    let body = serde_json::json!({ "network": network, "proxyIp": proxy_ip });
    crate::mio_files::write_compact(&path, &body).map_err(|e| ProxyError::CommandFailed(e.to_string()))
}

fn read_meta_proxy_ip(app: &AppHandle, sandbox: &str) -> Option<String> {
    let path = meta_path(app, sandbox).ok()?;
    crate::mio_files::read_value(&path)?["proxyIp"].as_str().map(str::to_string)
}

/// Whether `sandbox` has a proxy sidecar right now — derived from the `<sandbox>-proxy` container's
/// presence, never persisted (Resolved Decision 3).
pub async fn exists(registry: &Arc<ProviderRegistry>, sandbox: &str) -> bool {
    let provider = registry.local(ProviderId::Podman);
    provider
        .list_instances()
        .await
        .map(|instances| instances.iter().any(|s| s.name == proxy_container_name(sandbox)))
        .unwrap_or(false)
}

/// Restarts the proxy container so it re-reads the materialised `rules.json` mounted at
/// `/addons/rules.json` — the addon has no built-in watch on a mounted file (Resolved Decision 11).
pub async fn restart_proxy_container(
    provider: &Arc<dyn MachineProvider>,
    sandbox: &str,
) -> Result<(), ProxyError> {
    let name = proxy_container_name(sandbox);
    provider.stop(&name).await?;
    provider.start(&name).await?;
    Ok(())
}

/// Re-materialises `workspace_path`'s rules and restarts the proxy container so it picks them up.
pub async fn apply_rules(
    app: &AppHandle,
    provider: &Arc<dyn MachineProvider>,
    sandbox: &str,
    workspace_path: &str,
) -> Result<(), ProxyError> {
    write_materialised_rules(app, sandbox, workspace_path)?;
    restart_proxy_container(provider, sandbox).await
}

/// Removes the proxy container, its network, and its state directory, in that order — Podman
/// refuses to remove a network still holding an attached container, and the state directory holds
/// the CA private key, so it is deleted last and unconditionally. Best-effort throughout: called
/// both from the sandbox teardown path and as a rollback after a failed create, where the resources
/// that exist so far are the ones worth retrying to remove, not a reason to fail louder.
pub async fn remove_proxy(app: &AppHandle, provider: &Arc<dyn MachineProvider>, sandbox: &str) {
    let container_name = proxy_container_name(sandbox);
    if let Err(e) = provider.remove(&container_name).await {
        tracing::warn!(target: "proxy", %sandbox, error = %e, "failed to remove proxy container");
    }
    let network = proxy_network_name(sandbox);
    if let Err(e) = provider.remove_network(&network).await {
        tracing::warn!(target: "proxy", %sandbox, error = %e, "failed to remove proxy network");
    }
    if let Ok(dir) = state_dir(app, sandbox) {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!(target: "proxy", %sandbox, error = %e, "failed to remove proxy state directory");
        }
    }
}

/// Removes every `<name>-proxy` container (and its network/state directory) whose base sandbox name
/// is absent from `live`. Podman-only: proxies never exist on the microsandbox provider.
pub async fn sweep(app: &AppHandle, registry: &Arc<ProviderRegistry>, live: &[String]) {
    let provider = registry.local(ProviderId::Podman);
    let Ok(instances) = provider.list_instances().await else {
        return;
    };
    for instance in instances {
        let Some(base) = instance.name.strip_suffix(types::PROXY_CONTAINER_SUFFIX) else {
            continue;
        };
        if live.iter().any(|n| n == base) {
            continue;
        }
        remove_proxy(app, &provider, base).await;
    }
}

/// Derives proxy status from the `<sandbox>-proxy` container's presence — never persisted
/// (Resolved Decision 3).
pub async fn status(
    app: &AppHandle,
    registry: &Arc<ProviderRegistry>,
    sandbox: &str,
) -> Result<ProxyStatus, ProxyError> {
    let provider = registry.local(ProviderId::Podman);
    let container_name = proxy_container_name(sandbox);
    let instances = provider.list_instances().await?;
    let network = proxy_network_name(sandbox);
    let Some(instance) = instances.into_iter().find(|s| s.name == container_name) else {
        return Ok(ProxyStatus { running: false, proxy_ip: String::new(), network });
    };
    Ok(ProxyStatus {
        running: instance.status == "running",
        proxy_ip: read_meta_proxy_ip(app, sandbox).unwrap_or_default(),
        network,
    })
}
