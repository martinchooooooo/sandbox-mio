use std::process::{Output, Stdio};
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::backends::types::Sandbox;
use crate::process::types::ProcessRunner;

use super::types::{
    ExecOpts, InstanceSpec, MachineProvider, ProviderCapabilities, ProviderError, ProviderId,
    SidecarSpec, StartEnvSource,
};
use super::{check, parse_image_version, WORKSPACE_LABEL, WORKSPACE_MOUNT};

/// Capacity for a microsandbox instance's workspace volume. The volume is a sparse ext4 image
/// (spike-confirmed `used_bytes: 0` immediately after creation), so a generous capacity costs
/// nothing until the workspace actually grows into it.
const WORKSPACE_VOLUME_SIZE: &str = "20G";

/// microsandbox's own state directory: `MSB_HOME` when set (the e2e harness sets it per worktree),
/// otherwise `~/.microsandbox`.
pub(crate) fn msb_home() -> Option<std::path::PathBuf> {
    match std::env::var_os("MSB_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => Some(std::path::PathBuf::from(dir)),
        None => dirs::home_dir().map(|h| h.join(".microsandbox")),
    }
}

/// `<msb home>/sandboxes/<name>/logs/runtime.log`.
pub(crate) fn runtime_log_path(name: &str) -> Option<std::path::PathBuf> {
    msb_home().map(|h| h.join("sandboxes").join(name).join("logs").join("runtime.log"))
}

/// Whether the most recent start of `name` failed to bind a published port. `msb` exits 0 and
/// reports this only in `runtime.log` (`docs/features/microsandbox-network-policy/02_dev.md`,
/// Resolved Decision 8).
fn port_bind_failed(name: &str) -> bool {
    if crate::backends::mock_backend_enabled() {
        return false;
    }
    let Some(text) = runtime_log_path(name).and_then(|p| std::fs::read_to_string(p).ok()) else {
        return false;
    };
    let last_start = text.rfind("sandbox starting").unwrap_or(0);
    text[last_start..].contains("published TCP port listener failed")
}

/// How much of the end of `runtime.log` [`secret_violations`] scans.
const VIOLATION_SCAN_BYTES: u64 = 512 * 1024;

fn parse_violation(line: &str) -> Option<super::types::SecretViolation> {
    if !line.contains("secret violation") {
        return None;
    }
    let field = |key: &str| {
        line.split_whitespace()
            .find_map(|t| t.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
            .unwrap_or_default()
            .to_string()
    };
    Some(super::types::SecretViolation {
        ts: line.split_whitespace().next().unwrap_or_default().to_string(),
        action: field("action"),
        env_var: field("secret_env_var"),
        host: field("host"),
        method: field("method"),
        path: field("path"),
        location: field("location"),
    })
}

/// The newest `limit` blocked secret uses of sandbox `name`, newest first, read from the end of
/// its `runtime.log`. Only `block-and-log` and `block-and-terminate` write these lines. Empty on
/// the light tier, where no log exists.
pub fn secret_violations(name: &str, limit: usize) -> Vec<super::types::SecretViolation> {
    use std::io::{Read, Seek, SeekFrom};
    if crate::backends::mock_backend_enabled() {
        return Vec::new();
    }
    let Some(path) = runtime_log_path(name) else {
        return Vec::new();
    };
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(len.saturating_sub(VIOLATION_SCAN_BYTES)));
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    String::from_utf8_lossy(&bytes).lines().rev().filter_map(parse_violation).take(limit).collect()
}

