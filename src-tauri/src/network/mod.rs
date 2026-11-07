//! Mutagen-forward lifecycle and port allocation for terminal/agent-web and user-exposed ports.

pub mod types;

mod mock;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter};

use mock::MockNetworkManager;
use types::{ForwardInfo, NetworkError, NetworkManager, ServiceKind};

use crate::provider::types::{ExecOpts, MachineProvider, MachineRef};
use crate::provider::ProviderRegistry;

pub use types::EVENT_NAME;

fn forward_name(sandbox: &str, kind: ServiceKind) -> String {
    match kind {
        ServiceKind::Terminal => format!("terminal-{sandbox}"),
        ServiceKind::AgentWeb => format!("agentweb-{sandbox}"),
        ServiceKind::Exposed(port) => format!("expose-{sandbox}-{port}"),
    }
}

fn allocate_ephemeral_port() -> Result<u16, NetworkError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| NetworkError::PortAllocation(e.to_string()))?;
    listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| NetworkError::PortAllocation(e.to_string()))
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

/// Drives the bundled Mutagen sidecar (via `crate::sync`) and reads each sandbox's listening
/// sockets through its provider. Holds the [`AppHandle`] the sidecar is resolved through.
pub struct MutagenNetworkManager {
    app: AppHandle,
    forwards: Mutex<HashMap<(String, ServiceKind), ForwardInfo>>,
    /// Serializes `expose_port` end-to-end (forward creation through registry insert), so
    /// `close_sandbox` can wait out an in-flight expose.
    expose_lock: tokio::sync::Mutex<()>,
    /// One poll-loop task per sandbox currently being watched by `watch_ports_file`, so a second
    /// call for the same sandbox is a no-op and `close_sandbox` can cancel it.
    ports_watch: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// Resolves each method's explicit `machine` to its provider. Held as a registry rather than
    /// one pinned provider because each call names its own machine — mirroring `MiseRuntime`'s
    /// `providers`.
    providers: Arc<ProviderRegistry>,
}

impl MutagenNetworkManager {
    /// The provider driving `machine`, mirroring `MiseRuntime::provider_on`.
    fn provider_on(&self, machine: &MachineRef) -> Arc<dyn MachineProvider> {
        self.providers.provider_for(machine)
    }

    /// Push the full current snapshot to `EVENT_NAME` so `state.svelte.ts` stays live. Called after
    /// every mutation below instead of making the frontend refetch via `list_open_forwards`.
    fn emit_forwards(&self) {
        let snapshot: Vec<ForwardInfo> = self.forwards.lock().unwrap().values().cloned().collect();
        let _ = self.app.emit(EVENT_NAME, &snapshot);
    }
}

