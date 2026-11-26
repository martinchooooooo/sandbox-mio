//! Remote host management for [`super::types::SandboxBackend`]: the Podman system-connection
//! registry, the SSH-side preparation of a Linux server, and the pure parsing of connection and
//! `podman info` output the deploy flow reuses.

use std::sync::Arc;

use tauri::ipc::Channel;

use super::types::ConnectionInfo;
use super::{check, podman_on, run_podman};
use mio_domain::types::BackendError;
use mio_process::detached_command;
use mio_process::types::ProcessRunner;
use crate::ssh::identity::ensure_key;

/// Names of all registered Podman system connections.
pub(super) async fn list(runner: &Arc<dyn ProcessRunner>) -> Result<Vec<String>, BackendError> {
    let out = run_podman(runner, &["system", "connection", "list", "--format", "json"]).await?;
    let stdout = check(out)?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).map_err(|e| BackendError::ParseError(e.to_string()))?;
    Ok(entries
        .into_iter()
        .filter_map(|e| e["Name"].as_str().map(|s| s.to_string()))
        .collect())
}

/// Registers a remote host as a Podman system connection.
pub(super) async fn add(
    runner: &Arc<dyn ProcessRunner>,
    name: &str,
    ssh_url: &str,
    identity: Option<&str>,
) -> Result<(), BackendError> {
    let mut args = vec!["system", "connection", "add"];
    if let Some(identity) = identity {
        args.push("--identity");
        args.push(identity);
    }
    args.push(name);
    args.push(ssh_url);
    check(run_podman(runner, &args).await?).map(|_| ())
}

/// Enables rootless podman.socket and user lingering on a reachable remote host over SSH.
pub(super) async fn prepare(ssh_url: &str, identity_path: &str) -> Result<(), BackendError> {
    let (target, port) = ssh_target(ssh_url)?;
    let mut cmd = detached_command("ssh");
    // BatchMode=yes turns a missing key authorization into a fast failure instead of a
    // password prompt that would hang headless — authorizing the key is a manual
    // prerequisite (`ssh` reads passwords from the TTY, not stdin).
    cmd.arg("-o").arg("BatchMode=yes");
    cmd.arg("-i").arg(identity_path);
    if let Some(port) = port {
        cmd.arg("-p").arg(port);
    }
    cmd.arg(&target);
    // Linger must be enabled (as root) *before* `systemctl --user`, since it is what
    // starts the per-user systemd manager and creates its D-Bus socket; the tutorial's
    // reverse order fails on a host with no active login session. On hosts without
    // passwordless sudo this fails under BatchMode and the user runs it by hand.
    cmd.arg(
        "sudo loginctl enable-linger \"$USER\" \
         && XDG_RUNTIME_DIR=/run/user/$(id -u) systemctl --user enable --now podman.socket",
    );
    let out = cmd
        .output()
        .await
        .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
    check(out).map(|_| ())
}

/// Guided setup for an external Linux server: generate the SSH key, enable the socket, register the
/// connection, verify it, reporting each step on `on_progress`.
pub(super) async fn provision(
    runner: &Arc<dyn ProcessRunner>,
    name: &str,
    ssh_url: &str,
    identity_path: &str,
    on_progress: Channel<serde_json::Value>,
) -> Result<(), BackendError> {
    let step = |label: &str, result: Result<(), BackendError>| -> Result<(), BackendError> {
        match &result {
            Ok(()) => progress(&on_progress, label, "ok", ""),
            Err(e) => progress(&on_progress, label, "error", &e.to_string()),
        }
        result
    };

    step("ensure_key", ensure_key(identity_path))?;
    step("prepare_host", prepare(ssh_url, identity_path).await)?;
    step("add_host", add(runner, name, ssh_url, Some(identity_path)).await)?;
    step("verify_host", verify(runner, name).await)?;
    progress(&on_progress, "done", "ok", "");
    Ok(())
}

async fn verify(runner: &Arc<dyn ProcessRunner>, name: &str) -> Result<(), BackendError> {
    let out = podman_on(runner, name, &["info", "--format", "json"]).await?;
    check(out).map(|_| ())
}

/// Best-effort: a failed send (frontend dropped the channel) must not abort provisioning.
fn progress(channel: &Channel<serde_json::Value>, step: &str, status: &str, message: &str) {
    let _ = channel.send(serde_json::json!({
        "step": step,
        "status": status,
        "message": message,
    }));
}