/// An `msb` diagnostic naming an address-in-use condition.
fn is_port_in_use(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    s.contains("address already in use")
        || s.contains("os error 10048")
        || s.contains("only one usage of each socket address")
}

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
pub(super) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The guest-shell backgrounding wrapper satisfying `ExecOpts.detached` (`msb exec` has no `-d`).
/// Must run under `bash -c`, not `sh -c`: this image's `/bin/sh` is `dash`, which has no `disown`
/// builtin and exits 127 before backgrounding is confirmed, even though the `&` itself still
/// detaches the process successfully — confirmed against real `msb` 0.6.6 (addendum in
/// `docs/features/microsandbox-provider/spike/hands-on-cli.md`, Resolved Decision 4) and unchanged
/// in 0.7.12 (`docs/features/microsandbox-network-policy/spike/01_egress-baseline.md`).
fn detached_wrapper(argv: &[&str]) -> String {
    let joined = argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ");
    format!("{joined} >/tmp/out.log 2>&1 & disown")
}

/// Rewrites an exec so `opts.secret_env` reaches the guest through stdin instead of a flag: `msb
/// exec --env` only takes `KEY=value` (verified against `msb` 0.7.12), which would put the value in
/// argv and in `mio.log`. Returns the full guest command and the stdin payload carrying one value
/// per line, in the order the guest script reads them.
fn secret_exec_command(
    argv: &[&str],
    opts: &ExecOpts,
) -> Result<(Vec<String>, Vec<u8>), ProviderError> {
    let mut script = String::new();
    let mut stdin = Vec::new();
    for (name, value) in &opts.secret_env {
        let valid_name = !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid_name || value.contains(['\n', '\r', '\0']) {
            return Err(ProviderError::CommandFailed(format!(
                "secret variable {name:?} cannot be passed to the guest"
            )));
        }
        script.push_str(&format!("IFS= read -r v; export {name}=\"$v\"; "));
        stdin.extend_from_slice(value.as_bytes());
        stdin.push(b'\n');
    }
    script.push_str("exec \"$@\"");
    let mut command = vec!["sh".to_string(), "-c".to_string(), script, "sh".to_string()];
    if opts.detached {
        command.extend(["bash".to_string(), "-c".to_string(), detached_wrapper(argv)]);
    } else {
        command.extend(argv.iter().map(|a| a.to_string()));
    }
    Ok((command, stdin))
}

/// Construct the microsandbox-backed [`MachineProvider`]. Holds `podman` (the app's already-pinned
/// local `PodmanProvider`) only to build the base image — microsandbox has no build command of its
/// own (`docs/features/microsandbox-provider/01_ideas.md`, "Base image via an imported archive").
pub fn default_msb_provider(
    msb_runner: Arc<dyn ProcessRunner>,
    podman: Arc<dyn MachineProvider>,
    start_env: Arc<dyn StartEnvSource>,
) -> Arc<dyn MachineProvider> {
    Arc::new(MsbProvider {
        runner: msb_runner,
        podman,
        start_env,
        build_lock: tokio::sync::Mutex::new(()),
    })
}

