pub mod types;

use std::process::{Output, Stdio};

use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use types::{PodmanCliBackend, Sandbox, SandboxBackend};

const BASE_IMAGE: &str = "alpine";
const BASE_IMAGE_VERSION: &str = "latest";

/// The connection `podman machine init` creates for the local machine.
const DEFAULT_CONNECTION: &str = "podman-machine-default";

/// The container user's `~/work`, used as both the workspace bind-mount target and the
/// `--workdir`. Spelled literally because `podman -v` rejects a bare `~` and does not expand
/// `$HOME`, which is `/root` for the Alpine root user.
const WORKSPACE_MOUNT: &str = "/root/work";

/// Root holding every sandbox's workspace directory: `SANDBOX_WORKSPACE_ROOT` if set,
/// else `<Documents>/sandbox-mio`. Documents is resolved at runtime because Windows may
/// redirect it (e.g. under OneDrive).
fn workspace_root() -> Result<std::path::PathBuf, String> {
    if let Ok(root) = std::env::var("SANDBOX_WORKSPACE_ROOT") {
        return Ok(std::path::PathBuf::from(root));
    }
    let documents = dirs::document_dir()
        .ok_or_else(|| "could not resolve the Documents directory".to_string())?;
    Ok(documents.join("sandbox-mio"))
}

/// Rejects names Podman would refuse. Also the path-safety gate for `workspace_path`: the
/// name becomes a path segment, so barring `/` and `..` keeps the workspace inside its root.
fn validate_name(name: &str) -> Result<(), String> {
    let valid = name
        .chars()
        .enumerate()
        .all(|(i, c)| c.is_ascii_alphanumeric() || (i > 0 && matches!(c, '_' | '.' | '-')));
    if name.is_empty() || !valid {
        return Err(format!(
            "invalid sandbox name {name:?}: use letters, digits, '_', '.' or '-', starting with a letter or digit"
        ));
    }
    Ok(())
}

/// The workspace directory for `name`, forward-slashed so `podman -v` accepts it as a Windows
/// path and translates it into the machine VM. Local-only for now; the seam remote support
/// will swap behind to return a remote path.
fn workspace_path(name: &str) -> Result<String, String> {
    let path = workspace_root()?.join(name);
    Ok(path.to_string_lossy().replace('\\', "/"))
}

pub fn default_backend() -> std::sync::Arc<dyn SandboxBackend> {
    let connection =
        std::env::var("PODMAN_CONNECTION").unwrap_or_else(|_| DEFAULT_CONNECTION.to_string());
    std::sync::Arc::new(PodmanCliBackend { connection })
}

/// Returns stdout on success, or the captured stderr as `Err` on a non-zero exit.
/// Podman writes lifecycle errors (no such image, no such container, ...) to stderr.
fn check(out: Output) -> Result<String, String> {
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Splits an `ssh://user@host[:port]/path` URL into its `user@host` target and optional
/// port. The socket path is dropped: it is meaningful to `podman` but not to a bare `ssh`
/// invocation, which is all `prepare_host` needs. IPv6 literals are not handled.
fn ssh_target(ssh_url: &str) -> Result<(String, Option<String>), String> {
    let rest = ssh_url
        .strip_prefix("ssh://")
        .ok_or_else(|| format!("expected an ssh:// URL, got: {ssh_url}"))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return Err(format!("ssh URL is missing a host: {ssh_url}"));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() {
            return Ok((host.to_string(), Some(port.to_string())));
        }
    }
    Ok((authority.to_string(), None))
}

impl PodmanCliBackend {
    fn image() -> String {
        format!("{BASE_IMAGE}:{BASE_IMAGE_VERSION}")
    }

    async fn podman(&self, args: &[&str]) -> Result<Output, String> {
        Command::new("podman")
            .arg("--connection")
            .arg(&self.connection)
            .args(args)
            .output()
            .await
            .map_err(|e| e.to_string())
    }

    /// Runs `podman` without `--connection`, for system-connection registry operations
    /// that are not bound to the backend's active connection.
    async fn podman_global(args: &[&str]) -> Result<Output, String> {
        Command::new("podman")
            .args(args)
            .output()
            .await
            .map_err(|e| e.to_string())
    }

    fn ensure_key(identity_path: &str) -> Result<(), String> {
        if std::path::Path::new(identity_path).exists() {
            return Ok(());
        }
        let out = std::process::Command::new("ssh-keygen")
            .args(["-t", "ed25519", "-f", identity_path, "-N", ""])
            .output()
            .map_err(|e| format!("failed to run ssh-keygen: {e}"))?;
        check(out).map(|_| ())
    }

    async fn verify_host(name: &str) -> Result<(), String> {
        let out = Self::podman_global(&["--connection", name, "info", "--format", "json"]).await?;
        check(out).map(|_| ())
    }
}

