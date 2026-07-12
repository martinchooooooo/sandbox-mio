/// Errors from SSH config management.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("could not resolve home directory")]
    HomeDirNotFound,
    /// Config-lock mutex was poisoned by a panicking writer.
    #[error("SSH config lock poisoned")]
    LockPoisoned,
    /// File-system operation on SSH config failed.
    #[error("{0}")]
    WriteError(String),
}

/// One OpenSSH `Host` alias for a remote sandbox. Supplies port and identity that Mutagen's
/// `alias:path` URLs cannot embed. Keyed to a Podman connection (many sandboxes share one).
pub struct SshAlias {
    /// The `Host` token Mutagen URLs reference (used in `alias:path` scp-style URLs).
    pub alias: String,
    pub host: String,
    pub port: String,
    pub user: String,
    /// SSH private key path registered with Podman for this connection.
    pub identity: String,
}
