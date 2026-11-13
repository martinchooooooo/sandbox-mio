pub mod msb;
pub mod msb_policy;
pub mod probe;
pub mod types;

use std::process::{Output, Stdio};
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::backends::{base_image, podman_command};
use crate::process::types::ProcessRunner;
use types::{
    ExecOpts, InstanceSpec, MachineProvider, MachineRef, ProviderCapabilities, ProviderError,
    ProviderId, SidecarSpec,
};

/// Spelled literally because `podman -v` rejects a bare `~` and does not expand `$HOME`.
const WORKSPACE_MOUNT: &str = "/root/work";

const CONTAINERFILE: &str = include_str!("../../resources/Containerfile");

/// The label key `create_instance` stamps and `list_instances` reads back to recover an
/// `existing_path` sandbox's workspace, which is not reconstructible from the name alone.
pub(crate) const WORKSPACE_LABEL: &str = "sandbox-mio.workspace";

/// The id half of the one microsandbox machine there is (`msb:local`). Microsandbox runs its
/// instances on the host itself and has no connection registry, so unlike Podman there is nothing
/// to name — but [`MachineRef`] still needs an id to round-trip through its `provider:id` string.
const MSB_LOCAL_ID: &str = "local";

/// Resolves a [`MachineRef`] to the provider driving it. Replaces the single managed
/// `Arc<dyn MachineProvider>` Phases A–I wired into Tauri state, which could only ever be Podman's
/// (`docs/features/microsandbox-provider/02_dev.md`, Phase J).
pub struct ProviderRegistry {
    podman_runner: Arc<dyn ProcessRunner>,
    /// The Podman system connection a locally-created sandbox lives on. Read once at startup, like
    /// `CliBackend::connection` and `MiseRuntime::connection` already do.
    local_connection: String,
    /// Cached rather than rebuilt per call so every local Podman operation shares one base-image
    /// build lock — a fresh `PodmanProvider` per call would each hold their own and race the build.
    local_podman: Arc<dyn MachineProvider>,
    msb: Arc<dyn MachineProvider>,
}

impl ProviderRegistry {
    /// The machine a locally-created sandbox of `provider` lives on.
    pub fn local_ref(&self, provider: ProviderId) -> MachineRef {
        let id = match provider {
            ProviderId::Podman => self.local_connection.clone(),
            ProviderId::Microsandbox => MSB_LOCAL_ID.to_string(),
        };
        MachineRef { provider, id }
    }

    /// The provider driving `machine`. A Podman ref naming a connection other than the local one
    /// (a deploy target) gets its own pinned provider, exactly as `deploy` builds one.
    pub fn provider_for(&self, machine: &MachineRef) -> Arc<dyn MachineProvider> {
        match machine.provider {
            ProviderId::Podman if machine.id == self.local_connection => self.local_podman.clone(),
            ProviderId::Podman => default_provider(self.podman_runner.clone(), machine.id.clone()),
            ProviderId::Microsandbox => self.msb.clone(),
        }
    }

    /// The provider for `provider`'s local machine — shorthand for `provider_for(&local_ref(..))`.
    pub fn local(&self, provider: ProviderId) -> Arc<dyn MachineProvider> {
        self.provider_for(&self.local_ref(provider))
    }

    /// The provider running an instance named `name`, found by asking each in turn. Falls back to
    /// Podman when no provider lists it, so an operation on a vanished sandbox surfaces the engine's
    /// own "no such container" rather than an error of this app's invention. Podman is asked first,
    /// which short-circuits the microsandbox lookup's per-instance `msb inspect` for the common case.
    pub async fn provider_of(&self, name: &str) -> (ProviderId, Arc<dyn MachineProvider>) {
        for (id, provider) in self.all() {
            match provider.list_instances().await {
                Ok(instances) if instances.iter().any(|s| s.name == name) => {
                    return (id, provider);
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!(target: "provider", provider = %id, error = %e, "skipping provider in lookup")
                }
            }
        }
        (ProviderId::Podman, self.local_podman.clone())
    }

