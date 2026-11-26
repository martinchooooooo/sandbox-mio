//! The sidecar container's lifecycle: create and remove (local and remote), start and reload,
//! the startup sweep, and the status derived from the container's presence.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::AppHandle;

use super::store::{meta_engine, secrets_dir, ProxyStore};
use super::types::{ProxyEngine, ProxyError, ProxyStatus, SidecarSite};
use super::remote_push::{push_secret_files, staged_secret_names};
use super::{
    host_path, io_err, iron, iron_config, is_sidecar, proxy_container_name, proxy_env,
    proxy_image, proxy_network_name,
};
use crate::activity::types::TaskKind;
use crate::activity::TaskFailure;
use mio_domain::types::ProviderId;
use mio_provider::types::{
    ExecOpts, InstanceNetwork, MachineProvider, ProxyAttachment, SidecarMount, SidecarSpec,
};
use mio_provider::ProviderRegistry;

/// The sidecar's secrets tmpfs. RAM only, so a stopped or rebooted remote sidecar holds no credential
/// (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 1).
const SECRETS_TMPFS_OPTIONS: &str = "rw,mode=0755,size=1m";

const SUBNET_INDEX_MAX: u16 = 255;

/// How long a reload keeps retrying while the management listener is not yet bound: a sidecar
/// started moments ago (the first apply at create) may not have bound it yet.
const MGMT_READY_BUDGET: Duration = Duration::from_secs(10);
const MGMT_READY_STEP: Duration = Duration::from_millis(250);