#[async_trait::async_trait]
impl NetworkManager for MutagenNetworkManager {
    async fn open_forward(
        &self,
        sandbox: &str,
        kind: ServiceKind,
        alias: &str,
        remote_port: u16,
    ) -> Result<u16, NetworkError> {
        let name = forward_name(sandbox, kind);
        let host_port = allocate_ephemeral_port()?;
        crate::sync::forward_create(&self.app, &name, host_port, alias, remote_port).await?;
        self.forwards.lock().unwrap().insert(
            (sandbox.to_string(), kind),
            ForwardInfo {
                sandbox: sandbox.to_string(),
                kind,
                host_port,
                connection: alias.to_string(),
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
        // Order is load-bearing: take `expose_lock` *before* aborting the poll task. `expose_port`
        // creates the Mutagen forward and only then records it, so aborting first can cancel it in
        // that window and strand a live forward this map never learns about — one nothing can
        // terminate afterwards. Holding the lock lets an in-flight expose finish and register, and
        // cancels a queued one while it still waits here, before it creates anything.
        let _expose_guard = self.expose_lock.lock().await;
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
        machine: &MachineRef,
        workspace_path: &str,
        target_port: u16,
        label: Option<String>,
        auto_detected: bool,
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
        // The forward dials the container's own loopback through the sandbox's alias, which
        // only resolves once the sandbox is running.
        self.provider_on(machine).start(sandbox).await?;
        crate::sync::forward_create(&self.app, &name, host_port, sandbox, target_port).await?;

        self.forwards.lock().unwrap().insert(
            (sandbox.to_string(), kind),
            ForwardInfo {
                sandbox: sandbox.to_string(),
                kind,
                host_port,
                connection: sandbox.to_string(),
                remote_port: target_port,
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
        self.emit_forwards();
    }

    async fn watch_ports_file(
        self: std::sync::Arc<Self>,
        sandbox: &str,
        workspace_path: &str,
        machine: &MachineRef,
    ) {
        let mut watches = self.ports_watch.lock().unwrap();
        if watches.contains_key(sandbox) {
            return;
        }
        let sandbox = sandbox.to_string();
        let workspace_path = workspace_path.to_string();
        let machine = machine.clone();
        let manager = self.clone();
        let task_sandbox = sandbox.clone();
        let handle = tokio::spawn(async move {
            poll_ports_file(manager, task_sandbox, workspace_path, machine).await;
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
/// of the parses and are skipped for free. Bound address is ignored — a forward dials the
/// sandbox's own loopback, which reaches a server bound to `127.0.0.1` or `0.0.0.0` alike.
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
/// recognized as already-exposed instead of triggering a second forward for it:
///
/// 1. Reads `workspace_path`'s `.mio/ports.json` and diffs its `{"<port>": "<short name>"}`
///    entries against the live map. Only ports this loop itself exposed this way (`label.is_some()`)
///    are eligible to be auto-unexposed when their entry disappears from the file; a
///    manually-exposed port stays up even if it isn't (or is no longer) declared in the file.
/// 2. Scans the sandbox's own listening TCP sockets and exposes whichever aren't already tracked
///    under some other kind, so e.g. a dev server started from an open Terminal
///    pane shows up with no `.mio/ports.json` entry needed. Mirrors (1): only ports this loop
///    exposed this way (`auto_detected`) are auto-unexposed, once they stop listening.
///
/// Runs until `close_sandbox` aborts it.
async fn poll_ports_file(
    manager: std::sync::Arc<MutagenNetworkManager>,
    sandbox: String,
    workspace_path: String,
    machine: MachineRef,
) {
    let path = std::path::Path::new(&workspace_path).join(".mio").join("ports.json");
    let provider = manager.provider_on(&machine);
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
                        .expose_port(&sandbox, &machine, &workspace_path, port, Some(label), false)
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
            // A pane's own server (zellij, paseo) listens in the sandbox too and must not be
            // auto-exposed as a user port.
            let exclude = exposed_for_sandbox
                .iter()
                .map(|f| match f.kind {
                    ServiceKind::Exposed(p) => p,
                    ServiceKind::Terminal | ServiceKind::AgentWeb => f.remote_port,
                })
                .collect();
            (auto_tracked, exclude)
        };

        for port in auto_tracked.into_iter().filter(|p| !scanned.contains(p)) {
            manager.unexpose_port(&sandbox, port).await;
        }

        for port in scanned.into_iter().filter(|p| !exclude.contains(p)) {
            let _ = manager
                .expose_port(&sandbox, &machine, &workspace_path, port, None, true)
                .await;
        }
    }
}

/// Construct the managed [`NetworkManager`] for Tauri state. Mirrors `default_sync_engine` in
/// `sync/mod.rs`.
pub fn default_network_manager(
    app: AppHandle,
    providers: Arc<ProviderRegistry>,
) -> std::sync::Arc<dyn NetworkManager> {
    if crate::backends::mock_backend_enabled() {
        return std::sync::Arc::new(MockNetworkManager::new(app));
    }
    std::sync::Arc::new(MutagenNetworkManager {
        app,
        forwards: Mutex::new(HashMap::new()),
        expose_lock: tokio::sync::Mutex::new(()),
        ports_watch: Mutex::new(HashMap::new()),
        providers,
    })
}
