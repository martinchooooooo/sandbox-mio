use std::process::Output;

use crate::backends::types::{BackendError, Sandbox};

/// Errors from machine-provider operations. Mirrors [`BackendError`]'s process/parse variants so it
/// converts into the caller's error type at the subsystem boundary with no new IPC values.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Engine process failed or exited non-zero (captured stderr).
    #[error("{0}")]
    CommandFailed(String),
    /// Failed to parse the engine's JSON or text output.
    #[error("{0}")]
    ParseError(String),
}

impl From<ProviderError> for BackendError {
    fn from(e: ProviderError) -> Self {
        match e {
            ProviderError::CommandFailed(m) => BackendError::CommandFailed(m),
            ProviderError::ParseError(m) => BackendError::ParseError(m),
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

/// The container-or-VM primitive seam shared by every subsystem that drives one instance engine.
/// Declares only operations that generalise across a container engine and a microVM engine; all
/// engine-specific argv lives in the implementation. The `PortTransport` seam
/// (`src-tauri/src/transport/`) covers published-port lookup, so this trait deliberately omits it.
#[async_trait::async_trait]
pub trait MachineProvider: Send + Sync {
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
