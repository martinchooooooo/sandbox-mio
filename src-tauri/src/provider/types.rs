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
    /// A published host port was already taken when the engine tried to bind it.
    #[error("host port already in use by {0}")]
    PortInUse(String),
}

impl From<ProviderError> for BackendError {
    fn from(e: ProviderError) -> Self {
        match e {
            ProviderError::CommandFailed(m) => BackendError::CommandFailed(m),
            ProviderError::Unresponsive(m) => BackendError::PodmanUnresponsive(m),
            ProviderError::ParseError(m) => BackendError::ParseError(m),
            ProviderError::PodmanUnavailable(m) => BackendError::CommandFailed(m),
            e @ ProviderError::PortInUse(_) => BackendError::CommandFailed(e.to_string()),
        }
    }
}

/// Provider-neutral inputs for creating one instance. Every engine-specific create flag lives inside
/// the provider's `create_instance`, never on this shape.
pub struct InstanceSpec {
    pub name: String,
    pub image: String,
    /// Host path bind-mounted as the instance's workspace.
    pub workspace_source: String,
    /// `key=value` label pairs stamped on the instance.
    pub labels: Vec<(String, String)>,
    /// `key=value` environment pairs stamped on the instance (`--env` at create time), visible to
    /// every process tree including non-interactive `exec` sessions.
    pub env: Vec<(String, String)>,
    /// A network attachment that *replaces* the provider's default network mode
    /// (`docs/features/network-proxy/02_dev.md`, Spike Results: Podman rejects combining `pasta`
    /// with a second `--network`).
    pub network: Option<InstanceNetwork>,
    /// Outbound policy enforced by the engine itself. Only providers whose
    /// `capabilities().egress_policy` is true accept `Some`
    /// (`docs/features/microsandbox-network-policy/02_dev.md`).
    pub egress: Option<EgressPolicy>,
}

/// The fallback for traffic no [`EgressRule`] matches. Serialised lowercase, as it is in
/// `.mio/net-policy.json`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EgressMode {
    /// Everything public is reachable unless a rule denies it.
    #[default]
    Open,
    /// Only what a rule allows is reachable.
    Allowlist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Deny,
}

/// A named address group an [`EgressRule`] can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetGroup {
    Public,
    Private,
    Host,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetProtocol {
    Tcp,
    Udp,
}

/// What the engine does when a guest sends a secret placeholder somewhere it is not bound to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViolationAction {
    Block,
    #[default]
    BlockAndLog,
    BlockAndTerminate,
}

/// The one destination an [`EgressRule`] matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressTarget {
    /// An exact hostname, or `*.suffix` with at least two suffix labels.
    Host(String),
    Cidr(String),
    Group(NetGroup),
}

/// One ordered rule; the first rule matching a connection decides it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressRule {
    pub action: RuleAction,
    pub target: EgressTarget,
    /// `None` matches both protocols.
    pub protocol: Option<NetProtocol>,
    /// Inclusive `(start, end)` ranges; empty matches every port.
    pub ports: Vec<(u16, u16)>,
}

/// A secret the engine substitutes on the way out. Carries the guest variable name only — the value
/// reaches the engine through [`StartEnvSource`], never through this shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretBindingSpec {
    pub env_var: String,
    /// Exact hostnames the value may be sent to.
    pub hosts: Vec<String>,
    /// Header names substituted; empty substitutes no header.
    pub headers: Vec<String>,
    pub query: bool,
    pub body: bool,
}

/// An upstream proxy every outbound connection is dialled through — the network-proxy traffic
/// viewer's sidecar, reached on host loopback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainedProxy {
    pub host_port: u16,
    pub username: String,
    /// Name of the variable in the engine process's environment that holds the proxy password.
    pub password_env: String,
    /// Host path of the CA the proxy re-signs upstream TLS with.
    pub upstream_ca_path: String,
}

