use std::process::Output;

use crate::backends::types::{BackendError, Sandbox};

/// Errors from machine-provider operations. Mirrors [`BackendError`]'s process/parse variants so it
/// converts into the caller's error type at the subsystem boundary with no new IPC values.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Engine process failed or exited non-zero (captured stderr).
    #[error("{0}")]
    CommandFailed(String),
    /// The engine produced no exit status at all — carried as its own variant so the condition
    /// survives the hop into `BackendError` instead of collapsing into `CommandFailed`.
    #[error("{0}")]
    Unresponsive(String),
    /// Failed to parse the engine's JSON or text output.
    #[error("{0}")]
    ParseError(String),
    /// `MsbProvider::ensure_base_image` needs Podman to build/export the image
    /// (`docs/features/microsandbox-provider/02_dev.md`, Resolved Decision 2) and no working Podman
    /// machine is reachable. A distinct variant so this fails loudly instead of surfacing as a
    /// generic command failure.
    #[error("microsandbox base image requires a working Podman machine: {0}")]
    PodmanUnavailable(String),
}

impl From<ProviderError> for BackendError {
    fn from(e: ProviderError) -> Self {
        match e {
            ProviderError::CommandFailed(m) => BackendError::CommandFailed(m),
            ProviderError::Unresponsive(m) => BackendError::PodmanUnresponsive(m),
            ProviderError::ParseError(m) => BackendError::ParseError(m),
            ProviderError::PodmanUnavailable(m) => BackendError::CommandFailed(m),
        }
    }
}

/// How an instance's ports become reachable — the provider-neutral discriminant that selects the
/// engine's create argv. Deliberately free of engine-specific flags
/// (`docs/features/microsandbox-provider/02_dev.md`).
pub enum PublishModel {
    /// Reachability tunnels through the engine's `exec` transport; no ports are published.
    ExecTunnelled,
    /// Ports are published to host loopback and later bridged by an in-instance relay.
    PublishRelay,
}

/// Provider-neutral inputs for creating one instance. Every engine-specific create flag lives inside
/// the provider's `create_instance`, keyed off `publish_model`, never on this shape.
pub struct InstanceSpec {
    pub name: String,
    pub image: String,
    /// Host path bind-mounted as the instance's workspace.
    pub workspace_source: String,
    /// `key=value` label pairs stamped on the instance.
    pub labels: Vec<(String, String)>,
    pub publish_model: PublishModel,
}

/// Options for a one-shot exec: working directory, environment pairs, and a detached flag
/// (Podman's `-d`).
#[derive(Default)]
pub struct ExecOpts {
    pub workdir: Option<String>,
    pub env: Vec<(String, String)>,
    pub detached: bool,
}

/// Feature differences between provider engines, returned once by `capabilities()` rather than
/// probed per-operation (only `host_bind_mount` is genuinely runtime-dependent, and no provider
/// probes it yet — see `docs/features/microsandbox-provider/02_dev.md`). Deliberately omits
/// `publish_after_create`/`loopback_publish` from that document's earlier draft: `PublishModel::
/// ExecTunnelled` already makes both moot for every provider that constructs against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    /// Whether this provider supports `SandboxBackend::deploy`/`remove_remote` (an arbitrary
    /// remote target connection). `false` hides the deploy picker in the frontend rather than
    /// letting `deploy_sandbox` fail at invoke time.
    pub remote_targets: bool,
    /// Whether `SandboxBackend::listen_events` has an engine-native event stream to drive from.
    pub event_stream: bool,
    /// Whether the workspace can be a direct host bind mount rather than a synced volume.
    pub host_bind_mount: bool,
}

/// Which engine a [`MachineRef`] names. Also crosses IPC directly as a command parameter
/// (`machine_setup_steps`/`machine_readiness`/`prepare_machine` in `src-tauri/src/lib.rs`), which is
/// why it derives `Serialize`/`Deserialize` on top of the `MachineRef` prefix-string form below —
/// the two serializations are independent and allowed to diverge (`"microsandbox"` here vs. `"msb"`
/// in `tag()`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    /// The default everywhere a provider is unstated: every install has Podman, and an
    /// `AppSettings` file written before this feature deserializes to it.
    #[default]
    Podman,
    Microsandbox,
}

impl ProviderId {
    const fn tag(self) -> &'static str {
        match self {
            ProviderId::Podman => "podman",
            ProviderId::Microsandbox => "msb",
        }
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag())
    }
}

impl std::str::FromStr for ProviderId {
    type Err = ProviderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "podman" => Ok(ProviderId::Podman),
            "msb" => Ok(ProviderId::Microsandbox),
            other => Err(ProviderError::ParseError(format!("unknown provider id {other:?}"))),
        }
    }
}

