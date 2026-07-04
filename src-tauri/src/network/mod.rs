//! Centralizes Mutagen-forward lifecycle and port allocation for the terminal/agent-web bridges
//! and for user-exposed sandbox ports. Public surface is [`types::NetworkManager`], held in Tauri
//! state as `Arc<dyn NetworkManager>` and registered in `src-tauri/src/lib.rs`.

pub mod types;

mod mock;

use std::collections::HashMap;
use std::process::Output;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter};
use tokio::process::Command;

use mock::MockNetworkManager;
use types::{ForwardInfo, NetworkError, NetworkManager, ServiceKind};

pub use types::EVENT_NAME;

/// First container-side port in the pre-published relay pool (`crate::backends` publishes
/// `RELAY_POOL_SIZE` consecutive ports starting here at `create`/`deploy`, each to an ephemeral
/// host loopback port). `expose_port` bridges a user-chosen port to a free slot in this pool via
/// an in-container `socat` relay, so exposing a new port never requires recreating the container.
pub const RELAY_PORT_POOL_START: u16 = 40000;

/// Number of relay-pool slots pre-published per sandbox, i.e. the max number of ports a single
/// sandbox can have exposed to the host at once. Bump if this proves too small in practice.
pub const RELAY_POOL_SIZE: u16 = 8;

/// Build the `--publish 127.0.0.1::<port>` args for the whole relay pool, for
/// `PodmanCliBackend::create`/`deploy` to append alongside the zellij/paseo publishes.
pub fn relay_pool_publish_args() -> Vec<String> {
    (RELAY_PORT_POOL_START..RELAY_PORT_POOL_START + RELAY_POOL_SIZE)
        .map(|p| format!("127.0.0.1::{p}"))
        .collect()
}

/// The Mutagen forwarding session name for (`sandbox`, `kind`), e.g. `terminal-<sandbox>` or
/// `expose-<sandbox>-<port>`. Also the prefix `crate::sync::sweep_stale_forwards` matches on to
/// find orphaned sessions from a crashed previous run.
fn forward_name(sandbox: &str, kind: ServiceKind) -> String {
    match kind {
        ServiceKind::Terminal => format!("terminal-{sandbox}"),
        ServiceKind::AgentWeb => format!("agentweb-{sandbox}"),
        ServiceKind::Exposed(port) => format!("expose-{sandbox}-{port}"),
    }
}

/// Allocate an ephemeral Windows-host loopback port by binding and immediately releasing it before
/// Mutagen binds it. The race window is negligible on loopback. Replaces the three copies of this
/// trick that used to live in `lib.rs`.
fn allocate_ephemeral_port() -> Result<u16, NetworkError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| NetworkError::PortAllocation(e.to_string()))?;
    listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| NetworkError::PortAllocation(e.to_string()))
}

/// Parse the published host port from `podman port <name> <port>/tcp` output. Mirrors
/// `runtime::parse_host_port`; kept as its own small copy rather than a shared helper, matching
/// this codebase's existing per-module `podman` helper convention (see `backends`/`runtime`).
fn parse_host_port(output: &str) -> Option<u16> {
    output
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("127.0.0.1:"))
        .find_map(|p| p.parse().ok())
}

async fn podman(connection: &str, args: &[&str]) -> Result<Output, NetworkError> {
    let mut full = vec!["--connection", connection];
    full.extend_from_slice(args);
    Command::new("podman")
        .args(&full)
        .output()
        .await
        .map_err(|e| NetworkError::CommandFailed(e.to_string()))
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
}

