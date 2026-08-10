//! Mutagen-forward lifecycle and port allocation for terminal/agent-web and user-exposed ports.

pub mod types;

mod mock;

use std::collections::HashMap;
use std::process::Output;
use std::sync::Arc;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter};

use mock::MockNetworkManager;
use types::{ForwardInfo, NetworkError, NetworkManager, ServiceKind};

use crate::mockctl::types::ProcessRunner;
use crate::provider::types::{ExecOpts, MachineProvider};

pub use types::EVENT_NAME;

pub const RELAY_PORT_POOL_START: u16 = 40000;

pub const RELAY_POOL_SIZE: u16 = 8;

pub fn relay_pool_publish_args() -> Vec<String> {
    (RELAY_PORT_POOL_START..RELAY_PORT_POOL_START + RELAY_POOL_SIZE)
        .map(|p| format!("127.0.0.1::{p}"))
        .collect()
}

fn forward_name(sandbox: &str, kind: ServiceKind) -> String {
    match kind {
        ServiceKind::Terminal => format!("terminal-{sandbox}"),
        ServiceKind::AgentWeb => format!("agentweb-{sandbox}"),
        ServiceKind::Exposed(port) => format!("expose-{sandbox}-{port}"),
    }
}

/// Whether an `Exposed` forward went through a relay-pool slot and so has a `socat` to kill.
/// Derived rather than stored: only the relay branch of `expose_port` records a `remote_port`
/// different from the target port (an exec-capable forward reaches `target_port` directly).
fn is_relayed(entry: &ForwardInfo, target_port: u16) -> bool {
    entry.remote_port != target_port
}

fn allocate_ephemeral_port() -> Result<u16, NetworkError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| NetworkError::PortAllocation(e.to_string()))?;
    listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| NetworkError::PortAllocation(e.to_string()))
}