/// Live SSH endpoint for connection `name`, or `None` for a connection not reachable over SSH.
/// Read fresh every call: a machine's SSH port changes on each boot.
pub(super) async fn connection_info(
    runner: &Arc<dyn ProcessRunner>,
    name: &str,
) -> Result<Option<ConnectionInfo>, BackendError> {
    let out = run_podman(runner, &["system", "connection", "list", "--format", "json"]).await?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&check(out)?).map_err(|e| BackendError::ParseError(e.to_string()))?;
    let entry = entries
        .iter()
        .find(|e| e["Name"].as_str() == Some(name))
        .ok_or_else(|| BackendError::ParseError(format!("no podman connection named {name:?}")))?;
    parse_connection_entry(entry)
}

fn ssh_target(ssh_url: &str) -> Result<(String, Option<String>), BackendError> {
    let rest = ssh_url
        .strip_prefix("ssh://")
        .ok_or_else(|| BackendError::ParseError(format!("expected an ssh:// URL, got: {ssh_url}")))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return Err(BackendError::ParseError(format!("ssh URL is missing a host: {ssh_url}")));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() {
            return Ok((host.to_string(), Some(port.to_string())));
        }
    }
    Ok((authority.to_string(), None))
}

/// Podman 4.0 is the first release taking several `--network` flags with `name:ip=`, which the
/// dual-homed sidecar needs; netavark is the backend proven by the Phase A spike
/// (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 6).
const MIN_REMOTE_PODMAN_MAJOR: u64 = 4;

/// Whether the target's `podman info --format json` describes a host that can run the bounded
/// sandbox, or the reason it cannot. Pure so the floor is unit-tested.
pub(super) fn check_remote_podman(connection: &str, info_json: &str) -> Result<(), BackendError> {
    let unsupported = |reason: String| BackendError::RemoteHostUnsupported {
        connection: connection.to_string(),
        reason,
    };
    let info: serde_json::Value = serde_json::from_str(info_json)
        .map_err(|e| BackendError::ParseError(format!("podman info on {connection:?}: {e}")))?;
    let version = info["version"]["Version"].as_str().unwrap_or_default();
    let major: u64 = version.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
    if major < MIN_REMOTE_PODMAN_MAJOR {
        return Err(unsupported(format!(
            "Podman {version:?} is older than {MIN_REMOTE_PODMAN_MAJOR}.0"
        )));
    }
    let backend = info["host"]["networkBackend"].as_str().unwrap_or_default();
    if backend != "netavark" {
        return Err(unsupported(format!(
            "its network backend is {backend:?}, and the egress boundary needs netavark"
        )));
    }
    Ok(())
}

fn parse_connection_entry(entry: &serde_json::Value) -> Result<Option<ConnectionInfo>, BackendError> {
    let uri = entry["URI"]
        .as_str()
        .ok_or_else(|| BackendError::ParseError("connection entry has no URI".to_string()))?;
    let identity = entry["Identity"].as_str().unwrap_or_default().to_string();
    let Some(authority) = uri.strip_prefix("ssh://").and_then(|r| r.split('/').next()) else {
        // Not an ssh:// connection (e.g. a local unix socket): nothing for Mutagen to sync over.
        return Ok(None);
    };
    let (user, hostport) = authority
        .split_once('@')
        .ok_or_else(|| BackendError::ParseError(format!("connection URI has no user: {uri}")))?;
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            (h.to_string(), p.to_string())
        }
        _ => (hostport.to_string(), "22".to_string()),
    };
    Ok(Some(ConnectionInfo {
        host,
        port,
        user: user.to_string(),
        identity,
    }))
}

#[cfg(test)]
mod remote_podman_tests {
    use super::*;

    fn info(version: &str, backend: &str) -> String {
        format!(r#"{{"version":{{"Version":"{version}"}},"host":{{"networkBackend":"{backend}"}}}}"#)
    }

    #[test]
    fn netavark_podman_4_or_newer_is_accepted() {
        assert!(check_remote_podman("t", &info("4.0.0", "netavark")).is_ok());
        assert!(check_remote_podman("t", &info("5.8.6", "netavark")).is_ok());
    }

    #[test]
    fn an_old_podman_or_cni_is_refused_with_a_reason() {
        for bad in [info("3.4.4", "netavark"), info("5.0.0", "cni"), "{}".to_string()] {
            assert!(matches!(
                check_remote_podman("t", &bad),
                Err(BackendError::RemoteHostUnsupported { .. })
            ));
        }
    }
}
