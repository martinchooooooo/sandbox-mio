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