/// A provider-tagged machine handle, replacing the untyped `connection: &str` threaded through
/// `RuntimeManager`/`NetworkManager`/`sync` today. Serializes as a prefixed string (`podman:
/// mio-tests`, `msb:local`) so IPC payloads and persisted `.mio/` JSON stay plain strings; see
/// `docs/features/microsandbox-provider/02_dev.md`. Resolved to a provider by
/// `ProviderRegistry::provider_for` (`src-tauri/src/provider/mod.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MachineRef {
    pub provider: ProviderId,
    pub id: String,
}

impl std::fmt::Display for MachineRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.provider, self.id)
    }
}

impl std::str::FromStr for MachineRef {
    type Err = ProviderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (tag, id) = s
            .split_once(':')
            .ok_or_else(|| ProviderError::ParseError(format!("not a machine ref: {s:?}")))?;
        if id.is_empty() {
            return Err(ProviderError::ParseError(format!("machine ref has empty id: {s:?}")));
        }
        Ok(MachineRef { provider: tag.parse()?, id: id.to_string() })
    }
}

#[cfg(test)]
mod machine_ref_tests {
    use super::*;

    #[test]
    fn round_trips_through_display_and_from_str() {
        let refs = [
            MachineRef { provider: ProviderId::Podman, id: "mio-tests".to_string() },
            MachineRef { provider: ProviderId::Microsandbox, id: "local".to_string() },
        ];
        for r in refs {
            let s = r.to_string();
            let parsed: MachineRef = s.parse().expect("parses back");
            assert_eq!(parsed, r);
        }
    }

    #[test]
    fn rejects_unknown_provider_tag() {
        assert!("docker:foo".parse::<MachineRef>().is_err());
    }

    #[test]
    fn rejects_missing_separator() {
        assert!("mio-tests".parse::<MachineRef>().is_err());
    }

    #[test]
    fn rejects_empty_id() {
        assert!("podman:".parse::<MachineRef>().is_err());
    }
}

/// The container-or-VM primitive seam shared by every subsystem that drives one instance engine.
/// Declares only operations that generalise across a container engine and a microVM engine; all
/// engine-specific argv lives in the implementation. The `PortTransport` seam
/// (`src-tauri/src/transport/`) covers published-port lookup, so this trait deliberately omits it.
#[async_trait::async_trait]
pub trait MachineProvider: Send + Sync {
    /// Feature differences from the other provider implementation (remote targets, event
    /// streaming, bind-mount support). Constant for a given provider in v1.
    fn capabilities(&self) -> ProviderCapabilities;
    /// Build the base image if absent (idempotent; the image is never in a registry).
    async fn ensure_base_image(&self) -> Result<(), ProviderError>;
    /// Whether the base image already exists on the active connection, without building it.
    async fn base_image_ready(&self) -> Result<bool, ProviderError>;
    /// Create a stopped instance from `spec`; returns the created instance id.
    async fn create_instance(&self, spec: &InstanceSpec) -> Result<String, ProviderError>;
    /// List all instances (running and stopped) as the provider-neutral [`Sandbox`] shape.
    async fn list_instances(&self) -> Result<Vec<Sandbox>, ProviderError>;
    /// Start a stopped instance.
    async fn start(&self, name: &str) -> Result<(), ProviderError>;
    /// Stop a running instance.
    async fn stop(&self, name: &str) -> Result<(), ProviderError>;
    /// Remove an instance (its workspace directory is left intact).
    async fn remove(&self, name: &str) -> Result<(), ProviderError>;
    /// One-shot exec inside `name`, awaited to completion.
    async fn exec(
        &self,
        name: &str,
        argv: &[&str],
        opts: ExecOpts,
    ) -> Result<Output, ProviderError>;
    /// Exec inside `name` with `stdin` streamed to completion — for `tar`/stdin-fed callers that the
    /// one-shot `exec` cannot serve. Bypasses the process-runner seam.
    async fn exec_piped(
        &self,
        name: &str,
        argv: &[&str],
        stdin: &[u8],
    ) -> Result<Output, ProviderError>;
    /// A configured, unspawned exec command for callers needing live bidirectional streaming. The
    /// caller attaches its own piped stdio and spawns. Bypasses the process-runner seam.
    fn exec_command(&self, name: &str, argv: &[&str], opts: ExecOpts) -> tokio::process::Command;
    /// Copy `host_path` into `name` at the in-instance path `dest` (`podman cp`).
    async fn copy_in(&self, name: &str, host_path: &str, dest: &str) -> Result<(), ProviderError>;
}
