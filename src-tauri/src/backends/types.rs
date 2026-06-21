use serde::Serialize;
use tauri::ipc::Channel;

/// A sandbox instance as seen by the frontend. Returned by all [`SandboxBackend`] operations.
/// Field `status` is backend-defined (e.g. `"created"`, `"running"`, `"exited"`).
#[derive(Debug, Serialize, Clone)]
pub struct Sandbox {
    pub id: String,
    pub name: String,
    pub status: String,
    /// Laptop directory the user edits. For a sandbox later deployed to a remote host it is also
    /// the sync engine's local (alpha) end; until then it is the bind-mount source (see
    /// [`WorkspacePlan`]). Derived from the name, so it is set even for sandboxes only listed this
    /// session.
    pub workspace_path: String,
}

/// The Mutagen endpoints a freshly deployed sandbox needs to seed its sync session. Returned by
/// [`SandboxBackend::deploy`] and handed straight to `SyncEngine::create` by the `deploy_sandbox`
/// command, so the backend never reaches into the sync engine.
pub struct RemoteDeployment {
    /// Local (alpha) end: the laptop directory, which at deploy time holds the only copy of the
    /// workspace files (a `podman commit` does not capture the bind-mounted volume).
    pub alpha: String,
    /// Remote (beta) end as a Mutagen scp-style `alias:path` URL, where `alias` is the target
    /// connection's OpenSSH alias and `path` is the empty remote workspace the initial reconcile
    /// seeds from `alpha`.
    pub beta: String,
}

/// Where a sandbox's workspace lives and how it reaches the laptop. Returned by `resolve_workspace`
/// and consumed by the deploy flow to compute the remote bind-mount source and SSH endpoint;
/// `create` is always local and does not use it.
pub(super) struct WorkspacePlan {
    /// Path bind-mounted at `/root/work` on the host running the container. Equals `laptop_dir`
    /// for a local sandbox; a remote-filesystem path for a remote one.
    pub bind_mount_source: String,
    /// Laptop directory the user edits, and the sync engine's local (alpha) endpoint.
    pub laptop_dir: String,
    pub strategy: SyncStrategy,
}

/// How a sandbox's bind-mount source is reconciled with its laptop directory.
pub(super) enum SyncStrategy {
    /// Local connection: the two paths are the same directory, so there is nothing to sync.
    Local,
    /// Remote connection: the bind-mount source is on the remote host and must be mirrored to the
    /// laptop directory by the sync engine over the given SSH connection.
    Remote(ConnectionInfo),
}

/// Live SSH endpoint of a Podman connection, read fresh from `podman system connection list`:
/// a machine's port is assigned at boot, so a cached one goes stale after a restart.
#[derive(Debug, Clone)]
pub(super) struct ConnectionInfo {
    pub host: String,
    pub port: String,
    pub user: String,
    /// Private key Podman registered for the connection; reused for the remote `mkdir` and sync.
    pub identity: String,
}

