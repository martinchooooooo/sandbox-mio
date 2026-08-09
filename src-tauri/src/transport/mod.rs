//! Provider-neutral port-reachability lookup, extracted so `RuntimeManager` and `NetworkManager`
//! share one `podman port` implementation instead of each parsing its output separately.

pub mod types;

use types::{PortTransport, TransportError};

fn parse_host_port(output: &str) -> Option<u16> {
    output
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("127.0.0.1:"))
        .find_map(|p| p.parse().ok())
}

/// Runs `podman port <sandbox> <container_port>/tcp` over `connection` via the bundled-`podman.exe`
/// resolving `crate::backends::podman_command()`, not a fresh `Command::new("podman")` — see
/// `docs/features/mutagen-transport/02_dev.md`, Resolved Decisions.
pub struct PodmanPortTransport;

#[async_trait::async_trait]
impl PortTransport for PodmanPortTransport {
    async fn resolve_remote_port(
        &self,
        sandbox: &str,
        connection: &str,
        container_port: u16,
    ) -> Result<u16, TransportError> {
        let port_spec = format!("{container_port}/tcp");
        let output = crate::backends::podman_command()
            .args(["--connection", connection, "port", sandbox, &port_spec])
            .output()
            .await
            .map_err(|e| TransportError::CommandFailed(e.to_string()))?;
        if !output.status.success() {
            return Err(TransportError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_host_port(&stdout).ok_or_else(|| {
            TransportError::NoHostMapping(format!(
                "no 127.0.0.1 host mapping for container port {container_port}; output:\n{stdout}"
            ))
        })
    }
}

/// Construct the shared [`PortTransport`] for Tauri state. Stateless, so `RuntimeManager` and
/// `NetworkManager` can hold the same `Arc` with no functional requirement that they do.
pub fn default_port_transport() -> std::sync::Arc<dyn PortTransport> {
    std::sync::Arc::new(PodmanPortTransport)
}

/// For an exec-capable sandbox (`Sandbox::supports_exec_transport()`), nothing is published: once
/// the outer SSH/Mutagen client completes its handshake through `podman exec -i sandbox sshd -i`,
/// its remote environment *is* the container's own netns, so `container_port` needs no lookup.
/// Identical to the passthrough `mutagen-transport/01_ideas.md` sketched for a microsandbox
/// implementation of the same trait — see docs/features/podman-exec-transport/02_dev.md.
pub struct PassthroughPortTransport;

#[async_trait::async_trait]
impl PortTransport for PassthroughPortTransport {
    async fn resolve_remote_port(
        &self,
        _sandbox: &str,
        _connection: &str,
        container_port: u16,
    ) -> Result<u16, TransportError> {
        Ok(container_port)
    }
}

/// Construct the shared exec-transport [`PortTransport`] for Tauri state, alongside
/// [`default_port_transport`].
pub fn default_exec_port_transport() -> std::sync::Arc<dyn PortTransport> {
    std::sync::Arc::new(PassthroughPortTransport)
}
