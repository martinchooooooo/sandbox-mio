//! Per-sandbox mitmproxy sidecar: traffic inspection and secret injection
//! (`docs/features/network-proxy/02_dev.md`). Free functions taking `&Arc<dyn MachineProvider>`
//! and `&AppHandle`, modelled on `crate::credentials` rather than the trait+mock subsystem
//! pattern — every external call already crosses the `ProcessRunner` seam through the provider, so
//! the e2e `light` tier is covered with no new trait.
//!
//! Provider-aware: a Podman sidecar joins the sandbox on a bridge network, a microsandbox sidecar is
//! a second VM the agent chains to over host loopback
//! (`docs/features/microsandbox-network-policy/02_dev.md`, Phase F). Which provider a sidecar runs
//! on is recorded in its `meta.json`; state written before that feature is Podman's.

pub mod flowlog;
pub mod rules;
pub mod types;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::AppHandle;

use crate::activity::types::TaskKind;
use crate::activity::TaskFailure;
use crate::provider::msb_policy::{SOCKS_PASSWORD_ENV, SOCKS_USERNAME};
use crate::provider::types::{
    InstanceNetwork, MachineProvider, PortPublish, ProviderError, ProviderId, SidecarMount,
    SidecarReadiness, SidecarSpec,
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

/// The SOCKS5 port mitmdump listens on inside a microsandbox sidecar.
const MSB_SOCKS_GUEST_PORT: u16 = 1080;

/// `create_proxy` retries a microsandbox sidecar on a fresh port this many times
/// (`docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 8).
const PORT_ATTEMPTS: usize = 3;

/// Bodies above this size are streamed, not buffered: a buffered ~150 MB download OOM-killed a
/// 512 MiB msb sidecar.
const STREAM_LARGE_BODIES: &str = "stream_large_bodies=1m";

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

/// The flow-log addon script, embedded and refreshed at every `create_proxy` for the same reason
/// as `ADDON_SCRIPT` (`docs/features/proxy-traffic-viewer/02_dev.md`).
const FLOWLOG_SCRIPT: &str = include_str!("../../resources/mitm_flowlog_addon.py");

/// The NDJSON flow log `mitm_flowlog_addon.py` appends to and `flowlog::watch_flows` polls. A
/// microsandbox sidecar writes it inside its `flows/` directory mount; a Podman sidecar keeps its
/// single-file mount at the state root.
pub fn flows_path(app: &AppHandle, sandbox: &str) -> Result<PathBuf, ProxyError> {
    let dir = state_dir(app, sandbox)?;
    Ok(match sidecar_provider_id(app, sandbox) {
        ProviderId::Microsandbox => dir.join("flows").join("flows.ndjson"),
        ProviderId::Podman => dir.join("flows.ndjson"),
    })
}

/// The provider `sandbox`'s sidecar runs on, from its `meta.json`; Podman when none records it.
pub fn sidecar_provider_id(app: &AppHandle, sandbox: &str) -> ProviderId {
    let Some(meta) = meta_path_if_exists(app, sandbox).and_then(|p| crate::mio_files::read_value(&p))
    else {
        return ProviderId::Podman;
    };
    match meta["provider"].as_str() {
        Some("msb") => ProviderId::Microsandbox,
        _ => ProviderId::Podman,
    }
}

/// `meta.json`'s path when it exists, without creating the state directory as a side effect.
fn meta_path_if_exists(app: &AppHandle, sandbox: &str) -> Option<PathBuf> {
    let path = crate::paths::app_data_root(app).ok()?.join("proxy").join(sandbox).join("meta.json");
    path.is_file().then_some(path)
}

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

/// Creates the proxy sidecar and its state directory on `provider_id`, starts it, and waits for its
/// CA to appear. Returns the `ProxyAttachment` `SandboxBackend::create` needs to build the
/// sandbox's own `InstanceSpec`. Caller must have already run `ensure_proxy_image`.
///
/// Rules start empty: this runs *before* `SandboxBackend::create`, so the sandbox's workspace (and
/// therefore its `.mio/proxy-rules.json`) does not exist yet. A caller adds rules afterward through
/// `apply_rules` (Podman) or `write_secret_hosts` (microsandbox).
pub async fn create_proxy(
    app: &AppHandle,
    provider: &Arc<dyn MachineProvider>,
    provider_id: ProviderId,
    sandbox: &str,
) -> Result<ProxyAttachment, ProxyError> {
    match provider_id {
        ProviderId::Podman => create_podman_proxy(app, provider, sandbox).await,
        ProviderId::Microsandbox => create_msb_proxy(app, provider, sandbox).await,
    }
}

async fn create_podman_proxy(
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
    let flowlog_path = dir.join("flowlog_addon.py");
    std::fs::write(&flowlog_path, FLOWLOG_SCRIPT)
        .map_err(|e| ProxyError::Io { path: flowlog_path.to_string_lossy().to_string(), error: e.to_string() })?;
    // Create-if-absent, unlike `rules.json` above: a re-run against an existing state directory
    // must not wipe the captured history (Resolved Decision 5).
    let flows_path = flows_path(app, sandbox)?;
    if !flows_path.exists() {
        std::fs::write(&flows_path, b"")
            .map_err(|e| ProxyError::Io { path: flows_path.to_string_lossy().to_string(), error: e.to_string() })?;
    }

    let container_name = proxy_container_name(sandbox);
    let spec = SidecarSpec {
        name: container_name.clone(),
        image: mitmproxy_image(),
        network: Some(InstanceNetwork { name: network.clone(), ip: Some(proxy_ip.clone()) }),
        publish: Vec::new(),
        env: Vec::new(),
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
            SidecarMount {
                host_path: host_path(&flowlog_path),
                dest: "/addons/flowlog_addon.py".to_string(),
                read_only: true,
            },
            SidecarMount {
                host_path: host_path(&flows_path),
                dest: "/addons/flows.ndjson".to_string(),
                read_only: false,
            },
        ],
        command: vec![
            "mitmdump".to_string(),
            "--listen-host".to_string(),
            "0.0.0.0".to_string(),
            "--listen-port".to_string(),
            PROXY_PORT.to_string(),
            "--set".to_string(),
            STREAM_LARGE_BODIES.to_string(),
            "-s".to_string(),
            "/addons/addon.py".to_string(),
            "-s".to_string(),
            "/addons/flowlog_addon.py".to_string(),
        ],
        labels: Vec::new(),
    };
    provider.create_sidecar(&spec).await?;
    provider.start(&container_name).await?;
    wait_for_ca(app, sandbox).await?;
    write_meta(app, sandbox, &serde_json::json!({ "network": network, "proxyIp": proxy_ip }))?;

    Ok(ProxyAttachment::PodmanBridge { network, env: proxy_env(&proxy_ip) })
}

