//! Per-sandbox SSH identity: the keypair, the managed alias that reaches the sandbox over the
//! engine's exec transport, and the pinned host keys. Takes no provider: whatever must run inside a
//! sandbox (reading its host keys) is done by the caller and passed in.

use mio_domain::types::{BackendError, ProviderId};
use mio_process::detached_command_std;
use mio_process::podman::podman_literal_path;

/// The account the exec-transport SSH session authenticates as. Must match the base image, which
/// runs everything as root (`/root/work` lives under `/root`).
const CONTAINER_USER: &str = "root";

/// The label appended to a sandbox name to form its SSH alias. Mirrored by `ALIAS_SUFFIX` in
/// `e2e/lib/ssh.ts`.
pub const ALIAS_SUFFIX: &str = ".mio";

/// The SSH `Host` alias of sandbox `name`: the name plus [`ALIAS_SUFFIX`]. Never equals a bare
/// sandbox name, so a managed block cannot take over a host entry of the user's own SSH config
/// (`docs/features/namespaced-ssh-alias/02_dev.md`, Resolved Decision 1). Every write, removal,
/// quote or lookup of an alias goes through here; container-facing calls keep the bare name.
pub fn sandbox_alias(name: &str) -> String {
    format!("{name}{ALIAS_SUFFIX}")
}

