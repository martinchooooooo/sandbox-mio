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
/// `alias:path` URLs cannot embed. `Direct` is keyed to a Podman connection (many sandboxes share
/// one); `Proxied` is keyed to a sandbox name (one per sandbox) and backs the podman-exec-transport
/// reachability model (`docs/features/podman-exec-transport/02_dev.md`).
pub enum SshAlias {
    Direct {
        /// The `Host` token Mutagen URLs reference (used in `alias:path` scp-style URLs).
        alias: String,
        host: String,
        port: String,
        user: String,
        /// SSH private key path registered with Podman for this connection.
        identity: String,
    },
    Proxied {
        /// The `Host` token Mutagen URLs reference (used in `alias:path` scp-style URLs).
        alias: String,
        /// `ProxyCommand` execing `sshd -i` inside the sandbox over `podman exec`.
        command: String,
        /// Load-bearing: Mutagen's `alias:path` URLs carry no user, so without this OpenSSH offers
        /// the local Windows account name, which does not exist in the container's `/etc/passwd`.
        user: String,
        /// SSH private key path for the per-sandbox keypair `ProxyCommand`'s in-container `sshd`
        /// trusts. `ProxyCommand` only replaces the transport substrate, so OpenSSH still consults
        /// `IdentityFile` for the pubkey-auth handshake carried over that pipe.
        identity: String,
        /// Per-sandbox `UserKnownHostsFile`. Kept out of `~/.ssh/known_hosts` because the host key
        /// is baked into the base image, so it changes under a fixed alias whenever the image is
        /// rebuilt and a sandbox name is reused.
        known_hosts: String,
    },
}

/// What a host SSH client needs to reach a sandbox through its own `Proxied` alias, returned by
/// `sandbox_ssh_access`. Every value mirrors the managed block as just rewritten, never a cached
/// copy (`docs/features/host-tool-ssh-access/02_dev.md`, Phase A).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshAccess {
    /// The `Host` token, which is the sandbox name.
    pub alias: String,
    /// Private key path of the per-sandbox keypair the alias's `IdentityFile` names.
    pub identity_path: String,
    /// The alias's `UserKnownHostsFile`, pinned next to the identity.
    pub known_hosts_path: String,
    /// The exact `ProxyCommand` in the block. It embeds the machine the sandbox runs on, so a
    /// copy goes stale after a deploy or undeploy while the alias does not.
    pub proxy_command: String,
    /// Whether the sandbox is listed as running on its provider's local machine, by the same rule as
    /// `SandboxRoute::running` (`src-tauri/src/services/routing.rs`): false for a deployed sandbox.
    pub running: bool,
}

impl SshAlias {
    pub fn alias(&self) -> &str {
        match self {
            SshAlias::Direct { alias, .. } | SshAlias::Proxied { alias, .. } => alias,
        }
    }
}