fn check(out: Output) -> Result<String, NetworkError> {
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(NetworkError::CommandFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

/// If `workspace_path`'s `.mio/ports.json` doesn't exist yet, create it as an empty registry
/// (`{}`), so exposing a port from the UI also bootstraps the contract file `watch_ports_file`
/// reads — giving a script inside the container a file to add entries to. The manually exposed
/// port is deliberately NOT written as an entry: a declared entry is *file-managed* (the poll loop
/// auto-exposes it and auto-unexposes it when the entry disappears), whereas a manually exposed
/// port must stay independently closeable from the UI. Declaring it here would make the very next
/// `poll_ports_file` tick re-expose it right after a manual close (it's still in the file but no
/// longer in the live forwards map), so the close could never stick. Best-effort and
/// non-destructive: a file that already exists is left untouched.
fn ensure_ports_file(workspace_path: &str) {
    let mio_dir = std::path::Path::new(workspace_path).join(".mio");
    let path = mio_dir.join("ports.json");
    if path.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&mio_dir).and_then(|_| std::fs::write(&path, "{}"));
}

/// Drives the bundled Mutagen sidecar (via `crate::sync`) plus direct `podman exec`/`podman port`
/// calls for the relay-pool machinery. Holds the [`AppHandle`] the sidecar is resolved through.
pub struct MutagenNetworkManager {
    app: AppHandle,
    forwards: Mutex<HashMap<(String, ServiceKind), ForwardInfo>>,
    /// Serializes `expose_port` end-to-end (slot selection through registry insert) so two
    /// concurrent calls for the same sandbox cannot race onto the same relay-pool slot.
    expose_lock: tokio::sync::Mutex<()>,
    /// One poll-loop task per sandbox currently being watched by `watch_ports_file`, so a second
    /// call for the same sandbox is a no-op and `close_sandbox` can cancel it.
    ports_watch: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// `podman port`-based lookup, used for a sandbox on the publish+relay reachability model. An
    /// exec-capable sandbox's `expose_port` branch needs no lookup at all (the container's own
    /// loopback is reached directly), so unlike `PodmanMiseRuntime` there is no second
    /// `exec_port_transport` field here — see `docs/features/podman-exec-transport/02_dev.md`.
    published_port_transport: std::sync::Arc<dyn crate::transport::types::PortTransport>,
    /// The process seam from which a per-call, target-pinned provider is built. Each method drives an
    /// explicit `connection`/`entry.connection`, so the provider is constructed per call rather than
    /// held as a single field — mirroring `MiseRuntime`'s `provider_on`.
    runner: Arc<dyn ProcessRunner>,
}

impl MutagenNetworkManager {
    /// A provider pinned to an arbitrary `connection`, mirroring `MiseRuntime::provider_on` and
    /// `deploy`'s target-pinned provider.
    fn provider_on(&self, connection: &str) -> Arc<dyn MachineProvider> {
        crate::provider::default_provider(self.runner.clone(), connection.to_string())
    }

    /// Push the full current snapshot to `EVENT_NAME` so `state.svelte.ts` stays live. Called after
    /// every mutation below instead of making the frontend refetch via `list_open_forwards`.
    fn emit_forwards(&self) {
        let snapshot: Vec<ForwardInfo> = self.forwards.lock().unwrap().values().cloned().collect();
        let _ = self.app.emit(EVENT_NAME, &snapshot);
    }

    fn free_slot(&self, sandbox: &str) -> Option<u16> {
        let used: std::collections::HashSet<u16> = self
            .forwards
            .lock()
            .unwrap()
            .values()
            .filter(|f| f.sandbox == sandbox)
            .filter_map(|f| match f.kind {
                ServiceKind::Exposed(_) => Some(f.remote_port),
                _ => None,
            })
            .collect();
        (RELAY_PORT_POOL_START..RELAY_PORT_POOL_START + RELAY_POOL_SIZE)
            .find(|slot| !used.contains(slot))
    }
}

#[async_trait::async_trait]
impl NetworkManager for MutagenNetworkManager {
    async fn open_forward(
        &self,
        sandbox: &str,
        kind: ServiceKind,
        connection: &str,
        remote_port: u16,
    ) -> Result<u16, NetworkError> {
        let name = forward_name(sandbox, kind);
        let host_port = allocate_ephemeral_port()?;
        crate::sync::forward_create(&self.app, &name, host_port, connection, remote_port).await?;
        self.forwards.lock().unwrap().insert(
            (sandbox.to_string(), kind),
            ForwardInfo {
                sandbox: sandbox.to_string(),
                kind,
                host_port,
                connection: connection.to_string(),
                remote_port,
                forward_name: name,
                label: None,
                auto_detected: false,
            },
        );
        self.emit_forwards();
        Ok(host_port)
    }

    async fn close_forward(&self, sandbox: &str, kind: ServiceKind) {
        let entry = self.forwards.lock().unwrap().remove(&(sandbox.to_string(), kind));
        if let Some(entry) = entry {
            crate::sync::forward_terminate(&self.app, &entry.forward_name).await;
            self.emit_forwards();
        }
    }

    async fn close_sandbox(&self, sandbox: &str) {
        if let Some(handle) = self.ports_watch.lock().unwrap().remove(sandbox) {
            handle.abort();
        }
        let entries: Vec<ForwardInfo> = {
            let mut forwards = self.forwards.lock().unwrap();
            let keys: Vec<_> = forwards
                .keys()
                .filter(|(s, _)| s == sandbox)
                .cloned()
                .collect();
            keys.into_iter().filter_map(|k| forwards.remove(&k)).collect()
        };
        if entries.is_empty() {
            return;
        }
        for entry in &entries {
            crate::sync::forward_terminate(&self.app, &entry.forward_name).await;
            if let ServiceKind::Exposed(port) = entry.kind {
                if is_relayed(entry, port) {
                    // Best-effort: the container may already be gone (destroy) or stopped
                    // (undeploy), either of which already kills the relay process for free.
                    let pattern = format!("TCP-LISTEN:{}", entry.remote_port);
                    let _ = self
                        .provider_on(&entry.connection)
                        .exec(sandbox, &["pkill", "-f", &pattern], ExecOpts::default())
                        .await;
                }
            }
        }
        self.emit_forwards();
    }

    async fn list_forwards(&self) -> Vec<ForwardInfo> {
        self.forwards.lock().unwrap().values().cloned().collect()
    }

    async fn sweep(&self, live_sandboxes: &[String]) {
        crate::sync::sweep_stale_forwards(&self.app, live_sandboxes).await;
    }

    async fn expose_port(
        &self,
        sandbox: &str,
        connection: &str,
        workspace_path: &str,
        target_port: u16,
        label: Option<String>,
        auto_detected: bool,
        exec_capable: bool,
    ) -> Result<u16, NetworkError> {
        let _guard = self.expose_lock.lock().await;
        let kind = ServiceKind::Exposed(target_port);

        // `poll_ports_file` picks its scan candidates before taking this lock, so a manual or
        // file-declared expose can land in the gap. Re-checking here keeps the deliberate forward
        // (and its "Stop exposing" button) instead of demoting it to an auto-detected one.
        if auto_detected {
            let existing = self.forwards.lock().unwrap().get(&(sandbox.to_string(), kind)).map(|f| f.host_port);
            if let Some(host_port) = existing {
                return Ok(host_port);
            }
        }

        let name = forward_name(sandbox, kind);
        let host_port = allocate_ephemeral_port()?;
        let provider = self.provider_on(connection);

        let remote_port = if exec_capable {
            // No relay-pool slot, no socat: the container's own loopback is reachable directly
            // over the exec-transport `Proxied` alias once `sandbox` is running.
            provider.start(sandbox).await?;
            crate::sync::forward_create(&self.app, &name, host_port, sandbox, target_port).await?;
            target_port
        } else {
            let slot = self.free_slot(sandbox).ok_or_else(|| NetworkError::NoFreeSlot {
                sandbox: sandbox.to_string(),
                limit: RELAY_POOL_SIZE as usize,
            })?;

            provider.start(sandbox).await?;

            // `fork` handles one client per accepted connection; `reuseaddr` lets a restarted relay
            // rebind immediately after a prior one was killed. The relay runs inside the same netns
            // as the user's server, so dialing its own loopback reaches it regardless of whether
            // that server bound `127.0.0.1` or `0.0.0.0`.
            let listen_spec = format!("TCP-LISTEN:{slot},fork,reuseaddr");
            let dial_spec = format!("TCP:127.0.0.1:{target_port}");
            check(
                provider
                    .exec(
                        sandbox,
                        &["socat", &listen_spec, &dial_spec],
                        ExecOpts { detached: true, ..Default::default() },
                    )
                    .await?,
            )?;

            let slot_host_port = self
                .published_port_transport
                .resolve_remote_port(sandbox, connection, slot)
                .await
                .map_err(|e| NetworkError::CommandFailed(e.to_string()))?;

            crate::sync::forward_create(&self.app, &name, host_port, connection, slot_host_port).await?;
            slot
        };

        self.forwards.lock().unwrap().insert(
            (sandbox.to_string(), kind),
            ForwardInfo {
                sandbox: sandbox.to_string(),
                kind,
                host_port,
                connection: connection.to_string(),
                remote_port,
                forward_name: name,
                label: label.clone(),
                auto_detected,
            },
        );
        self.emit_forwards();
        ensure_ports_file(workspace_path);
        Ok(host_port)
    }

    async fn unexpose_port(&self, sandbox: &str, target_port: u16) {
        let kind = ServiceKind::Exposed(target_port);
        let entry = self.forwards.lock().unwrap().remove(&(sandbox.to_string(), kind));
        let Some(entry) = entry else { return };
        crate::sync::forward_terminate(&self.app, &entry.forward_name).await;
        if is_relayed(&entry, target_port) {
            let pattern = format!("TCP-LISTEN:{}", entry.remote_port);
            let _ = self
                .provider_on(&entry.connection)
                .exec(sandbox, &["pkill", "-f", &pattern], ExecOpts::default())
                .await;
        }
        self.emit_forwards();
    }

    async fn watch_ports_file(
        self: std::sync::Arc<Self>,
        sandbox: &str,
        workspace_path: &str,
        connection: &str,
        exec_capable: bool,
    ) {
        let mut watches = self.ports_watch.lock().unwrap();
        if watches.contains_key(sandbox) {
            return;
        }
        let sandbox = sandbox.to_string();
        let workspace_path = workspace_path.to_string();
        let connection = connection.to_string();
        let manager = self.clone();
        let task_sandbox = sandbox.clone();
        let handle = tokio::spawn(async move {
            poll_ports_file(manager, task_sandbox, workspace_path, connection, exec_capable).await;
        });
        watches.insert(sandbox, handle);
    }
}

/// Poll interval for `.mio/ports.json` and the listening-socket scan. Both are cheap (a local bind
/// mount read and a `podman exec cat /proc/net/tcp*`), so a short interval is fine.
const PORTS_FILE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Parses `/proc/net/tcp`/`/proc/net/tcp6` content (as read from inside the sandbox) into the set
/// of local ports currently in `LISTEN` state. Column 1 is `local_address` (`<hex addr>:<hex
/// port>`), column 3 is `st` (`0A` = `TCP_LISTEN`); the header row and any malformed line fail one
/// of the parses and are skipped for free. Bound address is ignored — the relay dials the
/// sandbox's own loopback either way (see `expose_port`'s `dial_spec` comment).
fn parse_listening_ports(output: &str) -> std::collections::HashSet<u16> {
    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.get(3)? != &"0A" {
                return None;
            }
            let port_hex = fields.get(1)?.rsplit(':').next()?;
            u16::from_str_radix(port_hex, 16).ok()
        })
        .collect()
}