/// Primary interface for sandbox lifecycle management.
///
/// Tauri state holds an `Arc<dyn SandboxBackend>` — commands receive it via [`tauri::State`]
/// and delegate directly to these methods.
///
/// The only implementation is [`PodmanCliBackend`], which drives the `podman` CLI over a
/// named **system connection**. The same code path serves the local Podman machine and any
/// remote host: both are just connections, and `podman` itself performs the SSH transport.
///
/// To change the active connection at startup, set `PODMAN_CONNECTION` (see
/// [`PodmanCliBackend`]). To swap the whole backend, change `default_backend()` in
/// `backends/mod.rs`.
#[async_trait::async_trait]
pub trait SandboxBackend: Send + Sync {
    /// Pull the base image if absent, then create a stopped container named `name`.
    ///
    /// Also ensures the sandbox's host workspace directory exists and bind-mounts it at
    /// `/root/work`. The directory is `<Documents>/sandbox-mio/<name>` (override the root
    /// with `SANDBOX_WORKSPACE_ROOT`). Phase 1 is local-only: the workspace lives on the
    /// laptop and `podman` translates the Windows path into the machine VM.
    async fn create(&self, name: &str) -> Result<Sandbox, String>;
    /// Return all containers (running and stopped).
    async fn list(&self) -> Result<Vec<Sandbox>, String>;
    /// Start a stopped container.
    async fn start(&self, name: &str) -> Result<(), String>;
    /// Stop a running container.
    async fn stop(&self, name: &str) -> Result<(), String>;
    /// Remove the **local** container, stopping it first if running. The sandbox's workspace
    /// directory is never deleted: for a deployed sandbox it may hold the only copy of unsynced
    /// work. The `destroy_sandbox` command tears down the sync session and remote container first.
    async fn destroy(&self, name: &str) -> Result<(), String>;
    /// Migrate `name`'s workload to `target_connection` by snapshot-and-push: stop and `podman
    /// commit` the local container, `podman image scp` the image to the target, create and start a
    /// remote container bind-mounting an empty remote workspace, and register the target's OpenSSH
    /// alias. Returns the sync endpoints; the `deploy_sandbox` command seeds the session from them.
    /// The local container is left stopped (not removed), so deploy is reversible. Runs Podman
    /// against `target_connection`, not the backend's pinned `connection`.
    async fn deploy(
        &self,
        name: &str,
        target_connection: &str,
    ) -> Result<RemoteDeployment, String>;
    /// Remove the container named `name` on `connection` (the deploy target), used when destroying
    /// or undeploying a deployed sandbox. The remote workspace directory is left intact.
    async fn remove_remote(&self, name: &str, connection: &str) -> Result<(), String>;
    /// Stream container events to the frontend channel until the connection drops.
    async fn listen_events(&self, on_event: Channel<serde_json::Value>) -> Result<(), String>;

    // --- Host (connection) management ---------------------------------------
    // These operate on the Podman system-connection registry, independent of which
    // connection the backend is currently bound to.

    /// Return the names of all registered Podman system connections (local + remote).
    async fn list_hosts(&self) -> Result<Vec<String>, String>;
    /// Register an existing remote host as a Podman system connection.
    ///
    /// `ssh_url` is the full `ssh://user@host[:port]/run/user/<uid>/podman/podman.sock`.
    /// `identity` is an optional path to the SSH private key.
    async fn add_host(
        &self,
        name: &str,
        ssh_url: &str,
        identity: Option<&str>,
    ) -> Result<(), String>;
    /// Enable the rootless `podman.socket` and user lingering on an already-reachable host
    /// over SSH. Exposed standalone (to prepare a host whose key is already authorized) and
    /// reused as a step of [`SandboxBackend::provision_host`].
    async fn prepare_host(&self, ssh_url: &str, identity_path: &str) -> Result<(), String>;
    /// Guided "prepare + connect" flow for an external Linux server. Generates an SSH
    /// key if absent, enables the remote `podman.socket` + user lingering, registers the
    /// connection, and verifies it. Progress for each step is streamed to `on_progress`.
    ///
    /// Prerequisite: the public key must already be authorized on the remote
    /// (`ssh-copy-id` is a one-time manual step — `ssh` reads passwords from the TTY,
    /// not stdin, so it cannot be driven from here).
    async fn provision_host(
        &self,
        name: &str,
        ssh_url: &str,
        identity_path: &str,
        on_progress: Channel<serde_json::Value>,
    ) -> Result<(), String>;
}

/// Drives Podman by invoking the `podman` CLI against a named **system connection**.
///
/// Every lifecycle call runs `podman --connection <connection> <subcommand>`; `podman`
/// performs the SSH transport, so the local machine and remote hosts are handled
/// identically. JSON output is parsed with `--format json`.
///
/// `connection` is the Podman system-connection name. It defaults to
/// `podman-machine-default` (the connection `podman machine init` creates for the local
/// machine) and is overridden by the `PODMAN_CONNECTION` environment variable in
/// `default_backend()`.
pub(super) struct PodmanCliBackend {
    pub connection: String,
}