/// Real [`MachineProvider`] driving the `msb` CLI. Unlike [`super::PodmanProvider`], microsandbox
/// has no remote-target/connection concept (`capabilities().remote_targets` is `false`), so every
/// call is unpinned.
pub(crate) struct MsbProvider {
    runner: Arc<dyn ProcessRunner>,
    /// Drives `ensure_base_image`'s Podman-build half only; never touched by any other method.
    podman: Arc<dyn MachineProvider>,
    /// Environment every start-capable call carries; microsandbox re-reads secret values at each
    /// start (`docs/features/microsandbox-network-policy/spike/02_secrets.md`, Item 4).
    start_env: Arc<dyn StartEnvSource>,
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
        self.run_env(argv, &[]).await
    }

    /// A call that may start `name` (`create`, `start`, `exec`), carrying its start environment.
    /// Only variable names are ever logged.
    async fn run_starting(&self, name: &str, argv: &[&str]) -> Result<Output, ProviderError> {
        let env = self.start_env.env_for(name)?;
        self.run_env(argv, &env).await
    }

    async fn run_env(
        &self,
        argv: &[&str],
        env: &[(String, String)],
    ) -> Result<Output, ProviderError> {
        let started = std::time::Instant::now();
        let result = self.runner.run_with_env(argv, env).await;
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
    /// verified against real `msb` 0.7.12 (`docs/features/microsandbox-network-policy/spike/
    /// 01_egress-baseline.md`, "0.6 → 0.7 Breaking Changes").
    async fn image_exists(&self) -> Result<bool, ProviderError> {
        self.image_present(&crate::backends::base_image()).await
    }

    /// Whether `image` is in the local `msb` store, by exact `reference`.
    async fn image_present(&self, image: &str) -> Result<bool, ProviderError> {
        let stdout = check(self.run(&["image", "ls", "--format", "json"]).await?)?;
        // An empty store prints nothing rather than `[]` on some paths; that is no images, not a
        // parse failure.
        if stdout.trim().is_empty() {
            return Ok(false);
        }
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| ProviderError::ParseError(e.to_string()))?;
        Ok(entries.iter().any(|e| e["reference"].as_str() == Some(image)))
    }

    /// Creates the instance's workspace volume, tolerating one that already exists. `msb volume
    /// create` exits 1 with `volume already exists: <name>` on a taken name (confirmed against real
    /// `msb` 0.7.12), so a volume orphaned by an interrupted create would otherwise brick every
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

    /// One-shot `msb exec --stream` with `stdin` written to completion. Spawns `msb_command()`
    /// directly, so it bypasses the runner seam; callers gate on `mock_backend_enabled()`.
    async fn exec_streamed(
        &self,
        name: &str,
        argv: &[&str],
        opts: &ExecOpts,
        stdin: &[u8],
    ) -> Result<Output, ProviderError> {
        // Fails before spawning, unlike `exec_command`, because this path can return an error.
        self.start_env.env_for(name)?;
        let mut child = self
            .exec_command_inner(name, argv, opts)
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

    /// An exec carrying `opts.secret_env`. The values travel on stdin (see
    /// [`secret_exec_command`]); under `MIO_MOCK_BACKEND=1` the scripted runner instead journals
    /// the names, since it cannot take stdin.
    async fn exec_with_secrets(
        &self,
        name: &str,
        argv: &[&str],
        opts: &ExecOpts,
    ) -> Result<Output, ProviderError> {
        if crate::backends::mock_backend_enabled() {
            let mut env = self.start_env.env_for(name)?;
            env.extend(opts.secret_env.iter().cloned());
            let mut full = vec!["exec", name, "--"];
            full.extend_from_slice(argv);
            return self.run_env(&full, &env).await;
        }
        let (command, stdin) = secret_exec_command(argv, opts)?;
        let command: Vec<&str> = command.iter().map(String::as_str).collect();
        let plain = ExecOpts {
            workdir: opts.workdir.clone(),
            env: opts.env.clone(),
            secret_env: Vec::new(),
            detached: false,
        };
        self.exec_streamed(name, &command, &plain, &stdin).await
    }

    /// Configured, unspawned `msb exec --stream` command shared by the streamed paths. `--stream`
    /// is byte-faithful piped stdin/stdout with no PTY (spike finding 7), the microsandbox
    /// counterpart of Podman's `exec -i`.
    /// `msb exec` on a stopped sandbox is an implicit start, so it carries the start environment
    /// too. An `env_for` failure leaves it out: the start then fails closed on the missing variable.
    fn exec_command_inner(&self, name: &str, argv: &[&str], opts: &ExecOpts) -> Command {
        let mut cmd = crate::process::msb_command();
        match self.start_env.env_for(name) {
            Ok(env) => {
                cmd.envs(env);
            }
            Err(e) => {
                tracing::warn!(target: "msb", %name, error = %e, "start environment unavailable")
            }
        }
        cmd.arg("exec").arg("--stream");
        if let Some(workdir) = &opts.workdir {
            cmd.arg("--workdir").arg(workdir);
        }
        for (k, v) in &opts.env {
            cmd.arg("--env").arg(format!("{k}={v}"));
        }
        debug_assert!(opts.secret_env.is_empty(), "streamed exec does not carry secret_env");
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
        ProviderCapabilities {
            remote_targets: false,
            event_stream: false,
            host_bind_mount: false,
            egress_policy: true,
        }
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
        if spec.network.is_some() || !spec.env.is_empty() {
            // Network-proxy sidecars are Podman-only for v1
            // (`docs/features/network-proxy/02_dev.md`, Resolved Decision 9).
            return Err(ProviderError::CommandFailed(
                "microsandbox does not support a network attachment or environment variables"
                    .to_string(),
            ));
        }
        let volume = workspace_volume_name(&spec.name);
        self.ensure_volume(&volume).await?;
        // msb 0.7 validates the workdir in the guest at create; without a `/root/work` mount the
        // create fails `could not validate workdir in guest`.
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
        if let Some(egress) = &spec.egress {
            args.extend(super::msb_policy::render(egress));
        }
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        if let Err(e) = check(self.run_starting(&spec.name, &argv).await?) {
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

    /// `msb create` takes no command, so `spec.command` becomes an init script; addons arrive by
    /// `--mount-dir`, never `--copy-dir`, and no deny egress is set — the sidecar must resolve and
    /// dial (`docs/features/microsandbox-network-policy/spike/04_msb-mitm-sidecar.md`, 2f and 6d).
    async fn create_sidecar(&self, spec: &SidecarSpec) -> Result<String, ProviderError> {
        let mut args: Vec<String> =
            vec!["create".to_string(), spec.image.clone(), "--name".to_string(), spec.name.clone()];
        for m in &spec.mounts {
            args.push("--mount-dir".to_string());
            let suffix = if m.read_only { ":ro" } else { "" };
            args.push(format!("{}:{}{suffix}", m.host_path, m.dest));
        }
        for p in &spec.publish {
            args.push("-p".to_string());
            args.push(format!("{}:{}:{}", p.bind, p.host_port, p.guest_port));
        }
        for (k, v) in &spec.labels {
            args.push("--label".to_string());
            args.push(format!("{k}={v}"));
        }
        args.push("--script-raw".to_string());
        args.push(format!("start={}", super::msb_policy::script_body(&spec.env, &spec.command)));
        args.push("--init".to_string());
        args.push("/.msb/scripts/start".to_string());
        let out = self.run(&args.iter().map(String::as_str).collect::<Vec<_>>()).await?;
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let bind_failed = out.status.success() && port_bind_failed(&spec.name);
        if !out.status.success() || bind_failed {
            // A failed create still registers the record, and a bind failure leaves a running VM,
            // so the name has to be freed either way before a retry.
            let _ = self.run(&["rm", "--force", &spec.name]).await;
            if bind_failed || is_port_in_use(&stderr) {
                return Err(ProviderError::PortInUse(spec.name.clone()));
            }
            return Err(ProviderError::CommandFailed(stderr));
        }
        Ok(spec.name.clone())
    }

    /// Keeps the workspace volume: `rm --force` without `volume rm`, awaited before the create
    /// because a `kind=disk` volume admits one attached sandbox
    /// (`docs/features/microsandbox-network-policy/spike/01_egress-baseline.md`).
    async fn recreate_instance(&self, spec: &InstanceSpec) -> Result<String, ProviderError> {
        check(self.run(&["rm", "--force", &spec.name]).await?)?;
        self.create_instance(spec).await
    }

    async fn stop_sidecar(&self, name: &str) -> Result<(), ProviderError> {
        check(self.run(&["stop", "-f", name]).await?).map(|_| ())
    }

    async fn pull_image(&self, image: &str) -> Result<(), ProviderError> {
        if self.image_present(image).await? {
            return Ok(());
        }
        check(self.run(&["pull", image]).await?).map(|_| ())
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
        let out = self.run_starting(name, &["start", name]).await?;
        if out.status.success() {
            // A stopped sidecar releases its published port, so a start can lose it to another
            // process; only `runtime.log` says so.
            if port_bind_failed(name) {
                return Err(ProviderError::PortInUse(name.to_string()));
            }
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
        if !opts.secret_env.is_empty() {
            return self.exec_with_secrets(name, argv, &opts).await;
        }
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
        self.run_starting(name, &full.iter().map(String::as_str).collect::<Vec<_>>()).await
    }

    async fn exec_piped(
        &self,
        name: &str,
        argv: &[&str],
        stdin: &[u8],
    ) -> Result<Output, ProviderError> {
        if crate::backends::mock_backend_enabled() {
            // Spawns `msb_command()` directly, so `ScriptedMsbRunner` cannot divert it and a light
            // spec would drive the real bundled `msb` — the gate `src-tauri/AGENTS.md` requires of any spawning method, as
            // `CliBackend::listen_events` already does. The scripted runner models one-shot
            // calls with no stdin, which is exactly what a `copy_in` under mock needs to be: the
            // call is recorded as `msb.exec` and the payload goes nowhere, since no guest exists to
            // receive it.
            let mut full = vec!["exec", name, "--"];
            full.extend_from_slice(argv);
            return self.run_starting(name, &full).await;
        }
        self.exec_streamed(name, argv, &ExecOpts::default(), stdin).await
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

    async fn create_network(&self, _name: &str, _subnet: &str) -> Result<(), ProviderError> {
        Err(ProviderError::CommandFailed(
            "microsandbox chains the proxy through host loopback and needs no network".to_string(),
        ))
    }

    async fn remove_network(&self, _name: &str) -> Result<(), ProviderError> {
        Err(ProviderError::CommandFailed(
            "microsandbox chains the proxy through host loopback and needs no network".to_string(),
        ))
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
        MsbProvider {
            runner: msb_runner,
            podman,
            start_env: Arc::new(super::super::types::NoStartEnv),
            build_lock: tokio::sync::Mutex::new(()),
        }
    }

    fn spec(name: &str) -> InstanceSpec {
        InstanceSpec {
            name: name.to_string(),
            image: "python".to_string(),
            workspace_source: "C:/work".to_string(),
            labels: vec![(WORKSPACE_LABEL.to_string(), "C:/work".to_string())],
            env: Vec::new(),
            network: None,
            egress: None,
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

    struct FakeEnv(Result<Vec<(String, String)>, String>);

    impl StartEnvSource for FakeEnv {
        fn env_for(&self, _instance: &str) -> Result<Vec<(String, String)>, ProviderError> {
            self.0.clone().map_err(ProviderError::CommandFailed)
        }
    }

    fn provider_with_env(director: Arc<MockDirector>, env: FakeEnv) -> MsbProvider {
        MsbProvider { start_env: Arc::new(env), ..provider_with(director) }
    }

    /// Every start-capable call carries the start environment by name, and the value never
    /// reaches argv.
    #[tokio::test]
    async fn start_capable_calls_carry_the_start_environment() {
        let director = Arc::new(MockDirector::new());
        let env = FakeEnv(Ok(vec![("GH_TOKEN".to_string(), "s3cret-value".to_string())]));
        let provider = provider_with_env(director.clone(), env);
        provider.create_instance(&spec("test-sbx")).await.expect("create succeeds");
        provider.stop("test-sbx").await.expect("stop succeeds");
        provider.start("test-sbx").await.expect("start succeeds");
        provider.exec("test-sbx", &["true"], ExecOpts::default()).await.expect("exec succeeds");
        for key in ["msb.create", "msb.start", "msb.exec"] {
            let calls = director.calls(key);
            assert_eq!(calls.len(), 1, "{key} journalled once");
            assert_eq!(calls[0].env_names, ["GH_TOKEN"], "{key} carries the variable");
            assert!(calls[0].argv.iter().all(|a| !a.contains("s3cret-value")), "{key} argv clean");
        }
        assert!(director.calls("msb.stop")[0].env_names.is_empty(), "stop never starts");
    }

    /// `msb exec --env` is `KEY=value` only, so a secret becomes a stdin line read by a guest
    /// wrapper; argv carries the variable name, never the value.
    #[test]
    fn secret_exec_command_keeps_the_value_off_argv() {
        let opts = ExecOpts {
            secret_env: vec![("PASEO_PASSWORD".to_string(), "s3cret-value".to_string())],
            ..Default::default()
        };
        let (command, stdin) = secret_exec_command(&["paseo", "daemon"], &opts).expect("renders");
        assert!(command[2].contains("export PASEO_PASSWORD="), "name is in the guest script");
        assert_eq!(&command[3..], ["sh", "paseo", "daemon"]);
        assert!(command.iter().all(|a| !a.contains("s3cret-value")), "value absent from argv");
        assert_eq!(stdin, b"s3cret-value\n");
    }

    #[test]
    fn secret_exec_command_wraps_a_detached_command() {
        let opts = ExecOpts {
            detached: true,
            secret_env: vec![("A".to_string(), "x".to_string())],
            ..Default::default()
        };
        let (command, _) = secret_exec_command(&["serve"], &opts).expect("renders");
        assert_eq!(&command[4..6], ["bash", "-c"]);
        assert!(command[6].contains("disown"));
    }

    #[test]
    fn secret_exec_command_refuses_what_a_line_cannot_carry() {
        let multiline = ExecOpts {
            secret_env: vec![("A".to_string(), "two\nlines".to_string())],
            ..Default::default()
        };
        assert!(secret_exec_command(&["true"], &multiline).is_err());
        let bad_name = ExecOpts {
            secret_env: vec![("A; rm".to_string(), "x".to_string())],
            ..Default::default()
        };
        assert!(secret_exec_command(&["true"], &bad_name).is_err());
    }

    #[tokio::test]
    async fn a_start_environment_error_aborts_before_any_msb_call() {
        let director = Arc::new(MockDirector::new());
        let env = FakeEnv(Err("secret \"gh\" has no stored value".to_string()));
        let provider = provider_with_env(director.clone(), env);
        let err = provider.start("test-sbx").await.expect_err("start fails");
        assert!(err.to_string().contains("no stored value"));
        assert!(director.calls("msb.start").is_empty());
    }

    fn sidecar_spec() -> SidecarSpec {
        use super::super::types::{PortPublish, SidecarMount};
        SidecarSpec {
            name: "s-proxy".to_string(),
            image: "docker.io/mitmproxy/mitmproxy:11.1.3".to_string(),
            network: None,
            mounts: vec![
                SidecarMount {
                    host_path: "C:/p/addons".to_string(),
                    dest: "/addons".to_string(),
                    read_only: true,
                },
                SidecarMount {
                    host_path: "C:/p/flows".to_string(),
                    dest: "/flows".to_string(),
                    read_only: false,
                },
            ],
            publish: vec![PortPublish { bind: "127.0.0.1".to_string(), host_port: 50123, guest_port: 1080 }],
            env: vec![("MIO_FLOWS_PATH".to_string(), "/flows/flows.ndjson".to_string())],
            command: vec!["mitmdump".to_string(), "--mode".to_string(), "socks5@0.0.0.0:1080".to_string()],
            labels: Vec::new(),
        }
    }

    #[tokio::test]
    async fn create_sidecar_maps_mounts_publish_and_init_script() {
        let director = Arc::new(MockDirector::new());
        let provider = provider_with(director.clone());
        provider.create_sidecar(&sidecar_spec()).await.expect("create succeeds");
        let argv = director.calls("msb.create")[0].argv.clone();
        let pos = |flag: &str| argv.iter().position(|a| a == flag).expect(flag);
        assert_eq!(argv[1], "docker.io/mitmproxy/mitmproxy:11.1.3");
        assert!(argv.windows(2).any(|w| w == ["--mount-dir", "C:/p/addons:/addons:ro"]));
        assert!(argv.windows(2).any(|w| w == ["--mount-dir", "C:/p/flows:/flows"]));
        assert!(argv.windows(2).any(|w| w == ["-p", "127.0.0.1:50123:1080"]));
        assert!(argv[pos("--script-raw") + 1].starts_with("start=#!/bin/sh\n"));
        assert_eq!(argv[pos("--init") + 1], "/.msb/scripts/start");
        assert!(pos("--script-raw") < pos("--init"));
        assert!(!argv.iter().any(|a| a == "--copy-dir" || a == "--net-default-egress"));
    }

    #[tokio::test]
    async fn create_sidecar_maps_an_address_in_use_failure() {
        let director = Arc::new(MockDirector::new());
        let msb_runner = Arc::new(ScriptedMsbRunner::new(director.clone()));
        let provider = provider_over(msb_runner.clone(), director.clone());
        director
            .configure(
                "msb.create",
                Behaviour::FailOnce {
                    exit_code: 1,
                    stderr: "bind 127.0.0.1:50123: Only one usage of each socket address".to_string(),
                },
            )
            .expect("known mock key");
        let err = provider.create_sidecar(&sidecar_spec()).await.expect_err("create fails");
        assert!(matches!(err, ProviderError::PortInUse(_)), "{err}");
        assert!(msb_runner.tracked_instance_names().is_empty(), "the record is removed for a retry");
    }

    #[tokio::test]
    async fn pull_image_skips_a_present_reference() {
        let director = Arc::new(MockDirector::new());
        let provider = provider_with(director.clone());
        provider.pull_image("docker.io/mitmproxy/mitmproxy:11.1.3").await.expect("pull succeeds");
        provider.pull_image("docker.io/mitmproxy/mitmproxy:11.1.3").await.expect("repeat succeeds");
        assert_eq!(director.calls("msb.pull").len(), 1);
    }

    #[tokio::test]
    async fn stop_sidecar_forces() {
        let director = Arc::new(MockDirector::new());
        let provider = provider_with(director.clone());
        provider.stop_sidecar("s-proxy").await.expect("stop succeeds");
        assert_eq!(director.calls("msb.stop")[0].argv, ["stop", "-f", "s-proxy"]);
    }

    /// The line shape recorded in `docs/features/microsandbox-network-policy/spike/02_secrets.md`,
    /// Item 2.
    #[test]
    fn parses_a_secret_violation_line() {
        let line = "2026-11-02T10:00:00.1Z  WARN microsandbox_network::engine::secrets::handler: secret violation: placeholder detected where substitution or passthrough is not permitted action=block-and-log secret_env_var=SPIKE_TOK placeholder=$MSB_SPIKE_TOK protocol=http/1.1 sni=postman-echo.com host=postman-echo.com method=GET path=/headers location=header match_form=raw";
        let v = parse_violation(line).expect("parses");
        assert_eq!(v.action, "block-and-log");
        assert_eq!(v.env_var, "SPIKE_TOK");
        assert_eq!(v.host, "postman-echo.com");
        assert_eq!((v.method.as_str(), v.path.as_str(), v.location.as_str()), ("GET", "/headers", "header"));
        assert_eq!(v.ts, "2026-11-02T10:00:00.1Z");
        assert!(parse_violation("INFO sandbox starting").is_none());
    }

    #[tokio::test]
    async fn recreate_instance_keeps_the_workspace_volume() {
        let director = Arc::new(MockDirector::new());
        let msb_runner = Arc::new(ScriptedMsbRunner::new(director.clone()));
        let provider = provider_over(msb_runner.clone(), director.clone());
        provider.create_instance(&spec("test-sbx")).await.expect("create succeeds");
        provider.recreate_instance(&spec("test-sbx")).await.expect("recreate succeeds");
        assert!(director.calls("msb.volume-rm").is_empty(), "the volume must survive");
        assert_eq!(msb_runner.tracked_instance_names(), ["test-sbx"]);
        assert_eq!(msb_runner.tracked_volumes(), ["test-sbx-ws"]);
    }
}