/// Reads the sandbox's currently-listening TCP ports via `podman exec`. Best-effort: any failure
/// (container not running, `cat` missing, ...) reports no listening ports rather than erroring, so
/// a scan hiccup can't tear down forwards the next block sets up to remove.
async fn scan_listening_ports(
    provider: &Arc<dyn MachineProvider>,
    sandbox: &str,
) -> std::collections::HashSet<u16> {
    match provider
        .exec(
            sandbox,
            &["sh", "-c", "cat /proc/net/tcp /proc/net/tcp6 2>/dev/null"],
            ExecOpts::default(),
        )
        .await
    {
        Ok(out) if out.status.success() => parse_listening_ports(&String::from_utf8_lossy(&out.stdout)),
        _ => Default::default(),
    }
}

/// The `watch_ports_file` poll loop body, run as its own detached task. Each tick does two
/// independent things against the *live* `forwards` map (not a locally-remembered snapshot), so a
/// port that got exposed some other way between ticks — e.g. a manual "Expose a port" click — is
/// recognized as already-exposed instead of triggering a second relay for it:
///
/// 1. Reads `workspace_path`'s `.mio/ports.json` and diffs its `{"<port>": "<short name>"}`
///    entries against the live map. Only ports this loop itself exposed this way (`label.is_some()`)
///    are eligible to be auto-unexposed when their entry disappears from the file; a
///    manually-exposed port stays up even if it isn't (or is no longer) declared in the file.
/// 2. Scans the sandbox's own listening TCP sockets and exposes whichever aren't already tracked
///    under some other kind/relay-pool slot, so e.g. a dev server started from an open Terminal
///    pane shows up with no `.mio/ports.json` entry needed. Mirrors (1): only ports this loop
///    exposed this way (`auto_detected`) are auto-unexposed, once they stop listening.
///
/// Runs until `close_sandbox` aborts it.
async fn poll_ports_file(
    manager: std::sync::Arc<MutagenNetworkManager>,
    sandbox: String,
    workspace_path: String,
    connection: String,
    exec_capable: bool,
) {
    let path = std::path::Path::new(&workspace_path).join(".mio").join("ports.json");
    let provider = manager.provider_on(&connection);
    loop {
        tokio::time::sleep(PORTS_FILE_POLL_INTERVAL).await;

        if let Ok(contents) = tokio::fs::read_to_string(&path).await {
            if let Ok(raw) = serde_json::from_str::<HashMap<String, String>>(&contents) {
                let declared: HashMap<u16, String> = raw
                    .into_iter()
                    .filter_map(|(port, label)| port.parse::<u16>().ok().map(|p| (p, label)))
                    .collect();

                let (file_managed, already_exposed): (Vec<u16>, std::collections::HashSet<u16>) = {
                    let forwards = manager.forwards.lock().unwrap();
                    let exposed_for_sandbox: Vec<&ForwardInfo> =
                        forwards.values().filter(|f| f.sandbox == sandbox).collect();
                    let file_managed = exposed_for_sandbox
                        .iter()
                        .filter(|f| f.label.is_some())
                        .filter_map(|f| match f.kind {
                            ServiceKind::Exposed(p) => Some(p),
                            _ => None,
                        })
                        .collect();
                    let already_exposed = exposed_for_sandbox
                        .iter()
                        .filter_map(|f| match f.kind {
                            ServiceKind::Exposed(p) => Some(p),
                            _ => None,
                        })
                        .collect();
                    (file_managed, already_exposed)
                };

                for port in file_managed.into_iter().filter(|p| !declared.contains_key(p)) {
                    manager.unexpose_port(&sandbox, port).await;
                }

                for (port, label) in declared.into_iter().filter(|(p, _)| !already_exposed.contains(p)) {
                    let _ = manager
                        .expose_port(&sandbox, &connection, &workspace_path, port, Some(label), false, exec_capable)
                        .await;
                }
            }
        }

        let scanned = scan_listening_ports(&provider, &sandbox).await;

        let (auto_tracked, exclude): (Vec<u16>, std::collections::HashSet<u16>) = {
            let forwards = manager.forwards.lock().unwrap();
            let exposed_for_sandbox: Vec<&ForwardInfo> =
                forwards.values().filter(|f| f.sandbox == sandbox).collect();
            let auto_tracked = exposed_for_sandbox
                .iter()
                .filter(|f| f.auto_detected)
                .filter_map(|f| match f.kind {
                    ServiceKind::Exposed(p) => Some(p),
                    _ => None,
                })
                .collect();
            // The pool slots are `socat` relays this loop opened, not user servers — but only a
            // publish+relay sandbox has any, so an exec-capable one may legitimately serve there.
            let mut exclude: std::collections::HashSet<u16> = if exec_capable {
                std::collections::HashSet::new()
            } else {
                (RELAY_PORT_POOL_START..RELAY_PORT_POOL_START + RELAY_POOL_SIZE).collect()
            };
            exclude.extend(exposed_for_sandbox.iter().map(|f| match f.kind {
                ServiceKind::Exposed(p) => p,
                ServiceKind::Terminal | ServiceKind::AgentWeb => f.remote_port,
            }));
            (auto_tracked, exclude)
        };

        for port in auto_tracked.into_iter().filter(|p| !scanned.contains(p)) {
            manager.unexpose_port(&sandbox, port).await;
        }

        for port in scanned.into_iter().filter(|p| !exclude.contains(p)) {
            let _ = manager
                .expose_port(&sandbox, &connection, &workspace_path, port, None, true, exec_capable)
                .await;
        }
    }
}

/// Construct the managed [`NetworkManager`] for Tauri state. Mirrors `default_sync_engine` in
/// `sync/mod.rs`.
pub fn default_network_manager(
    app: AppHandle,
    published_port_transport: std::sync::Arc<dyn crate::transport::types::PortTransport>,
    runner: Arc<dyn ProcessRunner>,
) -> std::sync::Arc<dyn NetworkManager> {
    if crate::backends::mock_backend_enabled() {
        return std::sync::Arc::new(MockNetworkManager::new(app));
    }
    std::sync::Arc::new(MutagenNetworkManager {
        app,
        forwards: Mutex::new(HashMap::new()),
        expose_lock: tokio::sync::Mutex::new(()),
        ports_watch: Mutex::new(HashMap::new()),
        published_port_transport,
        runner,
    })
}
