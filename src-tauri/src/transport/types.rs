use async_trait::async_trait;

/// Errors from [`PortTransport`]. Callers convert via `.to_string()` into their own error type
/// since `RuntimeError`/`NetworkError` each want a different wrapper variant for the same failure.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("{0}")]
    CommandFailed(String),
    #[error("no host mapping for port {0}")]
    NoHostMapping(String),
}

/// Resolves the port a Mutagen forward's far end must dial to reach a container-side port, so
/// `RuntimeManager` and `NetworkManager` share one lookup instead of each reimplementing `podman
/// port` parsing. [`crate::transport::PodmanPortTransport`] mirrors `podman port <sandbox>
/// <container_port>/tcp` for the publish+relay model; [`crate::transport::PassthroughPortTransport`]
/// serves the exec transport, where the forward already lands inside the container.
#[async_trait]
pub trait PortTransport: Send + Sync {
    async fn resolve_remote_port(
        &self,
        sandbox: &str,
        connection: &str,
        container_port: u16,
    ) -> Result<u16, TransportError>;
}