fn io_err(path: &std::path::Path, e: std::io::Error) -> ProxyError {
    ProxyError::Io { path: path.to_string_lossy().to_string(), error: e.to_string() }
}

/// An OS-assigned free port on host loopback. Duplicates `allocate_ephemeral_port` in
/// `src-tauri/src/network/mod.rs`, because subsystems never depend on each other.
fn allocate_socks_port() -> Result<u16, ProxyError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| ProxyError::CommandFailed(format!("no free loopback port: {e}")))?;
    listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| ProxyError::CommandFailed(format!("no free loopback port: {e}")))
}

/// An htpasswd line mitmproxy's `--proxyauth @file` accepts, so the sidecar's msb config holds a
/// path and the file holds a hash, never the plaintext (Resolved Decision 7).
fn htpasswd_line(username: &str, password: &str) -> String {
    use base64::Engine;
    use sha1::Digest;
    let digest = sha1::Sha1::digest(password.as_bytes());
    format!("{username}:{{SHA}}{}\n", base64::engine::general_purpose::STANDARD.encode(digest))
}

fn msb_readiness(host_port: u16, password: &str) -> SidecarReadiness {
    SidecarReadiness {
        host_port,
        socks5_auth: Some((SOCKS_USERNAME.to_string(), password.to_string())),
        budget_ms: crate::provider::probe::DEFAULT_BUDGET_MS,
    }
}