/// Best-effort: a failed send (frontend dropped the channel) must not abort provisioning.
fn progress(channel: &Channel<serde_json::Value>, step: &str, status: &str, message: &str) {
    let _ = channel.send(serde_json::json!({
        "step": step,
        "status": status,
        "message": message,
    }));
}

#[async_trait::async_trait]
impl SandboxBackend for PodmanCliBackend {
    async fn create(&self, name: &str) -> Result<Sandbox, String> {
        validate_name(name)?;
        let image = Self::image();
        let workspace = workspace_path(name)?;
        // Pre-created locally so an unwritable path fails here, not silently via podman's
        // auto-creation of the mount source.
        std::fs::create_dir_all(&workspace)
            .map_err(|e| format!("failed to create workspace {workspace}: {e}"))?;
        let volume = format!("{workspace}:{WORKSPACE_MOUNT}");
        // `--pull=missing` satisfies the trait's "pull if absent, then create" contract.
        let out = self
            .podman(&[
                "create", "--pull=missing", "--tty", "--volume", &volume, "--workdir",
                WORKSPACE_MOUNT, "--name", name, &image, "/bin/sh",
            ])
            .await?;
        let id = check(out)?.trim().to_string();
        Ok(Sandbox {
            id,
            name: name.to_string(),
            status: "created".to_string(),
            workspace_path: workspace,
        })
    }

    async fn list(&self) -> Result<Vec<Sandbox>, String> {
        let out = self.podman(&["ps", "--all", "--format", "json"]).await?;
        let stdout = check(out)?;
        let containers: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| e.to_string())?;
        Ok(containers
            .into_iter()
            .map(|c| {
                let name = c["Names"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim_start_matches('/')
                    .to_string();
                // Derived from the name, not the container's mount JSON, so it is set even
                // for sandboxes created outside this app.
                let workspace_path = workspace_path(&name).unwrap_or_default();
                Sandbox {
                    id: c["Id"].as_str().unwrap_or_default().to_string(),
                    status: c["State"].as_str().unwrap_or_default().to_string(),
                    name,
                    workspace_path,
                }
            })
            .collect())
    }

    async fn start(&self, name: &str) -> Result<(), String> {
        check(self.podman(&["start", name]).await?).map(|_| ())
    }

    async fn stop(&self, name: &str) -> Result<(), String> {
        check(self.podman(&["stop", name]).await?).map(|_| ())
    }

    async fn listen_events(&self, on_event: Channel<serde_json::Value>) -> Result<(), String> {
        let mut child = Command::new("podman")
            .arg("--connection")
            .arg(&self.connection)
            .args(["events", "--format", "json"])
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;

        let stdout = child.stdout.take().ok_or_else(|| "no stdout".to_string())?;
        let mut lines = BufReader::new(stdout).lines();

        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            on_event.send(value).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    async fn list_hosts(&self) -> Result<Vec<String>, String> {
        let out = Self::podman_global(&["system", "connection", "list", "--format", "json"]).await?;
        let stdout = check(out)?;
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| e.to_string())?;
        Ok(entries
            .into_iter()
            .filter_map(|e| e["Name"].as_str().map(|s| s.to_string()))
            .collect())
    }

    async fn add_host(
        &self,
        name: &str,
        ssh_url: &str,
        identity: Option<&str>,
    ) -> Result<(), String> {
        let mut args = vec!["system", "connection", "add"];
        if let Some(identity) = identity {
            args.push("--identity");
            args.push(identity);
        }
        args.push(name);
        args.push(ssh_url);
        check(Self::podman_global(&args).await?).map(|_| ())
    }

    async fn prepare_host(&self, ssh_url: &str, identity_path: &str) -> Result<(), String> {
        let (target, port) = ssh_target(ssh_url)?;
        let mut cmd = Command::new("ssh");
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
        let out = cmd.output().await.map_err(|e| e.to_string())?;
        check(out).map(|_| ())
    }

    async fn provision_host(
        &self,
        name: &str,
        ssh_url: &str,
        identity_path: &str,
        on_progress: Channel<serde_json::Value>,
    ) -> Result<(), String> {
        let step = |label: &str, result: Result<(), String>| -> Result<(), String> {
            match &result {
                Ok(()) => progress(&on_progress, label, "ok", ""),
                Err(e) => progress(&on_progress, label, "error", e),
            }
            result
        };

        step("ensure_key", Self::ensure_key(identity_path))?;
        step(
            "prepare_host",
            self.prepare_host(ssh_url, identity_path).await,
        )?;
        step(
            "add_host",
            self.add_host(name, ssh_url, Some(identity_path)).await,
        )?;
        step("verify_host", Self::verify_host(name).await)?;
        progress(&on_progress, "done", "ok", "");
        Ok(())
    }
}