/// Generates an ed25519 keypair at `identity_path` unless one exists.
pub fn ensure_key(identity_path: &str) -> Result<(), BackendError> {
    if std::path::Path::new(identity_path).exists() {
        return Ok(());
    }
    let out = detached_command_std("ssh-keygen")
        .args(["-t", "ed25519", "-f", identity_path, "-N", ""])
        .output()
        .map_err(|e| BackendError::CommandFailed(format!("failed to run ssh-keygen: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(BackendError::CommandFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

/// Per-sandbox ed25519 keypair for the exec-transport `Proxied` alias, generated once and reused
/// for the sandbox's lifetime. Lives under app data (not `~/.ssh`), per-sandbox and
/// per-worktree-isolatable via `MIO_APP_DATA_DIR`. Returns the private key path. Re-derivable from
/// the name alone, so `SandboxRouter` (`src-tauri/src/services/routing.rs`) finds an
/// already-created sandbox's identity without persisting the path anywhere new.
pub fn ensure_sandbox_identity(
    app: &tauri::AppHandle,
    name: &str,
) -> Result<String, BackendError> {
    let dir = crate::paths::sandbox_key_dir(app, name)
        .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| BackendError::CommandFailed(format!("failed to create {dir:?}: {e}")))?;
    let identity_path = dir.join("id_ed25519").to_string_lossy().replace('\\', "/");
    ensure_key(&identity_path)?;
    Ok(identity_path)
}

/// Ensure the SSH alias for a Podman sandbox is written to `~/.ssh/sandbox-mio/config`.
/// The identity and installed key last the sandbox's lifetime, but the alias is rewritten on every
/// terminal, agent-web and expose-port open because the alias file may not survive an app restart.
pub async fn ensure_sandbox_alias(
    name: &str,
    connection: &str,
    identity_path: &str,
) -> Result<(), BackendError> {
    let command = podman_proxy_command(name, connection);
    super::write_alias(&super::types::SshAlias::Proxied {
        alias: sandbox_alias(name),
        command,
        user: CONTAINER_USER.to_string(),
        identity: identity_path.to_string(),
        known_hosts: sandbox_known_hosts_path(identity_path),
    })?;
    Ok(())
}

/// The `ProxyCommand` a sandbox's own alias pins on machine `machine_id` of `provider`. The single
/// source for both the alias writes and the string `sandbox_ssh_access` shows the user, so the two
/// cannot diverge.
pub fn sandbox_proxy_command(name: &str, provider: ProviderId, machine_id: &str) -> String {
    match provider {
        ProviderId::Podman => podman_proxy_command(name, machine_id),
    }
}

/// The `ProxyCommand` [`ensure_sandbox_alias`] pins for a Podman sandbox on `connection`.
fn podman_proxy_command(name: &str, connection: &str) -> String {
    // Bare `mio-sshd`, never its absolute path: a pre-existing Mutagen daemon lacks `MSYS_NO_PATHCONV`
    // and would mangle it (`docs/features/podman-exec-transport/02_dev.md`, Resolved Decision 5).
    format!(
        "{} --connection {connection} exec -i {name} mio-sshd",
        podman_literal_path()
    )
}

/// The `UserKnownHostsFile` [`ensure_sandbox_alias`] pins, kept next to the sandbox's own identity.
pub fn sandbox_known_hosts_path(identity_path: &str) -> String {
    match identity_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/known_hosts"),
        None => "known_hosts".to_string(),
    }
}

/// Drop the host key pinned for a sandbox alias. Must run whenever a *fresh* container is created
/// under a name that may have been used before: the sshd host key is baked into the base image, so
/// it changes across an image rebuild while the alias name stays the same. The file stays empty
/// until `backends::seed_sandbox_host_keys` next reaches the container running.
pub fn forget_sandbox_host_key(identity_path: &str) {
    let _ = std::fs::remove_file(sandbox_known_hosts_path(identity_path));
}

/// Renders the pinned `known_hosts` from the host `.pub` files, dropping any line that is not a key.
/// The `[<alias>]:*` pattern matches the port some clients carry, meaningless over a `ProxyCommand`.
fn render_known_hosts(alias: &str, pub_keys: &str) -> String {
    let mut out = String::new();
    for line in pub_keys.lines() {
        let mut fields = line.split_whitespace();
        let (Some(kind), Some(blob)) = (fields.next(), fields.next()) else {
            continue;
        };
        let is_key_type = kind.starts_with("ssh-") || kind.starts_with("ecdsa-");
        let is_base64 = blob.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
        if is_key_type && is_base64 {
            out.push_str(&format!("{alias},[{alias}]:* {kind} {blob}\n"));
        }
    }
    out
}

/// Pins the host keys read from the running container (`pub_keys`, the concatenated `.pub` files)
/// so a strict-checking client connects on first contact
/// (`docs/features/host-tool-ssh-access/02_dev.md`, Resolved Decisions). Best effort: a failure is
/// warn-logged and must never fail a create, deploy or panel fetch.
pub fn pin_host_keys(name: &str, identity_path: &str, pub_keys: &str) {
    let rendered = render_known_hosts(&sandbox_alias(name), pub_keys);
    if rendered.is_empty() {
        tracing::warn!(target: "ssh", %name, "container returned no host keys; not pinning");
        return;
    }
    // A start's background pin and the panel's re-fetch seed concurrently; sharing the temp file
    // unlocked, one could rename the other's half-written copy into place.
    static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = WRITE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = std::path::PathBuf::from(sandbox_known_hosts_path(identity_path));
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == rendered) {
        return;
    }
    let tmp = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, &rendered).and_then(|()| std::fs::rename(&tmp, &path)) {
        tracing::warn!(target: "ssh", %name, error = %e, "writing pinned host keys failed");
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod known_hosts_tests {
    use super::*;

    const ED: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIELO root@abc";
    const RSA: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQAB+/== root@abc";
    const EC: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNh root@abc";

    #[test]
    fn every_key_gets_the_port_agnostic_host_pattern() {
        let out = render_known_hosts("box", &format!("{EC}\n{ED}\n{RSA}\n"));
        assert_eq!(
            out,
            "box,[box]:* ecdsa-sha2-nistp256 AAAAE2VjZHNh\n\
             box,[box]:* ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIELO\n\
             box,[box]:* ssh-rsa AAAAB3NzaC1yc2EAAAADAQAB+/==\n"
        );
    }

    #[test]
    fn empty_or_garbage_output_renders_nothing() {
        assert_eq!(render_known_hosts("box", ""), "");
        assert_eq!(render_known_hosts("box", "cat: /etc/ssh/ssh_host_*_key.pub: No such file\n"), "");
        assert_eq!(render_known_hosts("box", "\n\n not a key\n"), "");
    }

    #[test]
    fn garbage_lines_are_dropped_among_real_keys() {
        let out = render_known_hosts("box", &format!("noise\r\n{ED}\r\n"));
        assert_eq!(out, "box,[box]:* ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIELO\n");
    }
}

#[cfg(test)]
mod alias_tests {
    use super::*;

    #[test]
    fn alias_appends_the_suffix() {
        assert_eq!(sandbox_alias("my-sandbox"), "my-sandbox.mio");
    }

    /// The point of the namespace: no sandbox name, whatever it holds, maps to itself.
    #[test]
    fn alias_never_equals_the_name() {
        for name in ["github.com", "mio", "a", "Work-Laptop", "x.mio"] {
            assert_ne!(sandbox_alias(name), name);
        }
    }

    #[test]
    fn alias_keeps_the_dots_of_the_name() {
        assert_eq!(sandbox_alias("github.com"), "github.com.mio");
    }
}

#[cfg(test)]
mod proxy_command_tests {
    use super::*;

    /// The `--connection` value is what a deploy or undeploy changes, so it must come from the
    /// machine the route resolved rather than from the local default.
    #[test]
    fn podman_proxy_command_execs_on_the_route_machine() {
        let command = sandbox_proxy_command("my-sandbox", ProviderId::Podman, "remote-box");
        assert!(
            command.ends_with(" --connection remote-box exec -i my-sandbox mio-sshd"),
            "{command}"
        );
        assert_eq!(command, podman_proxy_command("my-sandbox", "remote-box"));
    }

    /// A bare `mio-sshd` keeps the argument free of MSYS-manglable absolute paths.
    #[test]
    fn podman_proxy_command_names_mio_sshd_bare() {
        let command = podman_proxy_command("my-sandbox", "podman-machine-default");
        assert!(!command.contains("/usr/local/bin"), "{command}");
    }
}