/// Provider-neutral outbound policy for [`InstanceSpec::egress`], built from
/// `.mio/net-policy.json` by `crate::net_policy`. Holds no secret value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressPolicy {
    pub mode: EgressMode,
    pub rules: Vec<EgressRule>,
    /// When true, a connection whose hostname cannot be inspected is refused.
    pub strict_hostnames: bool,
    pub secrets: Vec<SecretBindingSpec>,
    pub violation_action: ViolationAction,
    pub deny_message: Option<String>,
    pub proxy: Option<ChainedProxy>,
}

/// One blocked secret use microsandbox logged: a placeholder sent somewhere its binding does not
/// allow. Carries the variable name, never the value or the placeholder.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretViolation {
    pub ts: String,
    pub action: String,
    pub env_var: String,
    pub host: String,
    pub method: String,
    pub path: String,
    /// `header`, `query` or `body`.
    pub location: String,
}

/// A network attachment for [`InstanceSpec`], optionally pinned to a static IP — needed because the
/// network this feature creates has DNS disabled (`docs/features/network-proxy/02_dev.md`, Spike
/// Results: `aardvark-dns` cannot start on this project's podman machine).
pub struct InstanceNetwork {
    pub name: String,
    pub ip: Option<String>,
}

/// A bind mount for [`SidecarSpec`]: `host_path` mounted at `dest` inside the container.
pub struct SidecarMount {
    pub host_path: String,
    pub dest: String,
    pub read_only: bool,
}

/// A port of a sidecar published to the host: `bind:host_port` on the host forwards to `guest_port`.
pub struct PortPublish {
    pub bind: String,
    pub host_port: u16,
    pub guest_port: u16,
}

/// How to tell a sidecar's listener is actually serving: engines report an instance running
/// seconds before the process inside listens (`docs/features/microsandbox-network-policy/spike/
/// 04_msb-mitm-sidecar.md`, Item 6). Probed by `provider::probe::wait_sidecar_ready`.
pub struct SidecarReadiness {
    pub host_port: u16,
    /// SOCKS5 username/password to complete sub-negotiation with; `None` probes no-auth SOCKS5.
    pub socks5_auth: Option<(String, String)>,
    pub budget_ms: u64,
}

/// Provider-neutral inputs for a sidecar container — a shape [`InstanceSpec`] cannot express: an
/// arbitrary number of mounts at caller-chosen destinations, a caller-chosen launch command, no
/// implicit `/bin/sh` entrypoint. Used only by the network-proxy mitmproxy container
/// (`docs/features/network-proxy/02_dev.md`); every sandbox is still created through
/// [`MachineProvider::create_instance`].
pub struct SidecarSpec {
    pub name: String,
    pub image: String,
    /// Required by Podman; microsandbox chains over host loopback and ignores it.
    pub network: Option<InstanceNetwork>,
    pub mounts: Vec<SidecarMount>,
    /// Ports published to the host.
    pub publish: Vec<PortPublish>,
    /// `key=value` environment for `command`.
    pub env: Vec<(String, String)>,
    pub command: Vec<String>,
    /// `key=value` label pairs stamped on the container.
    pub labels: Vec<(String, String)>,
}

/// Options for a one-shot exec: working directory, environment pairs, and a detached flag
/// (Podman's `-d`).
///
/// `env` values appear in argv and therefore in `mio.log`; anything secret goes in `secret_env`,
/// whose values reach the guest without ever entering a command line
/// (`docs/features/techdebt-sweep/02_dev.md`, Resolved Decision 4).
#[derive(Default)]
pub struct ExecOpts {
    pub workdir: Option<String>,
    pub env: Vec<(String, String)>,
    pub secret_env: Vec<(String, String)>,
    pub detached: bool,
}

/// Feature differences between provider engines, returned once by `capabilities()` rather than
/// probed per-operation (only `host_bind_mount` is genuinely runtime-dependent, and no provider
/// probes it yet — see `docs/features/microsandbox-provider/02_dev.md`).
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
    /// Whether `create_instance` enforces [`InstanceSpec::egress`] with the engine's own network
    /// stack (`docs/features/microsandbox-network-policy/02_dev.md`).
    pub egress_policy: bool,
}

/// Which engine a [`MachineRef`] names. Also crosses IPC directly as a command parameter
/// (`machine_setup_steps`/`machine_readiness`/`prepare_machine` in `src-tauri/src/commands/machine.rs`), which is
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

