/// Errors produced by SSH config management ([`crate::ssh`]).
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("could not resolve home directory")]
    HomeDirNotFound,
    /// The config-lock mutex was poisoned by a panicking writer.
    #[error("SSH config lock poisoned")]
    LockPoisoned,
    /// Any file-system operation on the SSH config (create, write, rename).
    #[error("{0}")]
    WriteError(String),
}

/// One OpenSSH `Host` alias supplying the port and identity Mutagen's `alias:path` URLs cannot.
/// Keyed to a Podman host (connection), not a sandbox: many sandboxes on one host share an
/// endpoint. Written by [`crate::ssh::write_alias`].
pub struct SshAlias {
    /// The `Host` token Mutagen URLs reference (`alias:path`). Also the per-block marker key.
    pub alias: String,
    pub host: String,
    pub port: String,
    pub user: String,
    /// Path to the SSH private key Podman registered for the connection.
    pub identity: String,
}