/// A microsandbox sidecar VM: addons read-only, flows and the mitmproxy confdir read-write, SOCKS5
/// published on a fresh loopback port. `certs/` survives a recreate, since every agent chained to
/// the sidecar pins its CA (`docs/features/microsandbox-network-policy/02_dev.md`, Phase F).
async fn create_msb_proxy(
    app: &AppHandle,
    provider: &Arc<dyn MachineProvider>,
    sandbox: &str,
) -> Result<ProxyAttachment, ProxyError> {
    let dir = state_dir(app, sandbox)?;
    let certs = certs_dir(app, sandbox)?;
    let addons = dir.join("addons");
    let flows = dir.join("flows");
    for d in [&addons, &flows] {
        std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
    }
    let flowlog_path = addons.join("flowlog_addon.py");
    std::fs::write(&flowlog_path, FLOWLOG_SCRIPT).map_err(|e| io_err(&flowlog_path, e))?;
    let rules_path = addons.join("rules.json");
    crate::mio_files::write_compact(&rules_path, &serde_json::Value::Array(Vec::new()))
        .map_err(|e| ProxyError::CommandFailed(e.to_string()))?;
    let password = uuid::Uuid::new_v4().simple().to_string();
    let htpasswd = addons.join("htpasswd");
    std::fs::write(&htpasswd, htpasswd_line(SOCKS_USERNAME, &password))
        .map_err(|e| io_err(&htpasswd, e))?;
    // Create-if-absent: a re-run against an existing state directory keeps the captured history.
    let flows_file = flows.join("flows.ndjson");
    if !flows_file.exists() {
        std::fs::write(&flows_file, b"").map_err(|e| io_err(&flows_file, e))?;
    }
    let ca = ca_pem_path(app, sandbox)?;
    if crate::backends::mock_backend_enabled() && !ca.exists() {
        // Stands in for mitmdump's first start, which never happens on the light tier.
        std::fs::write(&ca, b"-----BEGIN CERTIFICATE-----\nmock\n-----END CERTIFICATE-----\n")
            .map_err(|e| io_err(&ca, e))?;
    }

    let name = proxy_container_name(sandbox);
    let mut last_err = None;
    for _ in 0..PORT_ATTEMPTS {
        let host_port = allocate_socks_port()?;
        // Written before the VM exists so the startup sweep can always attribute it to this app's
        // data root (Resolved Decision 20).
        write_meta(
            app,
            sandbox,
            &serde_json::json!({ "provider": "msb", "hostPort": host_port, "socksPassword": password }),
        )?;
        let spec = SidecarSpec {
            name: name.clone(),
            image: mitmproxy_image(),
            network: None,
            mounts: vec![
                SidecarMount {
                    host_path: host_path(&certs),
                    dest: "/home/mitmproxy/.mitmproxy".to_string(),
                    read_only: false,
                },
                SidecarMount { host_path: host_path(&addons), dest: "/addons".to_string(), read_only: true },
                SidecarMount { host_path: host_path(&flows), dest: "/flows".to_string(), read_only: false },
            ],
            publish: vec![PortPublish {
                bind: "127.0.0.1".to_string(),
                host_port,
                guest_port: MSB_SOCKS_GUEST_PORT,
            }],
            env: vec![
                ("MIO_FLOWS_PATH".to_string(), "/flows/flows.ndjson".to_string()),
                ("MIO_RULES_PATH".to_string(), "/addons/rules.json".to_string()),
            ],
            command: vec![
                "mitmdump".to_string(),
                "--set".to_string(),
                "confdir=/home/mitmproxy/.mitmproxy".to_string(),
                "--set".to_string(),
                STREAM_LARGE_BODIES.to_string(),
                "--mode".to_string(),
                format!("socks5@0.0.0.0:{MSB_SOCKS_GUEST_PORT}"),
                "--proxyauth".to_string(),
                "@/addons/htpasswd".to_string(),
                "-s".to_string(),
                "/addons/flowlog_addon.py".to_string(),
            ],
            labels: Vec::new(),
        };
        match provider.create_sidecar(&spec).await {
            Ok(_) => {
                provider.start(&name).await?;
                wait_for_ca(app, sandbox).await?;
                crate::provider::probe::wait_sidecar_ready(&msb_readiness(host_port, &password)).await?;
                return Ok(ProxyAttachment::MsbChain {
                    host_port,
                    password_env: SOCKS_PASSWORD_ENV.to_string(),
                    upstream_ca_path: host_path(&ca),
                });
            }
            Err(e @ ProviderError::PortInUse(_)) => last_err = Some(e),
            Err(e) => return Err(e.into()),
        }
    }
    Err(last_err.map(ProxyError::from).unwrap_or_else(|| {
        ProxyError::CommandFailed("no free port for the proxy sidecar".to_string())
    }))
}