/// Resolves the environment a start-capable engine call (`create`, `start`, `exec`) must carry for
/// one instance: bound secret values and a chained proxy's password. Implemented by
/// `services::start_env::AppStartEnv`; injected into `MsbProvider` so the provider never reads app
/// state itself (`docs/features/microsandbox-network-policy/02_dev.md`, Phase C).
///
/// Contract: an instance with no applied policy snapshot (a sidecar, a sandbox predating the
/// feature) gets an empty list. A bound secret with no stored value is an error, never an omitted
/// variable, so the engine cannot fall back to a same-named variable inherited from the app.
pub trait StartEnvSource: Send + Sync {
    fn env_for(&self, instance: &str) -> Result<Vec<(String, String)>, ProviderError>;
}

/// A [`StartEnvSource`] that never adds anything, for providers built without app state.
#[cfg(test)]
pub struct NoStartEnv;

#[cfg(test)]
impl StartEnvSource for NoStartEnv {
    fn env_for(&self, _instance: &str) -> Result<Vec<(String, String)>, ProviderError> {
        Ok(Vec::new())
    }
}

/// The container-or-VM primitive seam shared by every subsystem that drives one instance engine.
/// Declares only operations that generalise across a container engine and a microVM engine; all
/// engine-specific argv lives in the implementation.
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
    /// Create a sidecar from `spec` — a shape `create_instance` cannot express. A published port
    /// that turns out to be taken is [`ProviderError::PortInUse`].
    async fn create_sidecar(&self, spec: &SidecarSpec) -> Result<String, ProviderError>;
    /// Stop a sidecar, forcing it if the engine cannot stop its launched command gracefully
    /// (microsandbox's `--init` sidecars hang in `draining` otherwise).
    async fn stop_sidecar(&self, name: &str) -> Result<(), ProviderError>;
    /// Replace the instance named `spec.name` with a new one from `spec`, keeping its workspace
    /// data. The old instance is fully removed before the new one is created.
    async fn recreate_instance(&self, spec: &InstanceSpec) -> Result<String, ProviderError>;
    /// Pull `image` if not already present locally. Unlike `ensure_base_image`, never builds —
    /// always pulls from a registry. Used for the mitmproxy sidecar image, which this app never
    /// builds itself (`docs/features/network-proxy/02_dev.md`, Resolved Decision 10).
    async fn pull_image(&self, image: &str) -> Result<(), ProviderError>;
    /// List all instances (running and stopped) as the provider-neutral [`Sandbox`] shape.
    async fn list_instances(&self) -> Result<Vec<Sandbox>, ProviderError>;
    /// Start a stopped instance.
    async fn start(&self, name: &str) -> Result<(), ProviderError>;
    /// Stop a running instance.
    async fn stop(&self, name: &str) -> Result<(), ProviderError>;
    /// Remove an instance (its workspace directory is left intact).
    async fn remove(&self, name: &str) -> Result<(), ProviderError>;
    /// One-shot exec inside `name`, awaited to completion. The only exec that honours
    /// `ExecOpts.secret_env`.
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
    /// caller attaches its own piped stdio and spawns. Bypasses the process-runner seam, so it never
    /// carries `ExecOpts.secret_env`.
    fn exec_command(&self, name: &str, argv: &[&str], opts: ExecOpts) -> tokio::process::Command;
    /// Copy `host_path` into `name` at the in-instance path `dest` (`podman cp`).
    async fn copy_in(&self, name: &str, host_path: &str, dest: &str) -> Result<(), ProviderError>;
    /// Create a DNS-disabled network with the given subnet (CIDR), for a caller that needs
    /// reachability between two instances without relying on `aardvark-dns`
    /// (`docs/features/network-proxy/02_dev.md`, Spike Results). Idempotent name collision is the
    /// caller's concern, not this method's.
    async fn create_network(&self, name: &str, subnet: &str) -> Result<(), ProviderError>;
    /// Remove a network created by [`Self::create_network`]. Fails if an instance is still attached.
    async fn remove_network(&self, name: &str) -> Result<(), ProviderError>;
}
