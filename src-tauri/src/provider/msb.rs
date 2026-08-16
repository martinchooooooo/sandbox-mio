use std::process::{Output, Stdio};
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::backends::types::Sandbox;
use crate::process::types::ProcessRunner;

use super::types::{
    ExecOpts, InstanceSpec, MachineProvider, ProviderCapabilities, ProviderError, ProviderId,
    PublishModel,
};
use super::{check, parse_image_version, WORKSPACE_LABEL, WORKSPACE_MOUNT};

/// Capacity for a microsandbox instance's workspace volume. The volume is a sparse ext4 image
/// (spike-confirmed `used_bytes: 0` immediately after creation), so a generous capacity costs
/// nothing until the workspace actually grows into it.
const WORKSPACE_VOLUME_SIZE: &str = "20G";

fn workspace_volume_name(name: &str) -> String {
    format!("{name}-ws")
}

/// Normalizes microsandbox's seven PascalCase states onto `translateStatus()`'s five existing
/// lowercase values (`docs/features/microsandbox-provider/02_dev.md`, Resolved Decision 5).
fn map_status(raw: &str) -> String {
    match raw {
        "Running" | "Draining" => "running",
        "Stopped" | "Crashed" => "exited",
        "Created" | "Starting" => "created",
        "Paused" => "paused",
        other => other,
    }
    .to_string()
}

/// POSIX single-quote escaping for embedding an argument inside a `bash -c`/`sh -c` script string.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The guest-shell backgrounding wrapper satisfying `ExecOpts.detached` (`msb exec` has no `-d`).
/// Must run under `bash -c`, not `sh -c`: this image's `/bin/sh` is `dash`, which has no `disown`
/// builtin and exits 127 before backgrounding is confirmed, even though the `&` itself still
/// detaches the process successfully — confirmed hands-on against real `msb` 0.6.6, see the
/// addendum in `docs/features/microsandbox-provider/spike/hands-on-cli.md`
/// (Resolved Decision 4).
fn detached_wrapper(argv: &[&str]) -> String {
    let joined = argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ");
    format!("{joined} >/tmp/out.log 2>&1 & disown")
}

/// Construct the microsandbox-backed [`MachineProvider`]. Holds `podman` (the app's already-pinned
/// local `PodmanProvider`) only to build the base image — microsandbox has no build command of its
/// own (`docs/features/microsandbox-provider/01_ideas.md`, "Base image via an imported archive").
pub fn default_msb_provider(
    msb_runner: Arc<dyn ProcessRunner>,
    podman: Arc<dyn MachineProvider>,
) -> Arc<dyn MachineProvider> {
    Arc::new(MsbProvider { runner: msb_runner, podman, build_lock: tokio::sync::Mutex::new(()) })
}

/// Real [`MachineProvider`] driving the `msb` CLI. Unlike [`super::PodmanProvider`], microsandbox
/// has no remote-target/connection concept (`capabilities().remote_targets` is `false`), so every
/// call is unpinned.
pub(crate) struct MsbProvider {
    runner: Arc<dyn ProcessRunner>,
    /// Drives `ensure_base_image`'s Podman-build half only; never touched by any other method.
    podman: Arc<dyn MachineProvider>,
    /// Serializes base image build to prevent concurrent races (mirrors `PodmanProvider`).
    build_lock: tokio::sync::Mutex<()>,
}

/// Trims a captured stream for the log: enough to carry a real `msb` diagnostic, capped so a JSON
/// listing or a base-image dump cannot flood `mio.log`.
fn for_log(bytes: &[u8]) -> String {
    const LIMIT: usize = 600;
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    match text.char_indices().nth(LIMIT) {
        Some((cut, _)) => format!("{}…[{} bytes total]", &text[..cut], bytes.len()),
        None => text.to_string(),
    }
}

