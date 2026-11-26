use serde::Serialize;

/// Errors from sandbox backend operations. The error currency of every subsystem that drives a
/// container engine: each subsystem's own error type converts into it at its boundary.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    /// Sandbox name failed Podman's character/length rules.
    #[error("invalid sandbox name {name:?}: use letters, digits, '_' or '-', starting with a letter or digit")]
    InvalidName { name: String },
    /// A sandbox of this name already exists on some provider. Names are unique across the union of
    /// every provider's instances, not per provider, because every other command addresses a
    /// sandbox by name alone.
    #[error("a sandbox named {name:?} already exists on {provider}")]
    NameTaken { name: String, provider: ProviderId },
    /// The name ends in the suffix that names a sandbox's proxy sidecar, so a sandbox called
    /// `x-proxy` would be taken for `x`'s sidecar ([`super::PROXY_CONTAINER_SUFFIX`]).
    #[error("invalid sandbox name {name:?}: names ending in \"-proxy\" are reserved for traffic-inspection sidecars")]
    ReservedName { name: String },
    /// A new name contains `.`; the name is the SSH alias, so a dotted one could take over a real
    /// host (`docs/features/host-tool-ssh-access/04_improvements.md`, "Namespaced per-sandbox alias").
    #[error("invalid sandbox name {name:?}: new names cannot contain '.', because the name becomes an SSH host alias and a dotted name could take over a real host such as github.com")]
    NameHasDot { name: String },
    /// A new name matches a `Host` entry in the user's own SSH config, which the sandbox's alias
    /// would take over (same source as `NameHasDot`).
    #[error("invalid sandbox name {name:?}: your SSH config {path} already has a \"Host\" entry matching \"{pattern}\", and this sandbox's SSH alias would take that entry over; choose a different name")]
    ShadowsSshHost { name: String, pattern: String, path: String },
    /// Platform Documents directory not found (headless environment).
    #[error("could not resolve the Documents directory")]
    DocumentsDirNotFound,
    /// Failed to create sandbox's workspace directory.
    #[error("failed to create workspace {path}: {error}")]
    WorkspaceCreate { path: String, error: String },
    /// Provided existing_path does not exist.
    #[error("directory not found: {path}")]
    PathNotFound { path: String },
    /// Provided existing_path is a file, not a directory.
    #[error("not a directory: {path}")]
    PathNotADirectory { path: String },
    /// Provided existing_path is already tracked as a sandbox workspace.
    #[error("{path} is already a sandbox workspace")]
    PathAlreadyTracked { path: String },
    /// Podman or SSH process failed or exited with non-zero status.
    #[error("{0}")]
    CommandFailed(String),
    /// A podman invocation produced no exit status at all: it was still running when its bound
    /// elapsed, or it was not attempted because the seam is already known unresponsive. Distinct
    /// from `CommandFailed`, which always carries something podman actually said, and from
    /// `AppError::PodmanUnavailable`, whose remedies (`setupPodman`/`startPodman` in
    /// `src/lib/podman/setup.svelte.ts`) do not apply to a binary that never answers.
    #[error("{0}")]
    PodmanUnresponsive(String),
    /// Failed to parse JSON or URL from podman output or SSH URL argument.
    #[error("{0}")]
    ParseError(String),
    /// Deploy target is the same connection as the local container.
    #[error("cannot deploy {name:?} onto its own connection {connection:?}; the local container already runs there")]
    SelfDeploy { name: String, connection: String },
    /// Deploy was asked for a sandbox whose provider has no remote-target concept
    /// (`ProviderCapabilities.remote_targets`).
    #[error("{name:?} runs on a provider that cannot deploy to a remote host")]
    RemoteTargetsUnsupported { name: String },
    /// Deploy target has no SSH endpoint (e.g., unix-socket-only local connection).
    #[error("deploy target {connection:?} has no SSH endpoint to sync over")]
    NoSshEndpoint { connection: String },
    /// Error from SSH config management, carried as its message so this type depends on no
    /// subsystem; `SshError` converts into it in `ssh/types.rs`.
    #[error("{0}")]
    Ssh(String),
    /// The deploy target cannot host the egress boundary: its Podman is too old or uses a network
    /// backend the dual-homed sidecar needs more of (`docs/features/proxy-remote-deploy/02_dev.md`,
    /// Resolved Decision 6). Raised before anything is mutated.
    #[error("{connection:?} cannot host a bounded sandbox: {reason}")]
    RemoteHostUnsupported { connection: String, reason: String },
}

/// Classifies a raw spawn/wait failure from the process seam. `TimedOut` is the marker the process
/// layer sets for "podman produced no exit status", so no caller has to match on message text.
impl From<std::io::Error> for BackendError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::TimedOut => BackendError::PodmanUnresponsive(e.to_string()),
            _ => BackendError::CommandFailed(e.to_string()),
        }
    }
}

impl From<mio_files::FileError> for BackendError {
    fn from(e: mio_files::FileError) -> Self {
        BackendError::CommandFailed(e.to_string())
    }
}

/// Which engine a sandbox runs on, and which a `MachineRef` (`mio_provider::types`) names. Also
/// crosses IPC directly as a command parameter (`machine_setup_steps`/`machine_readiness`/
/// `prepare_machine` in `src-tauri/src/commands/machine.rs`), which is why it derives
/// `Serialize`/`Deserialize` on top of the `MachineRef` prefix-string form; both use the lowercase
/// name. A new provider is a variant here, a registry field, one arm per `match` over this enum,
/// and a `MachineProvider` implementation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    /// The default everywhere a provider is unstated.
    #[default]
    Podman,
}

impl ProviderId {
    const fn tag(self) -> &'static str {
        match self {
            ProviderId::Podman => "podman",
        }
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag())
    }
}

impl std::str::FromStr for ProviderId {
    type Err = BackendError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "podman" => Ok(ProviderId::Podman),
            other => Err(BackendError::ParseError(format!("unknown provider id {other:?}"))),
        }
    }
}

/// A sandbox instance (returned by all backend operations).
#[derive(Debug, Serialize, Clone)]
pub struct Sandbox {
    pub id: String,
    pub name: String,
    /// Backend-defined status (e.g., `"created"`, `"running"`, `"exited"`).
    pub status: String,
    /// Laptop workspace directory (the bind-mount source for local, sync alpha for deployed).
    pub workspace_path: String,
    /// Image version for updates (None for non-mio containers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_version: Option<u32>,
    /// Which engine runs this sandbox. Every command addresses a sandbox by name alone, so this is
    /// what lets the command layer resolve the name back to a provider without asking both.
    pub provider: ProviderId,
}