/// The chain attachment for an existing microsandbox sidecar, rebuilt from its `meta.json` and
/// `certs/` so a recreated agent pins the same port and CA.
pub fn msb_attachment(app: &AppHandle, sandbox: &str) -> Result<ProxyAttachment, ProxyError> {
    let host_port = read_meta_host_port(app, sandbox)
        .ok_or_else(|| ProxyError::CommandFailed(format!("no proxy port recorded for {sandbox}")))?;
    let ca = ca_pem_path(app, sandbox)?;
    if !ca.exists() {
        return Err(ProxyError::CommandFailed(format!("{} is missing", ca.to_string_lossy())));
    }
    Ok(ProxyAttachment::MsbChain {
        host_port,
        password_env: SOCKS_PASSWORD_ENV.to_string(),
        upstream_ca_path: host_path(&ca),
    })
}

/// Marks flows to every host a secret is bound to as `secretInjected`, through the flow-log
/// addon's existing host matcher — "a secret is bound to this host", since mitmproxy cannot see
/// microsandbox's substitution (Resolved Decision 10). Takes effect at the sidecar's next start.
pub fn write_secret_hosts(
    app: &AppHandle,
    sandbox: &str,
    applied: &crate::net_policy::AppliedPolicy,
) -> Result<(), ProxyError> {
    let mut hosts: Vec<&str> = Vec::new();
    for binding in &applied.policy.secrets {
        for h in &binding.hosts {
            if !hosts.contains(&h.as_str()) {
                hosts.push(h);
            }
        }
    }
    let entries: Vec<serde_json::Value> =
        hosts.iter().map(|h| serde_json::json!({ "host": h, "path_prefix": "" })).collect();
    let path = state_dir(app, sandbox)?.join("addons").join("rules.json");
    crate::mio_files::write_compact(&path, &serde_json::Value::Array(entries))
        .map_err(|e| ProxyError::CommandFailed(e.to_string()))
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

fn read_meta_host_port(app: &AppHandle, sandbox: &str) -> Option<u16> {
    let path = meta_path_if_exists(app, sandbox)?;
    crate::mio_files::read_value(&path)?["hostPort"].as_u64().and_then(|p| u16::try_from(p).ok())
}

/// The chained-proxy SOCKS5 password a microsandbox sidecar's `meta.json` records, read by
/// `services::start_env::AppStartEnv` for the agent's start environment.
pub fn read_socks_password(app: &AppHandle, sandbox: &str) -> Option<String> {
    let path = meta_path_if_exists(app, sandbox)?;
    crate::mio_files::read_value(&path)?["socksPassword"].as_str().map(str::to_string)
}

fn read_meta_proxy_ip(app: &AppHandle, sandbox: &str) -> Option<String> {
    let path = meta_path(app, sandbox).ok()?;
    crate::mio_files::read_value(&path)?["proxyIp"].as_str().map(str::to_string)
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
/// versioned `sandbox-mio/base:<n>` image and so carry an `image_version`; the mitmproxy image's
/// `11.1.3` tag does not parse as one.
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

/// Restarts the proxy sidecar so it re-reads its mounted `rules.json` — the addon has no built-in
/// watch on a mounted file (Resolved Decision 11). A microsandbox sidecar is force-stopped and
/// probed again before returning, since its agent fails closed until the listener is back.
pub async fn restart_proxy_container(
    app: &AppHandle,
    registry: &Arc<ProviderRegistry>,
    sandbox: &str,
) -> Result<(), ProxyError> {
    let (id, provider) = sidecar_provider(app, registry, sandbox);
    let name = proxy_container_name(sandbox);
    // A crashed sidecar may refuse the stop, so only the start decides the outcome.
    let stopped = provider.stop_sidecar(&name).await;
    match provider.start(&name).await {
        Ok(()) => {}
        Err(ProviderError::PortInUse(_)) => {
            return Err(ProxyError::CommandFailed(
                "proxy port taken — re-apply the network settings to recreate".to_string(),
            ))
        }
        Err(e) => return Err(stopped.err().unwrap_or(e).into()),
    }
    if id == ProviderId::Microsandbox {
        let port = read_meta_host_port(app, sandbox)
            .ok_or_else(|| ProxyError::CommandFailed(format!("no proxy port recorded for {sandbox}")))?;
        let password = read_socks_password(app, sandbox).unwrap_or_default();
        crate::provider::probe::wait_sidecar_ready(&msb_readiness(port, &password)).await?;
    }
    Ok(())
}

/// Re-materialises `workspace_path`'s rules and restarts the proxy container so it picks them up.
/// Podman only: microsandbox owns secret injection itself.
pub async fn apply_rules(
    app: &AppHandle,
    registry: &Arc<ProviderRegistry>,
    sandbox: &str,
    workspace_path: &str,
) -> Result<(), ProxyError> {
    write_materialised_rules(app, sandbox, workspace_path)?;
    restart_proxy_container(app, registry, sandbox).await
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
/// is absent from `live`. A microsandbox sidecar is swept only when this app's data root holds its
/// `meta.json`: the msb store is host-global, and another worktree's sidecar exists before its
/// agent does (`docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 20).
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
            if id == ProviderId::Microsandbox && sidecar_provider_id(app, base) != id {
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
    let network = if id == ProviderId::Podman { proxy_network_name(sandbox) } else { String::new() };
    let host_port = if id == ProviderId::Microsandbox { read_meta_host_port(app, sandbox) } else { None };
    let Some(instance) = instances.into_iter().find(|s| s.name == container_name && is_sidecar(s)) else {
        return Ok(ProxyStatus {
            exists: false,
            running: false,
            proxy_ip: String::new(),
            network,
            provider: id,
            host_port,
            healthy: None,
        });
    };
    let running = instance.status == "running";
    let healthy = match (id, host_port, running) {
        (ProviderId::Microsandbox, Some(port), true) => {
            let password = read_socks_password(app, sandbox).unwrap_or_default();
            Some(crate::provider::probe::sidecar_healthy(&msb_readiness(port, &password)).await)
        }
        _ => None,
    };
    Ok(ProxyStatus {
        exists: true,
        running,
        proxy_ip: read_meta_proxy_ip(app, sandbox).unwrap_or_default(),
        network,
        provider: id,
        host_port,
        healthy,
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
            provider: ProviderId::Microsandbox,
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

    /// The exact line real mitmproxy 11.1.3 accepted for `pw-123` on 0.7.12 (Phase F verification).
    #[test]
    fn htpasswd_line_is_an_apache_sha_entry() {
        assert_eq!(htpasswd_line("mio", "pw-123"), "mio:{SHA}aKAUJxJMDStxTTMOHvkBWjDBPVY=\n");
    }
}