impl MsbProvider {
    async fn run(&self, argv: &[&str]) -> Result<Output, ProviderError> {
        let started = std::time::Instant::now();
        let result = self.runner.run(argv).await;
        let ms = started.elapsed().as_millis();
        match &result {
            // Non-zero exit is logged but not returned as an error here: several callers
            // deliberately tolerate one (`ensure_volume`'s `already exists`, `start`'s `already
            // running`), so the log is the only place the distinction is visible.
            Ok(out) if out.status.success() => {
                tracing::info!(target: "msb", argv = %argv.join(" "), ms, "msb ok")
            }
            Ok(out) => tracing::warn!(
                target: "msb",
                argv = %argv.join(" "),
                ms,
                code = out.status.code().unwrap_or(-1),
                stderr = %for_log(&out.stderr),
                stdout = %for_log(&out.stdout),
                "msb exited non-zero"
            ),
            Err(e) => {
                tracing::error!(target: "msb", argv = %argv.join(" "), ms, error = %e, "msb spawn failed")
            }
        }
        result.map_err(|e| ProviderError::CommandFailed(e.to_string()))
    }

    /// Whether the base image is in the local `msb` store. Compares `msb image ls --format json`'s
    /// `reference` field exactly rather than substring-matching the plain-text listing, which would
    /// also accept `sandbox-mio/base:70` when asked for `sandbox-mio/base:7`. Both the `--format
    /// json` flag and `reference` carrying the full `name:tag` an image was loaded under are
    /// verified against real `msb` 0.6.6.
    async fn image_exists(&self) -> Result<bool, ProviderError> {
        let image = crate::backends::base_image();
        let stdout = check(self.run(&["image", "ls", "--format", "json"]).await?)?;
        // An empty store prints nothing rather than `[]` on some paths; that is no images, not a
        // parse failure.
        if stdout.trim().is_empty() {
            return Ok(false);
        }
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| ProviderError::ParseError(e.to_string()))?;
        Ok(entries.iter().any(|e| e["reference"].as_str() == Some(image.as_str())))
    }

    /// Creates the instance's workspace volume, tolerating one that already exists. `msb volume
    /// create` exits 1 with `volume already exists: <name>` on a taken name (confirmed against real
    /// `msb` 0.6.6), so a volume orphaned by an interrupted create would otherwise brick every
    /// later create of the same sandbox name until the user removed it by hand. Reusing it is safe:
    /// `remove` deletes an instance's volume, so a surviving one never belongs to a live instance,
    /// and the workspace's contents come from the Mutagen sync rather than the volume.
    async fn ensure_volume(&self, volume: &str) -> Result<(), ProviderError> {
        let out = self
            .run(&["volume", "create", volume, "--kind", "disk", "--size", WORKSPACE_VOLUME_SIZE])
            .await?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if stderr.contains("already exists") {
            return Ok(());
        }
        Err(ProviderError::CommandFailed(stderr))
    }

    /// Recovers the `sandbox-mio.workspace` label via `msb inspect`, since `msb ls` omits labels
    /// (spike finding 9) — one extra call per instance, the N+1 cost
    /// `docs/features/microsandbox-provider/01_ideas.md`'s Technology Choices already accepted.
    async fn workspace_label(&self, name: &str) -> Result<String, ProviderError> {
        let stdout = check(self.run(&["inspect", name, "--format", "json"]).await?)?;
        let v: serde_json::Value =
            serde_json::from_str(&stdout).map_err(|e| ProviderError::ParseError(e.to_string()))?;
        Ok(v["active_config"]["labels"][WORKSPACE_LABEL].as_str().unwrap_or_default().to_string())
    }

    /// Configured, unspawned `msb exec --stream` command shared by the streamed paths. `--stream`
    /// is byte-faithful piped stdin/stdout with no PTY (spike finding 7), the microsandbox
    /// counterpart of Podman's `exec -i`.
    fn exec_command_inner(&self, name: &str, argv: &[&str], opts: &ExecOpts) -> Command {
        let mut cmd = crate::process::msb_command();
        cmd.arg("exec").arg("--stream");
        if let Some(workdir) = &opts.workdir {
            cmd.arg("--workdir").arg(workdir);
        }
        for (k, v) in &opts.env {
            cmd.arg("--env").arg(format!("{k}={v}"));
        }
        cmd.arg(name).arg("--");
        for a in argv {
            cmd.arg(a);
        }
        cmd
    }
}