    /// [`Self::provider_of`] for the callers that need only the provider, not its id.
    pub async fn engine_of(&self, name: &str) -> Arc<dyn MachineProvider> {
        self.provider_of(name).await.1
    }

    /// Every provider, for the cross-provider operations (`list`, name-uniqueness) that have to ask
    /// all of them rather than one.
    pub fn all(&self) -> [(ProviderId, Arc<dyn MachineProvider>); 2] {
        [
            (ProviderId::Podman, self.local_podman.clone()),
            (ProviderId::Microsandbox, self.msb.clone()),
        ]
    }
}

/// Construct the [`ProviderRegistry`] for Tauri state. Both runners come from `src-tauri/src/bootstrap.rs`, so
/// scripted and real calls stay on the one director the rest of the app shares.
pub fn default_registry(
    podman_runner: Arc<dyn ProcessRunner>,
    msb_runner: Arc<dyn ProcessRunner>,
    start_env: Arc<dyn types::StartEnvSource>,
) -> Arc<ProviderRegistry> {
    let local_connection = crate::backends::connection_name();
    let local_podman = default_provider(podman_runner.clone(), local_connection.clone());
    let msb = msb::default_msb_provider(msb_runner, local_podman.clone(), start_env);
    Arc::new(ProviderRegistry { podman_runner, local_connection, local_podman, msb })
}

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
    /// Runs `podman --connection <connection> <argv>` through the mockable runner seam. Call/result
    /// logging lives one layer down, in `SystemPodmanRunner::run` (`mockctl/mod.rs`) — the one seam
    /// every real Podman invocation passes through, `verify_host`'s included — rather than here,
    /// so it can't end up covering some callers and not others.
    async fn run(&self, argv: &[&str]) -> Result<Output, ProviderError> {
        self.run_with_env(argv, &[]).await
    }

    /// [`Self::run`] with `env` set in the child process only, for values that must stay out of
    /// argv and therefore out of `mio.log`.
    async fn run_with_env(
        &self,
        argv: &[&str],
        env: &[(String, String)],
    ) -> Result<Output, ProviderError> {
        let mut full = vec!["--connection", &self.connection];
        full.extend_from_slice(argv);
        self.runner.run_with_env(&full, env).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::TimedOut => ProviderError::Unresponsive(e.to_string()),
            _ => ProviderError::CommandFailed(e.to_string()),
        })
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
        debug_assert!(opts.secret_env.is_empty(), "streamed exec does not carry secret_env");
        cmd.arg(name);
        for a in argv {
            cmd.arg(a);
        }
        cmd
    }
}

/// The `podman exec` arguments (after `--connection`) for one one-shot exec. Each secret is a
/// name-only `--env NAME`, which podman resolves from its own client environment, so no secret
/// value ever appears here.
fn exec_args(name: &str, argv: &[&str], opts: &ExecOpts) -> Vec<String> {
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
    for (k, _) in &opts.secret_env {
        full.push("--env".to_string());
        full.push(k.clone());
    }
    full.push(name.to_string());
    full.extend(argv.iter().map(|s| s.to_string()));
    full
}

/// Interleaves `podman logs --timestamps` output into one oldest-first list. Podman replays the
/// container's stdout and stderr on its own two streams, so each arrives ordered but separately.
/// The timestamp prefix is fixed-width, so a string sort is chronological; the sort is stable, so
/// equal stamps keep their stream order.
fn merge_log_streams(stdout: &[u8], stderr: &[u8]) -> Vec<String> {
    let mut lines: Vec<String> = [stdout, stderr]
        .iter()
        .flat_map(|stream| {
            String::from_utf8_lossy(stream).lines().map(str::to_string).collect::<Vec<_>>()
        })
        .filter(|l| !l.is_empty())
        .collect();
    lines.sort_by(|a, b| log_stamp(a).cmp(log_stamp(b)));
    lines
}

fn log_stamp(line: &str) -> &str {
    line.split_once(' ').map_or(line, |(stamp, _)| stamp)
}