/// Asks the running sidecar to reload `config.yaml`, retrying while its management listener is not
/// yet bound. The bearer token is read from the sidecar's own `IRON_MANAGEMENT_API_KEY`, so it
/// never enters a command line.
pub(super) async fn reload(provider: &Arc<dyn MachineProvider>, sidecar: &str) -> Result<(), ProxyError> {
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

pub(super) async fn is_running(
    provider: &Arc<dyn MachineProvider>,
    sidecar: &str,
) -> Result<bool, ProxyError> {
    Ok(provider.list_instances().await?.iter().any(|s| s.name == sidecar && s.status == "running"))
}

/// Starts a stopped sidecar and confirms it stays up (see [`confirm_started`]).
pub(super) async fn start_and_confirm(
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

/// Makes sure `sandbox`'s sidecar on `site` runs, so a sandbox about to start has its network.
/// A remote sidecar gets the staged secret files pushed on every call, so a failed earlier push
/// self-heals (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decisions 1, 5).
/// A missing sidecar is a no-op, which the mock backend relies on.
pub async fn ensure_sidecar_running(
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
) -> Result<(), ProxyError> {
    let sidecar = proxy_container_name(sandbox);
    let instances = site.provider.list_instances().await?;
    let Some(instance) = instances.iter().find(|s| s.name == sidecar && is_sidecar(s)) else {
        return Ok(());
    };
    let running = instance.status == "running";
    if site.local {
        return if running { Ok(()) } else { start_and_confirm(&site.provider, &sidecar).await };
    }
    let secrets = secrets_dir(&store.state_dir(sandbox)?);
    if !running {
        site.provider.start(&sidecar).await?;
    }
    push_secret_files(site, &sidecar, &secrets, &staged_secret_names(&secrets)).await?;
    if running { Ok(()) } else { confirm_started(&site.provider, &sidecar).await }
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

/// Creates the proxy sidecar and its state directory on `provider_id` and starts it. Returns the
/// `ProxyAttachment` `SandboxBackend::create` needs to build the sandbox's own `InstanceSpec`.
/// Caller must have already run `ensure_proxy_image`.
///
/// Policy starts empty: this runs *before* `SandboxBackend::create`, so the sandbox's workspace (and
/// therefore its `.mio/net-policy.json`) does not exist yet. A caller applies it afterward through
/// `apply::apply_policy`.
pub async fn create_proxy(
    store: &ProxyStore,
    provider: &Arc<dyn MachineProvider>,
    provider_id: ProviderId,
    sandbox: &str,
) -> Result<ProxyAttachment, ProxyError> {
    match provider_id {
        ProviderId::Podman => create_podman_proxy(store, provider, sandbox).await,
    }
}

/// The dual-homed iron-proxy sidecar: pinned on the internal network, joined to Podman's default
/// `podman` network as its route out, booted with the deny-all bootstrap config until the first
/// `apply::apply_policy` (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decisions 3, 18).
async fn create_podman_proxy(
    store: &ProxyStore,
    provider: &Arc<dyn MachineProvider>,
    sandbox: &str,
) -> Result<ProxyAttachment, ProxyError> {
    let (network, proxy_ip) = allocate_network(provider, sandbox).await?;
    let dir = store.state_dir(sandbox)?;
    let certs = store.certs_dir(sandbox)?;
    iron::generate_ca(&certs)?;
    iron::write_bootstrap(&dir, &proxy_ip)?;
    let secrets = secrets_dir(&dir);
    std::fs::create_dir_all(&secrets).map_err(|e| io_err(&secrets, e))?;
    // Create-if-absent: a re-run against an existing state directory must not wipe the captured
    // history (`docs/features/proxy-traffic-viewer/02_dev.md`, Resolved Decision 5).
    let flows_path = store.flows_path(sandbox)?;
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
        tmpfs: Vec::new(),
        command: vec!["-config".to_string(), format!("{}/config.yaml", iron::CONTAINER_CONFIG_DIR)],
        labels: Vec::new(),
    };
    provider.create_sidecar(&spec).await?;
    provider.start(&container_name).await?;
    store.write_meta(
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

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<(), ProxyError> {
    std::fs::create_dir_all(dst).map_err(|e| io_err(dst, e))?;
    for entry in std::fs::read_dir(src).map_err(|e| io_err(src, e))? {
        let entry = entry.map_err(|e| io_err(src, e))?;
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| io_err(&target, e))?;
        }
    }
    Ok(())
}

/// Copies the remote staging's `config/` and `certs/` into the created, unstarted sidecar. One tree
/// is copied onto `/etc/iron-proxy`: `podman cp` cannot write into the missing parent of
/// `/etc/iron-proxy/config`, but creates the destination itself when only its parent exists
/// (`docs/features/proxy-remote-deploy/spike/README.md`, check 1).
async fn copy_state_in(
    provider: &Arc<dyn MachineProvider>,
    container: &str,
    dir: &std::path::Path,
) -> Result<(), ProxyError> {
    let stage = dir.join("stage");
    let _ = std::fs::remove_dir_all(&stage);
    let staged = copy_dir(&dir.join("config"), &stage.join("config"))
        .and_then(|()| copy_dir(&dir.join("certs"), &stage.join("certs")));
    let copied = match staged {
        Ok(()) => provider
            .copy_in(container, &stage.to_string_lossy(), iron::CONTAINER_STATE_DIR)
            .await
            .map_err(ProxyError::from),
        Err(e) => Err(e),
    };
    let _ = std::fs::remove_dir_all(&stage);
    copied
}

/// Creates the remote network and the dual-homed remote sidecar on `site`, with a fresh CA, a
/// deny-all config and a RAM-only secrets directory (`docs/features/proxy-remote-deploy/02_dev.md`,
/// Resolved Decisions 1, 8, 9). Removes what it created when it fails; the caller has run
/// `ensure_proxy_image` and refused a name collision on `site`.
pub async fn create_remote_proxy(
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
) -> Result<ProxyAttachment, ProxyError> {
    let (network, proxy_ip) = allocate_network(&site.provider, sandbox).await?;
    let created = create_remote_sidecar(store, site, sandbox, &network, &proxy_ip).await;
    if created.is_err() {
        remove_sidecar_and_network(&site.provider, sandbox).await;
        store.remove_remote_state(sandbox);
    }
    created
}

async fn create_remote_sidecar(
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
    network: &str,
    proxy_ip: &str,
) -> Result<ProxyAttachment, ProxyError> {
    let (network, proxy_ip) = (network.to_string(), proxy_ip.to_string());
    let dir = store.remote_dir(sandbox)?;
    let certs = dir.join("certs");
    std::fs::create_dir_all(&certs).map_err(|e| io_err(&certs, e))?;
    iron::generate_ca(&certs)?;
    iron::write_bootstrap(&dir, &proxy_ip)?;
    let container_name = proxy_container_name(sandbox);
    let spec = SidecarSpec {
        name: container_name.clone(),
        image: iron::iron_proxy_image(),
        network: Some(InstanceNetwork { name: network.clone(), ip: Some(proxy_ip.clone()), dns: None }),
        extra_networks: vec!["podman".to_string()],
        log_max_size: Some(iron::SIDECAR_LOG_MAX_SIZE.to_string()),
        publish: Vec::new(),
        env: vec![(iron_config::MGMT_API_KEY_ENV.to_string(), uuid::Uuid::new_v4().simple().to_string())],
        mounts: Vec::new(),
        tmpfs: vec![format!("{}:{SECRETS_TMPFS_OPTIONS}", iron_config::CONTAINER_SECRETS_DIR)],
        command: vec!["-config".to_string(), format!("{}/config.yaml", iron::CONTAINER_CONFIG_DIR)],
        labels: Vec::new(),
    };
    site.provider.create_sidecar(&spec).await?;
    // Order is load-bearing: the files must be in the container before its first start, or iron-proxy
    // exits on a missing config.
    copy_state_in(&site.provider, &container_name, &dir).await?;
    site.provider.start(&container_name).await?;
    store.write_remote_meta(sandbox, &site.machine.id, &network, &proxy_ip)?;
    Ok(ProxyAttachment::PodmanBridge { network, env: proxy_env(&proxy_ip), dns: proxy_ip })
}

/// Removes the remote sidecar and its network from `site`, best-effort. Never touches the laptop's
/// state directory: the caller removes `remote/` with [`ProxyStore::remove_remote_state`].
pub async fn remove_remote_proxy(site: &SidecarSite, sandbox: &str) {
    remove_sidecar_and_network(&site.provider, sandbox).await;
}

async fn remove_sidecar_and_network(provider: &Arc<dyn MachineProvider>, sandbox: &str) {
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
    if let Err(e) = provider.remove_network(&proxy_network_name(sandbox)).await {
        tracing::warn!(target: "proxy", %sandbox, error = %e, "failed to remove proxy network");
    }
}

/// The sidecar's provider id and handle, resolved from `meta.json`.
pub fn sidecar_provider(
    store: &ProxyStore,
    registry: &Arc<ProviderRegistry>,
    sandbox: &str,
) -> (ProviderId, Arc<dyn MachineProvider>) {
    let id = store.sidecar_provider_id(sandbox);
    (id, registry.local(id))
}

/// Fails unless `sandbox`'s local sidecar exists on the provider its `meta.json` names. A failed
/// listing is an error too, so a deploy never proceeds on an unverified boundary.
pub async fn require_sidecar(
    store: &ProxyStore,
    registry: &Arc<ProviderRegistry>,
    sandbox: &str,
) -> Result<(), ProxyError> {
    let (_, provider) = sidecar_provider(store, registry, sandbox);
    let instances = provider.list_instances().await?;
    if instances.iter().any(|s| s.name == proxy_container_name(sandbox) && is_sidecar(s)) {
        return Ok(());
    }
    Err(ProxyError::CommandFailed(format!(
        "{sandbox:?} has no network proxy sidecar; recreate the sandbox to get the network boundary"
    )))
}

/// Restarts the proxy sidecar so it re-reads its config. A remote sidecar restarts through
/// [`ensure_sidecar_running`], because its secrets tmpfs is empty after any start.
pub async fn restart_proxy_container(
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
) -> Result<(), ProxyError> {
    let name = proxy_container_name(sandbox);
    // A crashed sidecar may refuse the stop, so only the start decides the outcome.
    let stopped = site.provider.stop_sidecar(&name).await;
    if !site.local {
        return ensure_sidecar_running(store, site, sandbox).await;
    }
    match site.provider.start(&name).await {
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
    store: &ProxyStore,
    provider: &Arc<dyn MachineProvider>,
    provider_id: ProviderId,
    sandbox: &str,
) {
    match provider_id {
        ProviderId::Podman => remove_sidecar_and_network(provider, sandbox).await,
    }
    store.remove_state(sandbox);
}

/// Removes every `<name>-proxy` sidecar (and its network/state directory) whose base sandbox name
/// is absent from `live`.
pub async fn sweep(store: &ProxyStore, registry: &Arc<ProviderRegistry>, live: &[String]) {
    for (id, provider) in registry.all() {
        let Ok(instances) = provider.list_instances().await else {
            continue;
        };
        for instance in instances.iter().filter(|s| is_sidecar(s)) {
            let Some(base) = instance.name.strip_suffix(mio_domain::PROXY_CONTAINER_SUFFIX) else {
                continue;
            };
            if live.iter().any(|n| n == base) {
                continue;
            }
            remove_proxy(store, &provider, id, base).await;
        }
    }
}

/// Derives proxy status from the `<sandbox>-proxy` container's presence on `site` — never persisted
/// (Resolved Decision 3). A deployed sandbox without a remote sidecar reports no sidecar and no host.
pub async fn status(
    store: &ProxyStore,
    site: &SidecarSite,
    sandbox: &str,
) -> Result<ProxyStatus, ProxyError> {
    let id = site.machine.provider;
    let container_name = proxy_container_name(sandbox);
    let network = proxy_network_name(sandbox);
    let (meta, host) = if site.local {
        (store.read_meta(sandbox).unwrap_or_default(), None)
    } else {
        match store.read_remote_meta(sandbox) {
            Some(meta) => (meta, Some(site.machine.id.clone())),
            None => {
                return Ok(ProxyStatus {
                    exists: false,
                    running: false,
                    proxy_ip: String::new(),
                    network,
                    provider: id,
                    engine: ProxyEngine::IronProxy,
                    host: None,
                });
            }
        }
    };
    let engine = meta_engine(&meta);
    let instances = site.provider.list_instances().await?;
    let Some(instance) = instances.into_iter().find(|s| s.name == container_name && is_sidecar(s)) else {
        return Ok(ProxyStatus {
            exists: false,
            running: false,
            proxy_ip: String::new(),
            network,
            provider: id,
            engine,
            host,
        });
    };
    let running = instance.status == "running";
    Ok(ProxyStatus {
        exists: true,
        running,
        proxy_ip: meta["proxyIp"].as_str().unwrap_or_default().to_string(),
        network: meta["network"].as_str().map_or(network, str::to_string),
        provider: id,
        engine,
        host,
    })
}
