pub mod types;

mod mock;

use std::process::{Output, Stdio};

use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use mock::MockBackend;
use crate::process_ext::NoWindow;
use types::{
    BackendError, ConnectionInfo, PodmanCliBackend, RemoteDeployment, Sandbox, SandboxBackend,
    SyncStrategy, WorkspacePlan,
};

const BASE_IMAGE: &str = "sandbox-mio/base";
/// Bump whenever the Containerfile changes: `ensure_base_image` short-circuits on an existing
/// tag, so an unchanged version silently keeps stale images.
pub(crate) const BASE_IMAGE_VERSION: u32 = 4;

const CONTAINERFILE: &str = include_str!("../../resources/Containerfile");

/// The connection `podman machine init` creates for the local machine.
pub(crate) const DEFAULT_CONNECTION: &str = "podman-machine-default";

pub(crate) fn base_image() -> String {
    format!("{BASE_IMAGE}:{BASE_IMAGE_VERSION}")
}

fn parse_image_version(image: &str) -> Option<u32> {
    image.rsplit(':').next()?.parse().ok()
}

/// Spelled literally because `podman -v` rejects a bare `~` and does not expand `$HOME`.
const WORKSPACE_MOUNT: &str = "/root/work";

const WORKSPACE_LABEL: &str = "sandbox-mio.workspace";

const ZELLIJ_WEB_PORT: u16 = 8082;

const PASEO_WEB_PORT: u16 = 6767;

fn workspace_root() -> Result<std::path::PathBuf, BackendError> {
    if let Ok(root) = std::env::var("SANDBOX_WORKSPACE_ROOT") {
        return Ok(std::path::PathBuf::from(root));
    }
    let documents = dirs::document_dir().ok_or(BackendError::DocumentsDirNotFound)?;
    Ok(documents.join("sandbox-mio"))
}

/// Rejects names Podman would refuse. Also the path-safety gate for `workspace_path`: the name
/// becomes a path segment, so barring `/` and `..` keeps auto-created workspaces inside the root.
fn validate_name(name: &str) -> Result<(), BackendError> {
    let valid = name
        .chars()
        .enumerate()
        .all(|(i, c)| c.is_ascii_alphanumeric() || (i > 0 && matches!(c, '_' | '.' | '-')));
    if name.is_empty() || !valid {
        return Err(BackendError::InvalidName { name: name.to_string() });
    }
    Ok(())
}

fn workspace_path(name: &str) -> Result<String, BackendError> {
    let path = workspace_root()?.join(name);
    Ok(path.to_string_lossy().replace('\\', "/"))
}

pub fn default_backend() -> std::sync::Arc<dyn SandboxBackend> {
    if mock_backend_enabled() {
        return std::sync::Arc::new(MockBackend::new());
    }
    let connection =
        std::env::var("PODMAN_CONNECTION").unwrap_or_else(|_| DEFAULT_CONNECTION.to_string());
    std::sync::Arc::new(PodmanCliBackend { connection, build_lock: tokio::sync::Mutex::new(()) })
}

pub(crate) fn mock_backend_enabled() -> bool {
    std::env::var("MIO_MOCK_BACKEND").ok().as_deref() == Some("1")
}

static PODMAN_RESOURCE_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Record the bundled Podman resource directory for [`podman_command`] to use. Called exactly
/// once, at startup, before any subsystem shells out to podman.
pub fn set_podman_resource_dir(dir: std::path::PathBuf) {
    let _ = PODMAN_RESOURCE_DIR.set(dir);
}

pub(crate) fn podman_command() -> Command {
    let mut cmd = match PODMAN_RESOURCE_DIR.get() {
        Some(dir) => {
            let mut cmd = Command::new(dir.join("podman.exe"));
            cmd.env("CONTAINERS_HELPER_BINARY_DIR", dir);
            cmd
        }
        None => Command::new("podman"),
    };
    cmd.no_window();
    cmd
}