#[async_trait::async_trait]
impl MachineProvider for PodmanProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            remote_targets: true,
            event_stream: true,
            host_bind_mount: true,
            egress_policy: false,
            sidecar_egress: true,
        }
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
        if spec.egress.is_some() {
            // Podman enforcement belongs to `docs/features/proxy-network-policy/02_dev.md`.
            return Err(ProviderError::CommandFailed(
                "Podman sandboxes cannot enforce a network policy yet".to_string(),
            ));
        }
        let volume = format!("{}:{WORKSPACE_MOUNT}", spec.workspace_source);
        // `--pull=missing` satisfies the "pull if absent, then create" contract.
        let mut args: Vec<String> = vec![
            "create".to_string(),
            "--pull=missing".to_string(),
            "--tty".to_string(),
        ];
        match &spec.network {
            Some(net) => {
                // Replaces the default `pasta` network entirely: Podman rejects combining `pasta`
                // with a second network (`docs/features/network-proxy/02_dev.md`, Spike Results),
                // so a proxy-enabled sandbox drops `pasta` rather than adding to it.
                args.push("--network".to_string());
                args.push(match &net.ip {
                    Some(ip) => format!("{}:ip={ip}", net.name),
                    None => net.name.clone(),
                });
                if let Some(dns) = &net.dns {
                    args.push("--dns".to_string());
                    args.push(dns.clone());
                }
            }
            None => {
                // No `--publish`: reachability goes through the exec-tunnelled `sshd`, not a
                // published loopback port (`docs/architecture/transport.md`).
                args.push("--network".to_string());
                args.push("pasta".to_string());
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
        for (k, v) in &spec.env {
            args.push("--env".to_string());
            args.push(format!("{k}={v}"));
        }
        args.push("--name".to_string());
        args.push(spec.name.clone());
        args.push(spec.image.clone());
        args.push("/bin/sh".to_string());
        let out = self.run(&args.iter().map(String::as_str).collect::<Vec<_>>()).await?;
        Ok(check(out)?.trim().to_string())
    }

    async fn create_sidecar(&self, spec: &SidecarSpec) -> Result<String, ProviderError> {
        // No `--pull=missing`: the caller pulls explicitly first (`pull_image`,
        // `docs/features/network-proxy/02_dev.md`, Resolved Decision 10) so a first-ever ~200 MB
        // pull is reported through `activity::TaskRegistry` instead of looking like a hung create.
        let network = spec.network.as_ref().ok_or_else(|| {
            ProviderError::CommandFailed("a Podman sidecar needs a network attachment".to_string())
        })?;
        let mut args: Vec<String> = vec!["create".to_string()];
        args.push("--network".to_string());
        args.push(match &network.ip {
            Some(ip) => format!("{}:ip={ip}", network.name),
            None => network.name.clone(),
        });
        for extra in &spec.extra_networks {
            args.push("--network".to_string());
            args.push(extra.clone());
        }
        if let Some(size) = &spec.log_max_size {
            // `k8s-file`, not the machine default `journald`, which needs a user systemd bus the
            // podman machine lacks (`docs/features/podman-egress-boundary/02_dev.md`, Resolved
            // Decision 4).
            args.push("--log-driver".to_string());
            args.push("k8s-file".to_string());
            args.push("--log-opt".to_string());
            args.push(format!("max-size={size}"));
        }
        for p in &spec.publish {
            args.push("--publish".to_string());
            args.push(format!("{}:{}:{}", p.bind, p.host_port, p.guest_port));
        }
        for (k, v) in &spec.env {
            args.push("--env".to_string());
            args.push(format!("{k}={v}"));
        }
        for m in &spec.mounts {
            args.push("--volume".to_string());
            let suffix = if m.read_only { ":ro" } else { "" };
            args.push(format!("{}:{}{suffix}", m.host_path, m.dest));
        }
        for (k, v) in &spec.labels {
            args.push("--label".to_string());
            args.push(format!("{k}={v}"));
        }
        args.push("--name".to_string());
        args.push(spec.name.clone());
        args.push(spec.image.clone());
        args.extend(spec.command.iter().cloned());
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
                    provider: ProviderId::Podman,
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

    async fn stop_sidecar(&self, name: &str) -> Result<(), ProviderError> {
        self.stop(name).await
    }

    /// The bind-mounted workspace survives the container.
    async fn recreate_instance(&self, spec: &InstanceSpec) -> Result<String, ProviderError> {
        check(self.run(&["rm", "--force", &spec.name]).await?)?;
        self.create_instance(spec).await
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
        let full = exec_args(name, argv, &opts);
        self.run_with_env(&full.iter().map(String::as_str).collect::<Vec<_>>(), &opts.secret_env)
            .await
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

    async fn create_network(
        &self,
        name: &str,
        subnet: &str,
        internal: bool,
    ) -> Result<(), ProviderError> {
        // `--disable-dns`: `aardvark-dns` cannot start on this project's podman machine
        // (`docs/features/network-proxy/02_dev.md`, Spike Results) — every network this app
        // creates must pass it, internal or not, or every container on it fails to start.
        let mut args = vec!["network", "create", "--disable-dns"];
        if internal {
            args.push("--internal");
        }
        args.extend(["--subnet", subnet, name]);
        check(self.run(&args).await?).map(|_| ())
    }

    async fn remove_network(&self, name: &str) -> Result<(), ProviderError> {
        check(self.run(&["network", "rm", "--force", name]).await?).map(|_| ())
    }

    async fn sidecar_logs(
        &self,
        name: &str,
        since: Option<&str>,
        tail: Option<usize>,
    ) -> Result<Vec<String>, ProviderError> {
        let tail = tail.map(|n| n.to_string());
        let mut args = vec!["logs", "--timestamps"];
        if let Some(since) = since {
            args.extend(["--since", since]);
        }
        if let Some(tail) = &tail {
            args.extend(["--tail", tail]);
        }
        args.push(name);
        let out = self.run(&args).await?;
        if !out.status.success() {
            return Err(ProviderError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        Ok(merge_log_streams(&out.stdout, &out.stderr))
    }

    async fn pull_image(&self, image: &str) -> Result<(), ProviderError> {
        let exists = self.run(&["image", "exists", image]).await?.status.success();
        if exists {
            return Ok(());
        }
        check(self.run(&["pull", image]).await?).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mockctl::types::{MockDirector, ScriptedPodmanRunner};

    fn secret_opts() -> ExecOpts {
        ExecOpts {
            workdir: Some("/root/work".to_string()),
            env: vec![("PLAIN".to_string(), "visible".to_string())],
            secret_env: vec![("PASEO_PASSWORD".to_string(), "s3cret-value".to_string())],
            detached: true,
        }
    }

    #[test]
    fn exec_args_render_a_secret_by_name_only() {
        let args = exec_args("box", &["paseo", "daemon"], &secret_opts());
        assert!(args.windows(2).any(|w| w == ["--env", "PASEO_PASSWORD"]), "name-only flag present");
        assert!(args.iter().any(|a| a == "PLAIN=visible"), "plain env keeps its value");
        assert!(args.iter().all(|a| !a.contains("s3cret-value")), "value absent from argv");
    }

    #[tokio::test]
    async fn exec_hands_the_value_to_the_runner_environment_only() {
        let director = Arc::new(MockDirector::new());
        let runner: Arc<dyn ProcessRunner> = Arc::new(ScriptedPodmanRunner::new(director.clone()));
        let provider = default_provider(runner, "conn".to_string());
        provider.exec("box", &["true"], secret_opts()).await.expect("exec succeeds");

        let calls = director.calls("podman.exec");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].env_names, ["PASEO_PASSWORD"]);
        assert!(calls[0].argv.iter().all(|a| !a.contains("s3cret-value")));
    }

    fn scripted() -> (Arc<MockDirector>, Arc<dyn MachineProvider>) {
        let director = Arc::new(MockDirector::new());
        let runner: Arc<dyn ProcessRunner> = Arc::new(ScriptedPodmanRunner::new(director.clone()));
        (director, default_provider(runner, "conn".to_string()))
    }

    fn has_pair(argv: &[String], flag: &str, value: &str) -> bool {
        argv.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[tokio::test]
    async fn create_network_adds_internal_only_when_asked_and_always_disables_dns() {
        let (director, provider) = scripted();
        provider.create_network("open", "10.90.0.0/24", false).await.expect("create succeeds");
        provider.create_network("closed", "10.90.1.0/24", true).await.expect("create succeeds");
        let calls = director.calls("podman.network-create");
        assert_eq!(
            calls[0].argv,
            ["network", "create", "--disable-dns", "--subnet", "10.90.0.0/24", "open"]
        );
        assert_eq!(
            calls[1].argv,
            ["network", "create", "--disable-dns", "--internal", "--subnet", "10.90.1.0/24", "closed"]
        );
    }

    fn sidecar_spec() -> SidecarSpec {
        use types::{InstanceNetwork, SidecarMount};
        SidecarSpec {
            name: "s-proxy".to_string(),
            image: "docker.io/ironsh/iron-proxy:0.56.0".to_string(),
            network: Some(InstanceNetwork {
                name: "mio-proxy-s".to_string(),
                ip: Some("10.90.3.2".to_string()),
                dns: None,
            }),
            extra_networks: Vec::new(),
            log_max_size: None,
            mounts: vec![SidecarMount {
                host_path: "C:/p/certs".to_string(),
                dest: "/certs".to_string(),
                read_only: true,
            }],
            publish: Vec::new(),
            env: Vec::new(),
            command: vec!["-config".to_string(), "/config/config.yaml".to_string()],
            labels: Vec::new(),
        }
    }

    #[tokio::test]
    async fn create_sidecar_renders_the_pinned_network_alone_by_default() {
        let (director, provider) = scripted();
        provider.create_sidecar(&sidecar_spec()).await.expect("create succeeds");
        let argv = director.calls("podman.create")[0].argv.clone();
        assert_eq!(argv.iter().filter(|a| *a == "--network").count(), 1);
        assert!(has_pair(&argv, "--network", "mio-proxy-s:ip=10.90.3.2"));
        assert!(has_pair(&argv, "--volume", "C:/p/certs:/certs:ro"));
        assert!(!argv.iter().any(|a| a == "--log-driver" || a == "--log-opt"));
        assert_eq!(
            argv[argv.len() - 3..],
            ["docker.io/ironsh/iron-proxy:0.56.0", "-config", "/config/config.yaml"]
        );
    }

    #[tokio::test]
    async fn create_sidecar_joins_extra_networks_after_the_pinned_one() {
        let (director, provider) = scripted();
        let spec = SidecarSpec {
            extra_networks: vec!["podman".to_string(), "other".to_string()],
            ..sidecar_spec()
        };
        provider.create_sidecar(&spec).await.expect("create succeeds");
        let argv = director.calls("podman.create")[0].argv.clone();
        let networks: Vec<&str> = argv
            .windows(2)
            .filter(|w| w[0] == "--network")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(networks, ["mio-proxy-s:ip=10.90.3.2", "podman", "other"]);
    }

    #[tokio::test]
    async fn create_sidecar_selects_a_capped_file_log_when_a_size_is_set() {
        let (director, provider) = scripted();
        let spec = SidecarSpec { log_max_size: Some("4mb".to_string()), ..sidecar_spec() };
        provider.create_sidecar(&spec).await.expect("create succeeds");
        let argv = director.calls("podman.create")[0].argv.clone();
        assert!(has_pair(&argv, "--log-driver", "k8s-file"));
        assert!(has_pair(&argv, "--log-opt", "max-size=4mb"));
        let image =
            argv.iter().position(|a| a == "docker.io/ironsh/iron-proxy:0.56.0").expect("image");
        let driver = argv.iter().position(|a| a == "--log-driver").expect("driver");
        assert!(driver < image, "log flags precede the image");
    }

    fn instance_spec(dns: Option<&str>) -> InstanceSpec {
        InstanceSpec {
            name: "s".to_string(),
            image: "localhost/sandbox-mio/base:7".to_string(),
            workspace_source: "C:/work".to_string(),
            labels: Vec::new(),
            env: Vec::new(),
            network: Some(types::InstanceNetwork {
                name: "mio-proxy-s".to_string(),
                ip: None,
                dns: dns.map(str::to_string),
            }),
            egress: None,
        }
    }

    #[tokio::test]
    async fn create_instance_points_the_resolver_at_the_given_address() {
        let (director, provider) = scripted();
        provider.create_instance(&instance_spec(Some("10.90.3.2"))).await.expect("create succeeds");
        let argv = director.calls("podman.create")[0].argv.clone();
        assert!(has_pair(&argv, "--network", "mio-proxy-s"));
        assert!(has_pair(&argv, "--dns", "10.90.3.2"));
    }

    #[tokio::test]
    async fn create_instance_keeps_the_network_resolver_without_a_dns_address() {
        let (director, provider) = scripted();
        provider.create_instance(&instance_spec(None)).await.expect("create succeeds");
        let argv = director.calls("podman.create")[0].argv.clone();
        assert!(!argv.iter().any(|a| a == "--dns"));
    }

    #[test]
    fn podman_reports_sidecar_egress() {
        let (_, provider) = scripted();
        assert!(provider.capabilities().sidecar_egress);
        assert!(!provider.capabilities().egress_policy);
    }

    #[tokio::test]
    async fn sidecar_logs_renders_since_and_tail() {
        let (director, provider) = scripted();
        provider
            .sidecar_logs("s-proxy", Some("2026-11-13T21:39:03.624385714+11:00"), Some(500))
            .await
            .expect("logs succeed");
        provider.sidecar_logs("s-proxy", None, None).await.expect("logs succeed");
        let calls = director.calls("podman.logs");
        assert_eq!(
            calls[0].argv,
            [
                "logs",
                "--timestamps",
                "--since",
                "2026-11-13T21:39:03.624385714+11:00",
                "--tail",
                "500",
                "s-proxy"
            ]
        );
        assert_eq!(calls[1].argv, ["logs", "--timestamps", "s-proxy"]);
    }

    #[tokio::test]
    async fn sidecar_logs_reads_no_lines_from_an_unscripted_mock() {
        let (_, provider) = scripted();
        let lines = provider.sidecar_logs("s-proxy", None, None).await.expect("logs succeed");
        assert!(lines.is_empty());
    }

    #[tokio::test]
    async fn sidecar_logs_returns_scripted_lines_and_surfaces_a_failure() {
        use crate::mockctl::types::Behaviour;
        let (director, provider) = scripted();
        director
            .configure(
                "podman.logs",
                Behaviour::ReturnValue {
                    stdout: "t1 {\"a\":1}\nt2 {\"b\":2}\n".to_string(),
                    exit_code: 0,
                },
            )
            .expect("known mock key");
        let lines = provider.sidecar_logs("s-proxy", None, None).await.expect("logs succeed");
        assert_eq!(lines, ["t1 {\"a\":1}", "t2 {\"b\":2}"]);

        director
            .configure(
                "podman.logs",
                Behaviour::FailOnce { exit_code: 125, stderr: "no such container".to_string() },
            )
            .expect("known mock key");
        let err = provider.sidecar_logs("s-proxy", None, None).await.expect_err("logs fail");
        assert!(
            matches!(err, ProviderError::CommandFailed(ref m) if m == "no such container"),
            "{err}"
        );
    }

    #[test]
    fn merge_log_streams_interleaves_by_timestamp() {
        let stdout = b"2026-11-13T21:39:04.000000000+11:00 out-b\n";
        let stderr = b"2026-11-13T21:39:03.000000000+11:00 err-a\n\
                       2026-11-13T21:39:05.000000000+11:00 err-c\n";
        let lines = merge_log_streams(stdout, stderr);
        let bodies: Vec<&str> = lines.iter().map(|l| l.split_once(' ').unwrap().1).collect();
        assert_eq!(bodies, ["err-a", "out-b", "err-c"]);
    }
}
