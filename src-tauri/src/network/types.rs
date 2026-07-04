use serde::Serialize;

/// Emitted by both [`crate::network::MutagenNetworkManager`] and the mock with the full current
/// `Vec<ForwardInfo>` snapshot after every mutation, so `src/lib/network/state.svelte.ts` can stay
/// live instead of the frontend needing to poll or refetch after its own `expose_port` calls.
pub const EVENT_NAME: &str = "network://forwards-changed";

/// Which service a [`ForwardInfo`] bridges. `Exposed` carries the user-chosen port inside the
/// sandbox that an in-container `socat` relay bridges to one of the pre-published relay-pool slots
/// (see [`crate::network::RELAY_POOL_SIZE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, std::hash::Hash, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "targetPort")]
pub enum ServiceKind {
    Terminal,
    AgentWeb,
    Exposed(u16),
}

/// Errors from [`NetworkManager`]. Converts into [`crate::error::AppError`] via `#[from]`.
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("failed to allocate a Windows-host port: {0}")]
    PortAllocation(String),
    #[error("no free relay slot for sandbox {sandbox:?} (limit {limit})")]
    NoFreeSlot { sandbox: String, limit: usize },
    #[error(transparent)]
    Sync(#[from] crate::sync::types::SyncError),
    #[error("{0}")]
    CommandFailed(String),
}

/// One active Mutagen forward the app created and is tracking, so the frontend can show "what
/// ports are open right now" and so teardown (pane close / undeploy / destroy) can reverse exactly
/// what was opened.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInfo {
    pub sandbox: String,
    pub kind: ServiceKind,
    /// Windows-host loopback port the frontend/webview connects to.
    pub host_port: u16,
    /// Podman connection / SSH alias this forward tunnels over.
    pub connection: String,
    /// Container-side port reached through the forward (the podman-published remote_port for
    /// Terminal/AgentWeb, or the relay-pool slot's container port for Exposed).
    pub remote_port: u16,
    /// The Mutagen forwarding session name, e.g. `terminal-<sandbox>` / `expose-<sandbox>-<port>`.
    pub forward_name: String,
    /// Short human name from `.mio/ports.json` when this exposure was declared by the in-container
    /// contract file rather than the "Expose a port" UI (see `watch_ports_file`). Its presence is
    /// also how the frontend decides whether to show the manual close (X) button: file-declared
    /// entries are removed by editing the file, not by clicking X.
    pub label: Option<String>,
}

/// Owns Mutagen-forward lifecycle and port allocation for the terminal/agent-web bridges and for
/// user-exposed sandbox ports, replacing the ad hoc `ForwardPortMap` plus duplicated
/// ephemeral-port/`forward_create`/`forward_terminate` call sites that used to live in `lib.rs`.
/// The sole implementation is [`crate::network::MutagenNetworkManager`]; `MockNetworkManager` is
/// fully in-memory for the e2e light tier.
#[async_trait::async_trait]
pub trait NetworkManager: Send + Sync {
    /// Allocate a fresh Windows-host loopback port, create (or replace) the Mutagen forward from
    /// it to `127.0.0.1:remote_port` on `connection` (the podman-published loopback port for
    /// zellij/paseo), register it under (`sandbox`, `kind`), and return the host port.
    async fn open_forward(
        &self,
        sandbox: &str,
        kind: ServiceKind,
        connection: &str,
        remote_port: u16,
    ) -> Result<u16, NetworkError>;

    /// Tear down the forward for (`sandbox`, `kind`) if one is registered. Best-effort.
    async fn close_forward(&self, sandbox: &str, kind: ServiceKind);

    /// Tear down every forward registered for `sandbox`, including any exposed ports. The single
    /// call site that replaces today's paired `forward_terminate` calls plus `forward_ports`
    /// mutation at pane-close / undeploy / destroy / app-shutdown.
    async fn close_sandbox(&self, sandbox: &str);

    /// Snapshot of every forward currently tracked, for the frontend's open-ports view.
    async fn list_forwards(&self) -> Vec<ForwardInfo>;

    /// Reconcile with reality at startup: terminate Mutagen sessions (`terminal-*` / `agentweb-*` /
    /// `expose-*`) orphaned by a crashed previous run, for any sandbox not in `live_sandboxes`.
    async fn sweep(&self, live_sandboxes: &[String]);

    /// Bridge `target_port` (a port the user's own process bound inside sandbox `name`, e.g. a dev
    /// server) to a fresh Windows-host loopback port, via an in-container `socat` relay bound to a
    /// free slot in the sandbox's pre-published relay pool. Never recreates the container, so it
    /// does not disturb whatever is already running inside it. Returns the host port.
    /// `workspace_path` is used only to bootstrap `.mio/ports.json` (see `crate::network::ensure_ports_file`)
    /// when it doesn't exist yet, so the contract file has somewhere to start from.
    async fn expose_port(
        &self,
        sandbox: &str,
        connection: &str,
        workspace_path: &str,
        target_port: u16,
        label: Option<String>,
    ) -> Result<u16, NetworkError>;

    /// Tear down the relay and forward for `target_port` on `sandbox`, freeing its pool slot.
    /// Best-effort.
    async fn unexpose_port(&self, sandbox: &str, target_port: u16);

    /// Start (idempotently) polling `<workspace_path>/.mio/ports.json` for `sandbox` — a contract
    /// file the container's own tooling can write to declare `{"<port>": "<short name>"}` entries —
    /// and call `expose_port`/`unexpose_port` for whatever changed since the last poll. A no-op if
    /// `sandbox` is already being watched. `close_sandbox` cancels the watch task.
    ///
    /// Takes `self` by `Arc` (rather than `&self`) so the spawned poll loop can hold its own
    /// `'static` handle back into the manager to call `expose_port`/`unexpose_port` on.
    async fn watch_ports_file(
        self: std::sync::Arc<Self>,
        sandbox: &str,
        workspace_path: &str,
        connection: &str,
    );
}