impl MutagenNetworkManager {
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
            if let ServiceKind::Exposed(_) = entry.kind {
                // Best-effort: the container may already be gone (destroy) or stopped (undeploy),
                // either of which already kills the relay process for free.
                let pattern = format!("TCP-LISTEN:{}", entry.remote_port);
                let _ = podman(&entry.connection, &["exec", sandbox, "pkill", "-f", &pattern]).await;
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
    ) -> Result<u16, NetworkError> {
        let _guard = self.expose_lock.lock().await;
        let slot = self.free_slot(sandbox).ok_or_else(|| NetworkError::NoFreeSlot {
            sandbox: sandbox.to_string(),
            limit: RELAY_POOL_SIZE as usize,
        })?;

        check(podman(connection, &["start", sandbox]).await?)?;

        // `fork` handles one client per accepted connection; `reuseaddr` lets a restarted relay
        // rebind immediately after a prior one was killed. The relay runs inside the same netns as
        // the user's server, so dialing its own loopback reaches it regardless of whether that
        // server bound `127.0.0.1` or `0.0.0.0`.
        let listen_spec = format!("TCP-LISTEN:{slot},fork,reuseaddr");
        let dial_spec = format!("TCP:127.0.0.1:{target_port}");
        check(
            podman(
                connection,
                &["exec", "-d", sandbox, "socat", &listen_spec, &dial_spec],
            )
            .await?,
        )?;

        let port_spec = format!("{slot}/tcp");
        let port_out = check(podman(connection, &["port", sandbox, &port_spec]).await?)?;
        let slot_host_port = parse_host_port(&port_out).ok_or_else(|| {
            NetworkError::CommandFailed(format!(
                "no 127.0.0.1 host mapping for relay slot {slot}; output:\n{port_out}"
            ))
        })?;

        let kind = ServiceKind::Exposed(target_port);
        let name = forward_name(sandbox, kind);
        let host_port = allocate_ephemeral_port()?;
        crate::sync::forward_create(&self.app, &name, host_port, connection, slot_host_port).await?;
        self.forwards.lock().unwrap().insert(
            (sandbox.to_string(), kind),
            ForwardInfo {
                sandbox: sandbox.to_string(),
                kind,
                host_port,
                connection: connection.to_string(),
                remote_port: slot,
                forward_name: name,
                label: label.clone(),
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
        let pattern = format!("TCP-LISTEN:{}", entry.remote_port);
        let _ = podman(&entry.connection, &["exec", sandbox, "pkill", "-f", &pattern]).await;
        self.emit_forwards();
    }

    async fn watch_ports_file(
        self: std::sync::Arc<Self>,
        sandbox: &str,
        workspace_path: &str,
        connection: &str,
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
            poll_ports_file(manager, task_sandbox, workspace_path, connection).await;
        });
        watches.insert(sandbox, handle);
    }
}

/// Poll interval for `.mio/ports.json`. It's a local bind mount (no network round trip to read),
/// so a short interval is cheap.
const PORTS_FILE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// The `watch_ports_file` poll loop body, run as its own detached task: reads `workspace_path`'s
/// `.mio/ports.json` and diffs its `{"<port>": "<short name>"}` entries against the *live*
/// `forwards` map (not a locally-remembered snapshot), so a port that got exposed some other way
/// between ticks — e.g. a manual "Expose a port" click, whose `expose_port` call also bootstraps
/// this same file via `ensure_ports_file` — is recognized as already-exposed instead of triggering
/// a second relay for it. Only ports this loop itself exposed (`label.is_some()`) are eligible to
/// be auto-unexposed when their entry disappears from the file; a manually-exposed port stays up
/// even if it isn't (or is no longer) declared in the file. Runs until `close_sandbox` aborts it.
async fn poll_ports_file(
    manager: std::sync::Arc<MutagenNetworkManager>,
    sandbox: String,
    workspace_path: String,
    connection: String,
) {
    let path = std::path::Path::new(&workspace_path).join(".mio").join("ports.json");
    loop {
        tokio::time::sleep(PORTS_FILE_POLL_INTERVAL).await;
        let Ok(contents) = tokio::fs::read_to_string(&path).await else { continue };
        let Ok(raw) = serde_json::from_str::<HashMap<String, String>>(&contents) else { continue };
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
                .expose_port(&sandbox, &connection, &workspace_path, port, Some(label))
                .await;
        }
    }
}

/// Construct the managed [`NetworkManager`] for Tauri state. Mirrors `default_sync_engine` in
/// `sync/mod.rs`.
pub fn default_network_manager(app: AppHandle) -> std::sync::Arc<dyn NetworkManager> {
    if crate::backends::mock_backend_enabled() {
        return std::sync::Arc::new(MockNetworkManager::new(app));
    }
    std::sync::Arc::new(MutagenNetworkManager {
        app,
        forwards: Mutex::new(HashMap::new()),
        expose_lock: tokio::sync::Mutex::new(()),
        ports_watch: Mutex::new(HashMap::new()),
    })
}
