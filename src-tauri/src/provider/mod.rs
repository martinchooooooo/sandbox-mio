// Unconstructed outside its own unit tests until Phase J's `provider_for` resolver makes
// `MsbProvider` reachable from `lib.rs` — Tauri can only manage one `Arc<dyn MachineProvider>` of
// state until Phase J replaces that with a per-operation resolver
// (`docs/features/microsandbox-provider/02_dev.md`, Phase E's `MachineRef` section).
#[allow(dead_code)]
pub mod msb;
pub mod types;

use std::process::{Output, Stdio};
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::backends::{base_image, podman_command};
use crate::mockctl::types::ProcessRunner;
use types::{
    ExecOpts, InstanceSpec, MachineProvider, ProviderCapabilities, ProviderError, PublishModel,
};

/// Spelled literally because `podman -v` rejects a bare `~` and does not expand `$HOME`.
const WORKSPACE_MOUNT: &str = "/root/work";

const ZELLIJ_WEB_PORT: u16 = 8082;

const PASEO_WEB_PORT: u16 = 6767;

const CONTAINERFILE: &str = include_str!("../../resources/Containerfile");

/// The label key `create_instance` stamps and `list_instances` reads back to recover an
/// `existing_path` sandbox's workspace, which is not reconstructible from the name alone.
pub(crate) const WORKSPACE_LABEL: &str = "sandbox-mio.workspace";

/// Construct the Podman-backed [`MachineProvider`] pinned to `connection`. Remote-target operations
/// (`deploy` / `remove_remote`) build a second provider pinned to their target connection.
pub fn default_provider(
    runner: Arc<dyn ProcessRunner>,
    connection: String,
) -> Arc<dyn MachineProvider> {
    Arc::new(PodmanProvider {
        connection,
        runner,
        build_lock: tokio::sync::Mutex::new(()),
    })
}

/// Real [`MachineProvider`] driving the `podman` CLI over a pinned system connection. One-shot calls
/// route through `runner` (scripted under `MIO_MOCK_BACKEND=1`); the piped/streamed paths spawn
/// `podman_command()` directly, which the runner cannot represent.
pub(crate) struct PodmanProvider {
    connection: String,
    /// The process seam every one-shot `podman` call routes through — real in production, scripted
    /// under `MIO_MOCK_BACKEND=1`. See `docs/features/scriptable-mock-backend/02_dev.md`.
    runner: Arc<dyn ProcessRunner>,
    /// Serializes base image build to prevent concurrent races.
    build_lock: tokio::sync::Mutex<()>,
}