/// Returns stdout on success, or the captured stderr as `Err` on a non-zero exit.
/// Podman writes lifecycle errors (no such image, no such container, ...) to stderr.
fn check(out: Output) -> Result<String, BackendError> {
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(BackendError::CommandFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
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

impl PodmanCliBackend {
    fn image() -> String {
        base_image()
    }

    async fn podman(&self, args: &[&str]) -> Result<Output, BackendError> {
        podman_command()
            .arg("--connection")
            .arg(&self.connection)
            .args(args)
            .output()
            .await
            .map_err(|e| BackendError::CommandFailed(e.to_string()))
    }

    /// Runs `podman` without `--connection`, for system-connection registry operations
    /// that are not bound to the backend's active connection.
    async fn podman_global(args: &[&str]) -> Result<Output, BackendError> {
        podman_command()
            .args(args)
            .output()
            .await
            .map_err(|e| BackendError::CommandFailed(e.to_string()))
    }

    /// Runs `podman --connection <connection> <args>` against an arbitrary connection. Used by
    /// deploy, whose target connection differs from the backend's pinned `self.connection`.
    async fn podman_on(connection: &str, args: &[&str]) -> Result<Output, BackendError> {
        let mut full = vec!["--connection", connection];
        full.extend_from_slice(args);
        Self::podman_global(&full).await
    }

    fn ensure_key(identity_path: &str) -> Result<(), BackendError> {
        if std::path::Path::new(identity_path).exists() {
            return Ok(());
        }
        let out = std::process::Command::new("ssh-keygen")
            .no_window()
            .args(["-t", "ed25519", "-f", identity_path, "-N", ""])
            .output()
            .map_err(|e| BackendError::CommandFailed(format!("failed to run ssh-keygen: {e}")))?;
        check(out).map(|_| ())
    }

    async fn verify_host(name: &str) -> Result<(), BackendError> {
        let out = Self::podman_global(&["--connection", name, "info", "--format", "json"]).await?;
        check(out).map(|_| ())
    }

    /// Ensures the mise base image exists on `connection`, building from the bundled Containerfile
    /// if absent. The image is never published to any registry, so absence always means build.
    /// Called by `ensure_base_image` (local, serialized under the build lock) and `deploy` (remote,
    /// no lock — concurrent deploys to the same target are harmless since `podman build` is idempotent).
    async fn ensure_image_on(connection: &str) -> Result<(), BackendError> {
        let image = Self::image();
        if Self::podman_on(connection, &["image", "exists", &image]).await?.status.success() {
            return Ok(());
        }
        // A temp context dir is needed because `podman build -` does not stream the Containerfile
        // through `--connection` to a remote machine. The connection name disambiguates concurrent
        // local+remote builds sharing the same process ID.
        let context = std::env::temp_dir().join(format!(
            "sandbox-mio-build-{pid}-{conn}",
            pid = std::process::id(),
            conn = connection.replace(['/', '\\', ':'], "-"),
        ));
        std::fs::create_dir_all(&context)
            .and_then(|_| std::fs::write(context.join("Containerfile"), CONTAINERFILE))
            .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
        let context_arg = context.to_string_lossy().replace('\\', "/");
        let result = check(Self::podman_on(connection, &["build", "--tag", &image, &context_arg]).await?).map(|_| ());
        let _ = std::fs::remove_dir_all(&context);
        result
    }

    /// Live SSH endpoint for connection `name`, or `None` for a connection not reachable over SSH.
    /// Read fresh every call: a machine's SSH port changes on each boot.
    async fn remote_connection_info(name: &str) -> Result<Option<ConnectionInfo>, BackendError> {
        let out = Self::podman_global(&["system", "connection", "list", "--format", "json"]).await?;
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&check(out)?).map_err(|e| BackendError::ParseError(e.to_string()))?;
        let entry = entries
            .iter()
            .find(|e| e["Name"].as_str() == Some(name))
            .ok_or_else(|| BackendError::ParseError(format!("no podman connection named {name:?}")))?;
        parse_connection_entry(entry)
    }

    /// Create `dir` on the remote host over the connection's SSH identity. Used instead of the
    /// laptop-side `std::fs::create_dir_all` because a remote bind-mount source lives in the
    /// remote filesystem.
    async fn remote_mkdir(info: &ConnectionInfo, dir: &str) -> Result<(), BackendError> {
        let out = Command::new("ssh")
            .no_window()
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg("-i")
            .arg(&info.identity)
            .arg("-p")
            .arg(&info.port)
            .arg(format!("{}@{}", info.user, info.host))
            .arg(format!("mkdir -p '{dir}'"))
            .output()
            .await
            .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
        check(out).map(|_| ())
    }

    /// Resolve where a freshly created sandbox's workspace lives. `None` derives and creates a
    /// fresh directory under `workspace_root()` (today's behavior). `Some(p)` canonicalizes `p`
    /// via `dunce::canonicalize` (plain `std::fs::canonicalize` would return a Windows `\\?\`
    /// UNC-prefixed path that `podman -v` does not understand), rejects it if not found or not a
    /// directory, and rejects it as `PathAlreadyTracked` if it falls inside `workspace_root()` or
    /// exactly matches an already-tracked sandbox's workspace — without touching the filesystem
    /// further.
    async fn resolve_create_workspace(
        &self,
        name: &str,
        existing_path: Option<&str>,
    ) -> Result<String, BackendError> {
        let Some(p) = existing_path else {
            // Pre-created so an unwritable path fails here, not silently via podman's
            // auto-creation of the mount source.
            let workspace = workspace_path(name)?;
            std::fs::create_dir_all(&workspace).map_err(|e| BackendError::WorkspaceCreate {
                path: workspace.clone(),
                error: e.to_string(),
            })?;
            return Ok(workspace);
        };
        let canonical = dunce::canonicalize(p)
            .map_err(|_| BackendError::PathNotFound { path: p.to_string() })?;
        if !canonical.is_dir() {
            return Err(BackendError::PathNotADirectory { path: p.to_string() });
        }
        let root = workspace_root()?;
        let canonical_root = dunce::canonicalize(&root).unwrap_or(root);
        if canonical.starts_with(&canonical_root) {
            return Err(BackendError::PathAlreadyTracked { path: p.to_string() });
        }
        let normalized = canonical.to_string_lossy().replace('\\', "/");
        if self.list().await?.iter().any(|s| s.workspace_path == normalized) {
            return Err(BackendError::PathAlreadyTracked { path: p.to_string() });
        }
        Ok(normalized)
    }

    /// Resolve where `name`'s workspace lives on `connection` and how it reaches the laptop. A
    /// local connection returns equal paths and a no-op strategy; a remote connection returns a
    /// remote bind-mount source and the live SSH endpoint. Called by `deploy` against the chosen
    /// target connection to compute the remote endpoint; `create` is always local.
    async fn resolve_workspace(name: &str, connection: &str) -> Result<WorkspacePlan, BackendError> {
        let laptop_dir = workspace_path(name)?;
        let Some(info) = Self::remote_connection_info(connection).await? else {
            // Local machine: bind-mount source and laptop directory are the same, no sync.
            return Ok(WorkspacePlan {
                bind_mount_source: laptop_dir.clone(),
                laptop_dir,
                strategy: SyncStrategy::Local,
            });
        };
        // Root logs into `/root`; every other user into `/home/<user>`.
        let home = if info.user == "root" {
            "/root".to_string()
        } else {
            format!("/home/{}", info.user)
        };
        Ok(WorkspacePlan {
            bind_mount_source: format!("{home}/work/{name}"),
            laptop_dir,
            strategy: SyncStrategy::Remote(info),
        })
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
    async fn ensure_base_image(&self) -> Result<(), BackendError> {
        // Held across the whole check+build so concurrent local creates serialize: the first
        // builder wins while the rest wait, then each re-checks `image exists` inside
        // `ensure_image_on` and returns immediately — without it they would race the build.
        let _guard = self.build_lock.lock().await;
        Self::ensure_image_on(&self.connection).await
    }

    async fn create(&self, name: &str, existing_path: Option<&str>) -> Result<Sandbox, BackendError> {
        validate_name(name)?;
        let image = Self::image();
        // Every sandbox is created locally; remoteness is a later deploy action, so the workspace
        // is always a laptop directory bind-mounted directly.
        let workspace = self.resolve_create_workspace(name, existing_path).await?;
        let volume = format!("{workspace}:{WORKSPACE_MOUNT}");
        let label = format!("{WORKSPACE_LABEL}={workspace}");
        // Publish zellij's web port to an ephemeral host-loopback port. `--host-lo-to-ns-lo` is the
        // decisive pasta flag: it forwards the host-loopback-published port to the container's
        // loopback, where zellij binds; without it pasta forwards to the container's eth0 and the
        // loopback-bound server is unreachable. The host side is pinned to 127.0.0.1 so nothing is
        // exposed on the local network. Both local and remote containers get these flags.
        //
        // Podman only forwards ports it is explicitly told about (confirmed: with no `--publish`
        // it invokes pasta with `-t none -u none`, forwarding nothing) — there is no way to reach a
        // container's port from outside its netns without publishing it first. `relay_publishes`
        // pre-publishes a small pool of container-side ports (crate::network::RELAY_POOL_SIZE) that
        // `NetworkManager::expose_port` can later bridge to an arbitrary user-chosen port via an
        // in-container `socat` relay, without ever recreating the container.
        let publish = format!("127.0.0.1::{ZELLIJ_WEB_PORT}");
        let publish_paseo = format!("127.0.0.1::{PASEO_WEB_PORT}");
        let relay_publishes = crate::network::relay_pool_publish_args();
        let mut args = vec![
            "create".to_string(), "--pull=missing".to_string(), "--tty".to_string(),
            "--network".to_string(), "pasta:--host-lo-to-ns-lo".to_string(),
            "--publish".to_string(), publish, "--publish".to_string(), publish_paseo,
        ];
        for p in &relay_publishes {
            args.push("--publish".to_string());
            args.push(p.clone());
        }
        args.extend([
            "--volume".to_string(), volume, "--workdir".to_string(), WORKSPACE_MOUNT.to_string(),
            "--label".to_string(), label, "--name".to_string(), name.to_string(),
            image, "/bin/sh".to_string(),
        ]);
        // `--pull=missing` satisfies the trait's "pull if absent, then create" contract.
        let out = self
            .podman(&args.iter().map(String::as_str).collect::<Vec<_>>())
            .await?;
        let id = check(out)?.trim().to_string();
        Ok(Sandbox {
            id,
            name: name.to_string(),
            status: "created".to_string(),
            workspace_path: workspace,
            image_version: Some(BASE_IMAGE_VERSION),
        })
    }

    async fn list(&self) -> Result<Vec<Sandbox>, BackendError> {
        let out = self.podman(&["ps", "--all", "--format", "json"]).await?;
        let stdout = check(out)?;
        let containers: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| BackendError::ParseError(e.to_string()))?;
        // Deployment state is not derived here: a deployed sandbox is identified by its Mutagen
        // session (see the deploy flow), keeping `list` independent of the sync engine.
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
                // Prefer the label written at `create` time, since an `existing_path` sandbox's
                // workspace is not under `workspace_root()` and cannot be reconstructed from the
                // name alone. Falls back to the name-derived path for containers created before
                // this label existed, or outside this app.
                let workspace_path = c["Labels"]
                    .get(WORKSPACE_LABEL)
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| workspace_path(&name).unwrap_or_default());
                let image_version =
                    parse_image_version(c["Image"].as_str().unwrap_or_default());
                Sandbox {
                    id: c["Id"].as_str().unwrap_or_default().to_string(),
                    status: c["State"].as_str().unwrap_or_default().to_string(),
                    name,
                    workspace_path,
                    image_version,
                }
            })
            .collect())
    }

    async fn start(&self, name: &str) -> Result<(), BackendError> {
        check(self.podman(&["start", name]).await?).map(|_| ())
    }

    async fn stop(&self, name: &str) -> Result<(), BackendError> {
        check(self.podman(&["stop", name]).await?).map(|_| ())
    }

    async fn destroy(&self, name: &str) -> Result<(), BackendError> {
        // `--force` stops a running container first. The workspace directory is left intact.
        check(self.podman(&["rm", "--force", name]).await?).map(|_| ())
    }

    async fn deploy(
        &self,
        name: &str,
        target_connection: &str,
    ) -> Result<RemoteDeployment, BackendError> {
        validate_name(name)?;
        // Deploying onto the source connection would name-collide with the kept-stopped local
        // container and is pointless; any other connection (including a different local machine)
        // is a valid target.
        if target_connection == self.connection {
            return Err(BackendError::SelfDeploy {
                name: name.to_string(),
                connection: target_connection.to_string(),
            });
        }
        // Resolve the remote endpoint first, so an unreachable target fails before any mutation of
        // the local container.
        let plan = Self::resolve_workspace(name, target_connection).await?;
        let SyncStrategy::Remote(info) = plan.strategy else {
            return Err(BackendError::NoSshEndpoint {
                connection: target_connection.to_string(),
            });
        };

        // Stop the workload onto the remote; kept stopped (not removed) so undeploy can restart it.
        check(self.podman(&["stop", name]).await?)?;
        // No committed-image transfer: software is reproduced from the mise manifest by
        // `RuntimeManager::reproduce_remote` after this returns. Must not be reintroduced.
        let image = Self::image();

        // Create the remote workspace and register the OpenSSH alias so Mutagen can reach it.
        Self::remote_mkdir(&info, &plan.bind_mount_source).await?;
        crate::ssh::write_alias(&crate::ssh::types::SshAlias {
            alias: target_connection.to_string(),
            host: info.host.clone(),
            port: info.port.clone(),
            user: info.user.clone(),
            identity: info.identity.clone(),
        })?;

        // Ensure the base image exists on the remote. It is never published to any registry, so
        // absence means build — the same path `ensure_base_image` takes for the local connection.
        Self::ensure_image_on(target_connection).await?;

        // Create and start the remote container on the target connection, not self.connection.
        let volume = format!("{}:{WORKSPACE_MOUNT}", plan.bind_mount_source);
        let publish = format!("127.0.0.1::{ZELLIJ_WEB_PORT}");
        let publish_paseo = format!("127.0.0.1::{PASEO_WEB_PORT}");
        let relay_publishes = crate::network::relay_pool_publish_args();
        let mut create_args = vec![
            "create".to_string(), "--tty".to_string(), "--network".to_string(),
            "pasta:--host-lo-to-ns-lo".to_string(), "--publish".to_string(), publish,
            "--publish".to_string(), publish_paseo,
        ];
        for p in &relay_publishes {
            create_args.push("--publish".to_string());
            create_args.push(p.clone());
        }
        create_args.extend([
            "--volume".to_string(), volume, "--workdir".to_string(), WORKSPACE_MOUNT.to_string(),
            "--name".to_string(), name.to_string(), image.clone(), "/bin/sh".to_string(),
        ]);
        check(
            Self::podman_on(
                target_connection,
                &create_args.iter().map(String::as_str).collect::<Vec<_>>(),
            )
            .await?,
        )?;
        check(Self::podman_on(target_connection, &["start", name]).await?)?;

        Ok(RemoteDeployment {
            alpha: plan.laptop_dir,
            beta: format!("{target_connection}:{}", plan.bind_mount_source),
        })
    }

    async fn remove_remote(&self, name: &str, connection: &str) -> Result<(), BackendError> {
        check(Self::podman_on(connection, &["rm", "--force", name]).await?).map(|_| ())
    }

    async fn listen_events(&self, on_event: Channel<serde_json::Value>) -> Result<(), BackendError> {
        let mut child = podman_command()
            .arg("--connection")
            .arg(&self.connection)
            .args(["events", "--format", "json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BackendError::CommandFailed(e.to_string()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BackendError::CommandFailed("no stdout".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| BackendError::CommandFailed("no stderr".to_string()))?;
        // Drained on its own task rather than left inherited: this call runs unconditionally at app
        // startup (`ensureActivity` in `src/routes/+layout.svelte`), before any Podman-readiness
        // check, so a connection-not-found failure must not leak raw podman stderr to the app's own
        // console — it has to come back as a `BackendError` the caller can swallow.
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = tokio::io::AsyncReadExt::read_to_string(&mut BufReader::new(stderr), &mut buf).await;
            buf
        });

        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await.map_err(|e| BackendError::CommandFailed(e.to_string()))? {
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value =
                serde_json::from_str(&line).map_err(|e| BackendError::ParseError(e.to_string()))?;
            on_event
                .send(value)
                .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
        }

        let status = child.wait().await.map_err(|e| BackendError::CommandFailed(e.to_string()))?;
        if !status.success() {
            let stderr = stderr_task.await.unwrap_or_default();
            return Err(BackendError::CommandFailed(stderr.trim().to_string()));
        }
        Ok(())
    }

    async fn list_hosts(&self) -> Result<Vec<String>, BackendError> {
        let out = Self::podman_global(&["system", "connection", "list", "--format", "json"]).await?;
        let stdout = check(out)?;
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| BackendError::ParseError(e.to_string()))?;
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
    ) -> Result<(), BackendError> {
        let mut args = vec!["system", "connection", "add"];
        if let Some(identity) = identity {
            args.push("--identity");
            args.push(identity);
        }
        args.push(name);
        args.push(ssh_url);
        check(Self::podman_global(&args).await?).map(|_| ())
    }

    async fn prepare_host(&self, ssh_url: &str, identity_path: &str) -> Result<(), BackendError> {
        let (target, port) = ssh_target(ssh_url)?;
        let mut cmd = Command::new("ssh");
        cmd.no_window();
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

    async fn provision_host(
        &self,
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

/// Ensure the SSH alias for `connection` is written to `~/.ssh/sandbox-mio/config` so Mutagen
/// can resolve it for forwarding sessions. Read fresh from `podman system connection list` each
/// call so the port (reassigned on each machine boot) is always current. Called before any
/// `mutagen forward create` that targets `connection`.
pub async fn ensure_connection_alias(connection: &str) -> Result<(), BackendError> {
    let Some(info) = PodmanCliBackend::remote_connection_info(connection).await? else {
        return Err(BackendError::NoSshEndpoint { connection: connection.to_string() });
    };
    crate::ssh::write_alias(&crate::ssh::types::SshAlias {
        alias: connection.to_string(),
        host: info.host,
        port: info.port,
        user: info.user,
        identity: info.identity,
    })?;
    Ok(())
}