#[async_trait::async_trait]
impl MachineProvider for MsbProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities { remote_targets: false, event_stream: false, host_bind_mount: false }
    }

    async fn ensure_base_image(&self) -> Result<(), ProviderError> {
        let _guard = self.build_lock.lock().await;
        if self.image_exists().await? {
            return Ok(());
        }
        let image = crate::backends::base_image();
        if crate::backends::mock_backend_enabled() {
            // No real image to export under MIO_MOCK_BACKEND=1; go straight to the mockable `msb
            // load` call so light specs never spawn a real `podman save` process (src-tauri/
            // AGENTS.md's "spawning method... no mock_backend_enabled() gate" gotcha).
            return check(self.run(&["load", "--input", "mock", "--tag", &image]).await?).map(|_| ());
        }
        self.podman
            .ensure_base_image()
            .await
            .map_err(|e| ProviderError::PodmanUnavailable(e.to_string()))?;
        let tar_path =
            std::env::temp_dir().join(format!("sandbox-mio-msb-load-{}.tar", std::process::id()));
        let tar_arg = tar_path.to_string_lossy().replace('\\', "/");
        let connection = crate::backends::connection_name();
        let save = crate::backends::podman_command()
            .arg("--connection")
            .arg(&connection)
            .arg("save")
            .arg("--output")
            .arg(&tar_arg)
            .arg(&image)
            .output()
            .await
            .map_err(|e| ProviderError::CommandFailed(e.to_string()))?;
        if !save.status.success() {
            let _ = std::fs::remove_file(&tar_path);
            return Err(ProviderError::PodmanUnavailable(
                String::from_utf8_lossy(&save.stderr).trim().to_string(),
            ));
        }
        let load_result = self.run(&["load", "--input", &tar_arg, "--tag", &image]).await;
        let _ = std::fs::remove_file(&tar_path);
        check(load_result?).map(|_| ())
    }

    async fn base_image_ready(&self) -> Result<bool, ProviderError> {
        self.image_exists().await
    }

    async fn create_instance(&self, spec: &InstanceSpec) -> Result<String, ProviderError> {
        if !matches!(spec.publish_model, PublishModel::ExecTunnelled) {
            // A microsandbox instance is always exec-capable by construction; `MsbProvider` never
            // builds publish+relay argv (docs/features/microsandbox-provider/02_dev.md, gotcha table).
            return Err(ProviderError::CommandFailed(
                "microsandbox only supports exec-tunnelled publishing".to_string(),
            ));
        }
        let volume = workspace_volume_name(&spec.name);
        self.ensure_volume(&volume).await?;
        let mount = format!("{volume}:{WORKSPACE_MOUNT}:kind=disk,size={WORKSPACE_VOLUME_SIZE}");
        let mut args: Vec<String> = vec![
            "create".to_string(),
            spec.image.clone(),
            "--name".to_string(),
            spec.name.clone(),
            "--mount-named".to_string(),
            mount,
        ];
        for (k, v) in &spec.labels {
            args.push("--label".to_string());
            args.push(format!("{k}={v}"));
        }
        if let Err(e) = check(self.run(&args.iter().map(String::as_str).collect::<Vec<_>>()).await?)
        {
            // A failed `msb create` registers the sandbox before the step that fails, so roll back
            // both halves or the name is left taken with no workspace volume behind it — a record
            // `msb start` can only ever fail on. Order mirrors `remove`: record, then volume.
            // Best-effort — the create error is what the user needs to see either way.
            let _ = self.run(&["rm", "--force", &spec.name]).await;
            let _ = self.run(&["volume", "rm", &volume]).await;
            return Err(e);
        }
        // microsandbox has no separate instance id; the name is the id.
        Ok(spec.name.clone())
    }

    async fn list_instances(&self) -> Result<Vec<Sandbox>, ProviderError> {
        let stdout = check(self.run(&["ls", "--format", "json"]).await?)?;
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| ProviderError::ParseError(e.to_string()))?;
        let mut sandboxes = Vec::with_capacity(entries.len());
        for e in entries {
            let name = e["name"].as_str().unwrap_or_default().to_string();
            let status = map_status(e["status"].as_str().unwrap_or_default());
            let image_version = parse_image_version(e["image"].as_str().unwrap_or_default());
            let workspace_path = self.workspace_label(&name).await.unwrap_or_default();
            sandboxes.push(Sandbox {
                id: name.clone(),
                name,
                status,
                workspace_path,
                image_version,
                provider: ProviderId::Microsandbox,
            });
        }
        Ok(sandboxes)
    }

    /// Starts the instance, tolerating one already running. `msb create` auto-starts what it creates
    /// (`docs/features/microsandbox-provider/spike/hands-on-cli.md`, finding 3), so the start that
    /// has to precede the Mutagen session (`02_dev.md`, Phase H) always lands on a running instance
    /// and would otherwise fail with `error: sandbox still running: ... already running` (exit 1).
    /// Tolerance is modelled on [`Self::ensure_volume`]'s handling of a name already taken.
    async fn start(&self, name: &str) -> Result<(), ProviderError> {
        let out = self.run(&["start", name]).await?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if stderr.contains("already running") {
            return Ok(());
        }
        Err(ProviderError::CommandFailed(stderr))
    }

    async fn stop(&self, name: &str) -> Result<(), ProviderError> {
        check(self.run(&["stop", name]).await?).map(|_| ())
    }

    async fn remove(&self, name: &str) -> Result<(), ProviderError> {
        check(self.run(&["rm", "--force", name]).await?)?;
        let volume = workspace_volume_name(name);
        // Best-effort: the instance is already gone either way, and an orphaned volume is a cheap,
        // recoverable leftover rather than a reason to fail the whole remove.
        let _ = self.run(&["volume", "rm", &volume]).await;
        Ok(())
    }

    async fn exec(
        &self,
        name: &str,
        argv: &[&str],
        opts: ExecOpts,
    ) -> Result<Output, ProviderError> {
        let mut full: Vec<String> = vec!["exec".to_string()];
        if let Some(workdir) = &opts.workdir {
            full.push("--workdir".to_string());
            full.push(workdir.clone());
        }
        for (k, v) in &opts.env {
            full.push("--env".to_string());
            full.push(format!("{k}={v}"));
        }
        full.push(name.to_string());
        full.push("--".to_string());
        if opts.detached {
            full.push("bash".to_string());
            full.push("-c".to_string());
            full.push(detached_wrapper(argv));
        } else {
            full.extend(argv.iter().map(|s| s.to_string()));
        }
        self.run(&full.iter().map(String::as_str).collect::<Vec<_>>()).await
    }

    async fn exec_piped(
        &self,
        name: &str,
        argv: &[&str],
        stdin: &[u8],
    ) -> Result<Output, ProviderError> {
        if crate::backends::mock_backend_enabled() {
            // Spawns `msb_command()` directly, so `ScriptedMsbRunner` cannot divert it and a light
            // spec would drive the host's real `msb` (or an `MSB_PATH` mock that speaks no exec
            // protocol) — the gate `src-tauri/AGENTS.md` requires of any spawning method, as
            // `PodmanCliBackend::listen_events` already does. The scripted runner models one-shot
            // calls with no stdin, which is exactly what a `copy_in` under mock needs to be: the
            // call is recorded as `msb.exec` and the payload goes nowhere, since no guest exists to
            // receive it.
            let mut full = vec!["exec", name, "--"];
            full.extend_from_slice(argv);
            return self.run(&full).await;
        }
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
        let out = child
            .wait_with_output()
            .await
            .map_err(|e| ProviderError::CommandFailed(e.to_string()))?;
        // Logged separately from `run`: this path spawns `msb_command()` itself, so none of `run`'s
        // instrumentation sees it, and it is the one carrying the SSH key into the guest.
        tracing::info!(
            target: "msb",
            argv = %format!("exec --stream {name} -- {}", argv.join(" ")),
            stdin_bytes = stdin.len(),
            code = out.status.code().unwrap_or(-1),
            stderr = %for_log(&out.stderr),
            "msb exec (piped)"
        );
        Ok(out)
    }

    fn exec_command(&self, name: &str, argv: &[&str], opts: ExecOpts) -> Command {
        self.exec_command_inner(name, argv, &opts)
    }

    async fn copy_in(&self, name: &str, host_path: &str, dest: &str) -> Result<(), ProviderError> {
        let bytes = tokio::fs::read(host_path)
            .await
            .map_err(|e| ProviderError::CommandFailed(e.to_string()))?;
        // `msb` has no `podman cp` equivalent. `tee`/`cat >` will not create missing parent
        // directories the way `podman cp` does, so `mkdir -p` first keeps behaviour equivalent for
        // a dest like `/root/.paseo/projects/projects.json` whose directory doesn't exist yet on a
        // fresh instance.
        let q = shell_quote(dest);
        let script = format!("mkdir -p \"$(dirname {q})\" && cat > {q}");
        let out = self.exec_piped(name, &["sh", "-c", &script], &bytes).await?;
        check(out).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mockctl::types::{
        Behaviour, MockDirector, ScriptedMsbRunner, ScriptedPodmanRunner,
    };

    #[test]
    fn map_status_normalizes_pascal_case_states() {
        assert_eq!(map_status("Running"), "running");
        assert_eq!(map_status("Draining"), "running");
        assert_eq!(map_status("Stopped"), "exited");
        assert_eq!(map_status("Crashed"), "exited");
        assert_eq!(map_status("Created"), "created");
        assert_eq!(map_status("Starting"), "created");
        assert_eq!(map_status("Paused"), "paused");
    }

    #[test]
    fn shell_quote_escapes_embedded_quotes() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn detached_wrapper_backgrounds_and_disowns() {
        let s = detached_wrapper(&["python3", "-m", "http.server", "8082"]);
        assert_eq!(s, "'python3' '-m' 'http.server' '8082' >/tmp/out.log 2>&1 & disown");
    }

    fn provider() -> MsbProvider {
        provider_with(Arc::new(MockDirector::new()))
    }

    fn provider_with(director: Arc<MockDirector>) -> MsbProvider {
        let msb_runner = Arc::new(ScriptedMsbRunner::new(director.clone()));
        provider_over(msb_runner, director)
    }

    /// Takes the concrete runner so a test can keep a handle on its tracked state.
    fn provider_over(msb_runner: Arc<ScriptedMsbRunner>, director: Arc<MockDirector>) -> MsbProvider {
        let msb_runner: Arc<dyn ProcessRunner> = msb_runner;
        let podman_runner: Arc<dyn ProcessRunner> = Arc::new(ScriptedPodmanRunner::new(director));
        let podman = super::super::default_provider(podman_runner, "unused".to_string());
        MsbProvider { runner: msb_runner, podman, build_lock: tokio::sync::Mutex::new(()) }
    }

    fn spec(name: &str) -> InstanceSpec {
        InstanceSpec {
            name: name.to_string(),
            image: "python".to_string(),
            workspace_source: "C:/work".to_string(),
            labels: vec![(WORKSPACE_LABEL.to_string(), "C:/work".to_string())],
            publish_model: PublishModel::ExecTunnelled,
        }
    }

    #[tokio::test]
    async fn create_list_start_stop_remove_round_trip() {
        let provider = provider();
        let id = provider.create_instance(&spec("test-sbx")).await.expect("create succeeds");
        assert_eq!(id, "test-sbx");

        let list = provider.list_instances().await.expect("list succeeds");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-sbx");
        // `msb create` auto-starts, so a freshly created instance is already running.
        assert_eq!(list[0].status, "running");
        assert_eq!(list[0].workspace_path, "C:/work");

        provider.stop("test-sbx").await.expect("stop succeeds");
        let list = provider.list_instances().await.expect("list succeeds");
        assert_eq!(list[0].status, "exited");

        provider.start("test-sbx").await.expect("start succeeds");
        let list = provider.list_instances().await.expect("list succeeds");
        assert_eq!(list[0].status, "running");

        provider.stop("test-sbx").await.expect("stop succeeds");
        let list = provider.list_instances().await.expect("list succeeds");
        assert_eq!(list[0].status, "exited");

        provider.remove("test-sbx").await.expect("remove succeeds");
        let list = provider.list_instances().await.expect("list succeeds");
        assert!(list.is_empty());
    }

    /// A failed `msb create` must strand neither half: it registers the sandbox record before it
    /// fails, and the volume was made a step earlier. A stranded record takes the name and has no
    /// volume to start against; a stranded volume makes every later create of that name fail on
    /// `msb volume create`'s duplicate check instead of on what actually went wrong.
    #[tokio::test]
    async fn failed_create_rolls_back_the_record_and_the_workspace_volume() {
        let director = Arc::new(MockDirector::new());
        let msb_runner = Arc::new(ScriptedMsbRunner::new(director.clone()));
        let provider = provider_over(msb_runner.clone(), director.clone());
        director
            .configure(
                "msb.create",
                Behaviour::FailOnce { exit_code: 1, stderr: "no such image".to_string() },
            )
            .expect("known mock key");

        let spec = spec("test-sbx");
        assert!(provider.create_instance(&spec).await.is_err(), "scripted create fails");
        assert!(msb_runner.tracked_instance_names().is_empty(), "the record is rolled back");
        assert!(msb_runner.tracked_volumes().is_empty(), "the volume is rolled back");
        assert!(
            provider.list_instances().await.expect("list succeeds").is_empty(),
            "the failed create leaves nothing in the sandbox list"
        );
        provider.create_instance(&spec).await.expect("retry succeeds");
    }

    /// The rollback is best-effort, so a volume can still be orphaned (an interrupted process, a
    /// failed `volume rm`). Reusing it must not block the create.
    #[tokio::test]
    async fn create_instance_reuses_an_orphaned_volume() {
        let provider = provider();
        provider
            .run(&["volume", "create", "test-sbx-ws", "--kind", "disk", "--size", "20G"])
            .await
            .expect("runner call succeeds");

        provider.create_instance(&spec("test-sbx")).await.expect("create tolerates the leftover");
    }

    /// The check has to distinguish `sandbox-mio/base:7` from `sandbox-mio/base:70`, which a
    /// substring match on the plain-text listing could not.
    #[tokio::test]
    async fn base_image_ready_matches_the_reference_exactly() {
        let provider = provider();
        let image = crate::backends::base_image();
        assert!(!provider.base_image_ready().await.expect("empty store reads clean"));

        provider
            .run(&["load", "--input", "x.tar", "--tag", &format!("{image}0")])
            .await
            .expect("runner call succeeds");
        assert!(
            !provider.base_image_ready().await.expect("store reads clean"),
            "a longer version sharing the prefix is not this image"
        );

        provider
            .run(&["load", "--input", "x.tar", "--tag", &image])
            .await
            .expect("runner call succeeds");
        assert!(provider.base_image_ready().await.expect("store reads clean"));
    }

    /// `create_sandbox` (`src-tauri/src/lib.rs`) starts the instance before creating its Mutagen
    /// session, but `msb create` has already started it, so that start must succeed on a running
    /// instance rather than surface `msb start`'s exit 1.
    #[tokio::test]
    async fn start_tolerates_an_already_running_instance() {
        let provider = provider();
        provider.create_instance(&spec("test-sbx")).await.expect("create succeeds");

        provider.start("test-sbx").await.expect("start after create succeeds");
        provider.start("test-sbx").await.expect("a repeated start succeeds");
        let list = provider.list_instances().await.expect("list succeeds");
        assert_eq!(list[0].status, "running");
    }

    /// Only the already-running failure is tolerated; anything else `msb start` reports still fails.
    #[tokio::test]
    async fn start_surfaces_an_unrelated_failure() {
        let director = Arc::new(MockDirector::new());
        let provider = provider_with(director.clone());
        director
            .configure(
                "msb.start",
                Behaviour::FailAlways { exit_code: 1, stderr: "no such sandbox".to_string() },
            )
            .expect("known mock key");

        assert!(provider.start("test-sbx").await.is_err());
    }

    #[tokio::test]
    async fn create_instance_rejects_publish_relay() {
        let provider = provider();
        let spec = InstanceSpec { publish_model: PublishModel::PublishRelay, ..spec("test-sbx") };
        assert!(provider.create_instance(&spec).await.is_err());
    }
}