/// Returns stdout on success, or the captured stderr as `Err` on a non-zero exit.
/// Podman writes lifecycle errors (no such image, no such container, ...) to stderr.
fn check(out: Output) -> Result<String, ProviderError> {
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(ProviderError::CommandFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

fn parse_image_version(image: &str) -> Option<u32> {
    image.rsplit(':').next()?.parse().ok()
}

impl PodmanProvider {
    /// Runs `podman --connection <connection> <argv>` through the mockable runner seam.
    async fn run(&self, argv: &[&str]) -> Result<Output, ProviderError> {
        let mut full = vec!["--connection", &self.connection];
        full.extend_from_slice(argv);
        self.runner
            .run(&full)
            .await
            .map_err(|e| ProviderError::CommandFailed(e.to_string()))
    }

    async fn image_exists(&self) -> Result<bool, ProviderError> {
        Ok(self
            .run(&["image", "exists", &base_image()])
            .await?
            .status
            .success())
    }

    /// Ensures the mise base image exists on the pinned connection, building from the bundled
    /// Containerfile if absent. The image is never published to any registry, so absence always
    /// means build. `ensure_base_image` serializes this under the build lock; `deploy` drives it on
    /// a fresh target-pinned provider whose lock is uncontended (concurrent remote builds are
    /// harmless — `podman build` is idempotent).
    async fn ensure_image(&self) -> Result<(), ProviderError> {
        let image = base_image();
        if self.image_exists().await? {
            return Ok(());
        }
        // A temp context dir is needed because `podman build -` does not stream the Containerfile
        // through `--connection` to a remote machine. The connection name disambiguates concurrent
        // local+remote builds sharing the same process ID.
        let context = std::env::temp_dir().join(format!(
            "sandbox-mio-build-{pid}-{conn}",
            pid = std::process::id(),
            conn = self.connection.replace(['/', '\\', ':'], "-"),
        ));
        std::fs::create_dir_all(&context)
            .and_then(|_| std::fs::write(context.join("Containerfile"), CONTAINERFILE))
            .map_err(|e| ProviderError::CommandFailed(e.to_string()))?;
        let context_arg = context.to_string_lossy().replace('\\', "/");
        let result =
            check(self.run(&["build", "--tag", &image, &context_arg]).await?).map(|_| ());
        let _ = std::fs::remove_dir_all(&context);
        result
    }

    /// Configured, unspawned `podman --connection <c> exec` command shared by the streamed paths.
    /// Callers attach their own stdio and spawn.
    #[allow(dead_code)]
    fn exec_command_inner(&self, name: &str, argv: &[&str], opts: &ExecOpts) -> Command {
        let mut cmd = podman_command();
        cmd.arg("--connection").arg(&self.connection).arg("exec").arg("-i");
        if opts.detached {
            cmd.arg("-d");
        }
        if let Some(workdir) = &opts.workdir {
            cmd.arg("--workdir").arg(workdir);
        }
        for (k, v) in &opts.env {
            cmd.arg("--env").arg(format!("{k}={v}"));
        }
        cmd.arg(name);
        for a in argv {
            cmd.arg(a);
        }
        cmd
    }
}

#[async_trait::async_trait]
impl MachineProvider for PodmanProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities { remote_targets: true, event_stream: true, host_bind_mount: true }
    }

    async fn ensure_base_image(&self) -> Result<(), ProviderError> {
        // Held across the whole check+build so concurrent local creates serialize: the first
        // builder wins while the rest wait, then each re-checks `image exists` inside
        // `ensure_image` and returns immediately — without it they would race the build.
        let _guard = self.build_lock.lock().await;
        self.ensure_image().await
    }

    async fn base_image_ready(&self) -> Result<bool, ProviderError> {
        self.image_exists().await
    }

    async fn create_instance(&self, spec: &InstanceSpec) -> Result<String, ProviderError> {
        let volume = format!("{}:{WORKSPACE_MOUNT}", spec.workspace_source);
        // `--pull=missing` satisfies the "pull if absent, then create" contract.
        let mut args: Vec<String> = vec![
            "create".to_string(),
            "--pull=missing".to_string(),
            "--tty".to_string(),
        ];
        match spec.publish_model {
            PublishModel::ExecTunnelled => {
                // No `--network pasta:--host-lo-to-ns-lo`, no `--publish` at all: reachability goes
                // through `podman exec`-tunneled `sshd`, not a published loopback port — see
                // docs/features/podman-exec-transport/02_dev.md.
                args.push("--network".to_string());
                args.push("pasta".to_string());
            }
            PublishModel::PublishRelay => {
                // Publish zellij's web port to an ephemeral host-loopback port. `--host-lo-to-ns-lo`
                // is the decisive pasta flag: it forwards the host-loopback-published port to the
                // container's loopback, where zellij binds; without it pasta forwards to the
                // container's eth0 and the loopback-bound server is unreachable. The host side is
                // pinned to 127.0.0.1 so nothing is exposed on the local network.
                //
                // Podman only forwards ports it is explicitly told about (confirmed: with no
                // `--publish` it invokes pasta with `-t none -u none`, forwarding nothing) — there is
                // no way to reach a container's port from outside its netns without publishing it
                // first. The relay pool pre-publishes a small pool of container-side ports
                // (crate::network::RELAY_POOL_SIZE) that `NetworkManager::expose_port` can later
                // bridge to an arbitrary user-chosen port via an in-container `socat` relay, without
                // ever recreating the container.
                args.push("--network".to_string());
                args.push("pasta:--host-lo-to-ns-lo".to_string());
                args.push("--publish".to_string());
                args.push(format!("127.0.0.1::{ZELLIJ_WEB_PORT}"));
                args.push("--publish".to_string());
                args.push(format!("127.0.0.1::{PASEO_WEB_PORT}"));
                for p in crate::network::relay_pool_publish_args() {
                    args.push("--publish".to_string());
                    args.push(p);
                }
            }
        }
        args.push("--volume".to_string());
        args.push(volume);
        args.push("--workdir".to_string());
        args.push(WORKSPACE_MOUNT.to_string());
        for (k, v) in &spec.labels {
            args.push("--label".to_string());
            args.push(format!("{k}={v}"));
        }
        args.push("--name".to_string());
        args.push(spec.name.clone());
        args.push(spec.image.clone());
        args.push("/bin/sh".to_string());
        let out = self.run(&args.iter().map(String::as_str).collect::<Vec<_>>()).await?;
        Ok(check(out)?.trim().to_string())
    }

    async fn list_instances(&self) -> Result<Vec<crate::backends::types::Sandbox>, ProviderError> {
        let out = self.run(&["ps", "--all", "--format", "json"]).await?;
        let stdout = check(out)?;
        let containers: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| ProviderError::ParseError(e.to_string()))?;
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
                    .unwrap_or_else(|| crate::backends::workspace_path(&name).unwrap_or_default());
                let image_version = parse_image_version(c["Image"].as_str().unwrap_or_default());
                crate::backends::types::Sandbox {
                    id: c["Id"].as_str().unwrap_or_default().to_string(),
                    status: c["State"].as_str().unwrap_or_default().to_string(),
                    name,
                    workspace_path,
                    image_version,
                }
            })
            .collect())
    }

    async fn start(&self, name: &str) -> Result<(), ProviderError> {
        check(self.run(&["start", name]).await?).map(|_| ())
    }

    async fn stop(&self, name: &str) -> Result<(), ProviderError> {
        check(self.run(&["stop", name]).await?).map(|_| ())
    }

    async fn remove(&self, name: &str) -> Result<(), ProviderError> {
        // `--force` stops a running container first. The workspace directory is left intact.
        check(self.run(&["rm", "--force", name]).await?).map(|_| ())
    }

    async fn exec(
        &self,
        name: &str,
        argv: &[&str],
        opts: ExecOpts,
    ) -> Result<Output, ProviderError> {
        let mut full: Vec<String> = vec!["exec".to_string()];
        if opts.detached {
            full.push("-d".to_string());
        }
        if let Some(workdir) = &opts.workdir {
            full.push("--workdir".to_string());
            full.push(workdir.clone());
        }
        for (k, v) in &opts.env {
            full.push("--env".to_string());
            full.push(format!("{k}={v}"));
        }
        full.push(name.to_string());
        full.extend(argv.iter().map(|s| s.to_string()));
        self.run(&full.iter().map(String::as_str).collect::<Vec<_>>()).await
    }

    async fn exec_piped(
        &self,
        name: &str,
        argv: &[&str],
        stdin: &[u8],
    ) -> Result<Output, ProviderError> {
        let mut child = self
            .exec_command_inner(name, argv, &ExecOpts::default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ProviderError::CommandFailed(e.to_string()))?;
        let mut sink = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::CommandFailed("no stdin".to_string()))?;
        sink.write_all(stdin)
            .await
            .map_err(|e| ProviderError::CommandFailed(e.to_string()))?;
        drop(sink);
        child
            .wait_with_output()
            .await
            .map_err(|e| ProviderError::CommandFailed(e.to_string()))
    }

    fn exec_command(&self, name: &str, argv: &[&str], opts: ExecOpts) -> Command {
        self.exec_command_inner(name, argv, &opts)
    }

    async fn copy_in(&self, name: &str, host_path: &str, dest: &str) -> Result<(), ProviderError> {
        check(self.run(&["cp", host_path, &format!("{name}:{dest}")]).await?).map(|_| ())
    }
}
