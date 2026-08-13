//! Runtime control plane for the e2e `light` tier (`MIO_MOCK_BACKEND=1`). Scripts the external
//! **process** each subsystem shells out to (`podman`, the Mutagen sidecar) rather than a
//! subsystem-level mock, so the real orchestration structs (`PodmanCliBackend`,
//! `PodmanMachineManager`, ...) run unmodified against a scripted [`ProcessRunner`]. Exposed to
//! running specs via the `mock_configure`/`mock_reset` Tauri commands in `src-tauri/src/lib.rs`.
//! See `docs/features/scriptable-mock-backend/02_dev.md`.

pub mod types;

use std::collections::HashSet;
use std::process::Output;
use std::sync::{Arc, LazyLock};

use crate::process_ext::NoWindow;

use types::{
    Behaviour, MockDirector, ProcessRunner, ScriptedMsbRunner, ScriptedMutagenRunner,
    ScriptedOutput, ScriptedPodmanRunner, SystemMsbRunner, SystemMutagenRunner, SystemPodmanRunner,
    TrackedContainer, TrackedMsbInstance,
};

#[async_trait::async_trait]
impl ProcessRunner for SystemPodmanRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        crate::backends::podman_command().args(args).output().await
    }
}

/// `msb` is never bundled (see [`SystemMsbRunner`]'s doc comment), so this only needs to honour an
/// `MSB_PATH` override the way `podman_command()` honours its bundled resource dir — real `msb`
/// itself also reads `MSB_PATH`/`MSB_HOME` directly from the inherited process environment for its
/// own state isolation, which this app does not need to (and must not) re-derive.
pub(crate) fn msb_command() -> tokio::process::Command {
    let mut cmd = match std::env::var_os("MSB_PATH") {
        Some(dir) => tokio::process::Command::new(std::path::Path::new(&dir).join("msb.exe")),
        None => tokio::process::Command::new("msb"),
    };
    cmd.no_window();
    cmd
}

#[async_trait::async_trait]
impl ProcessRunner for SystemMsbRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        msb_command().args(args).output().await
    }
}

impl SystemMutagenRunner {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl ProcessRunner for SystemMutagenRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        // Sidecar-resolution and data-dir errors collapse into one io::Error here; the caller
        // (`sync::run_via`) re-wraps it as `SyncError::Sidecar`, losing the `DataDir` distinction.
        let command = crate::sync::command(&self.app).map_err(std::io::Error::other)?;
        let output = command.args(args).output().await.map_err(std::io::Error::other)?;
        Ok(Output {
            status: exit_status(output.status.code().unwrap_or(-1)),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

fn validate_behaviour(behaviour: &Behaviour) -> Result<(), String> {
    match behaviour {
        Behaviour::FailOnce { exit_code, .. } | Behaviour::FailAlways { exit_code, .. } => {
            if *exit_code == 0 {
                return Err("a fail behaviour must set a non-zero exit_code".to_string());
            }
        }
        Behaviour::ReturnValue { .. } | Behaviour::ReturnSequence { .. } => {}
    }
    Ok(())
}

/// Every scriptable `<binary>.<subcommand>` key. `mock_configure` rejects any key absent here, so
/// a typo or an un-derivable call shape fails loudly at configure time, not silently at consult
/// time. See `docs/features/scriptable-mock-backend/02_dev.md`, "Key matching strategy".
static KNOWN_CALLS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "podman.create",
        "podman.ps",
        "podman.start",
        "podman.stop",
        "podman.rm",
        "podman.machine-list",
        "podman.machine-start",
        "podman.system-connection-list",
        "podman.image-exists",
        "podman.build",
        "podman.info",
        // Mutagen's `sync` subcommands only — `forward`/`version`/`daemon` reach the sidecar via
        // `sync::run` (NetworkManager territory), unscripted for now (docs/features/scriptable-mock-backend/04_improvements.md).
        "mutagen.sync-create",
        "mutagen.sync-pause",
        "mutagen.sync-resume",
        "mutagen.sync-terminate",
        "mutagen.sync-flush",
        "mutagen.sync-list",
        // `msb.*` keys backed by `ScriptedMsbRunner`'s tracked-instance round trip
        // (docs/features/microsandbox-provider/02_dev.md, Phase F), extended by Phase G's
        // `MsbProvider` for the base-image and exec paths.
        "msb.create",
        "msb.ls",
        "msb.start",
        "msb.stop",
        "msb.rm",
        "msb.exec",
        "msb.inspect",
        "msb.load",
        "msb.volume-create",
        "msb.volume-rm",
        "msb.image-ls",
        // `MsbMachineManager::readiness`/`prepare` (Phase I).
        "msb.doctor",
        // `MsbProvider::ensure_base_image` builds the image with Podman before handing it to
        // `msb load` (docs/features/microsandbox-provider/02_dev.md, Phase G).
        "podman.save",
    ])
});

impl MockDirector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install `behaviour` under `key`, rejecting an unknown key or an inconsistent payload
    /// (e.g. a fail behaviour with `exit_code: 0`).
    pub fn configure(&self, key: &str, behaviour: Behaviour) -> Result<(), String> {
        if !KNOWN_CALLS.contains(key) {
            return Err(format!("unknown mock key {key:?}"));
        }
        validate_behaviour(&behaviour)?;
        self.overrides.lock().unwrap().insert(key.to_string(), behaviour);
        Ok(())
    }

    /// Clear every installed override.
    pub fn reset(&self) {
        self.overrides.lock().unwrap().clear();
    }

    /// Consult the override for `key`, if any. Consumes a `FailOnce` override on the call that
    /// returns it, and a `ReturnSequence` override once its last entry is served.
    fn consult(&self, key: &str) -> Option<ScriptedOutput> {
        let mut overrides = self.overrides.lock().unwrap();
        let behaviour = overrides.get_mut(key)?;
        match behaviour {
            Behaviour::FailOnce { exit_code, stderr } => {
                let out = ScriptedOutput { exit_code: *exit_code, stdout: String::new(), stderr: stderr.clone() };
                overrides.remove(key);
                Some(out)
            }
            Behaviour::FailAlways { exit_code, stderr } => {
                Some(ScriptedOutput { exit_code: *exit_code, stdout: String::new(), stderr: stderr.clone() })
            }
            Behaviour::ReturnValue { stdout, exit_code } => {
                Some(ScriptedOutput { exit_code: *exit_code, stdout: stdout.clone(), stderr: String::new() })
            }
            Behaviour::ReturnSequence { values } => {
                if values.is_empty() {
                    overrides.remove(key);
                    return None;
                }
                let payload = values.remove(0);
                let exhausted = values.is_empty();
                let out = ScriptedOutput { exit_code: payload.exit_code, stdout: payload.stdout, stderr: String::new() };
                if exhausted {
                    overrides.remove(key);
                }
                Some(out)
            }
        }
    }
}

#[cfg(windows)]
fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code as u32)
}

#[cfg(unix)]
fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code)
}

fn to_output(scripted: ScriptedOutput) -> Output {
    Output {
        status: exit_status(scripted.exit_code),
        stdout: scripted.stdout.into_bytes(),
        stderr: scripted.stderr.into_bytes(),
    }
}

/// Strips a leading `--connection <name>` pair (present on most `backends`/`runtime` calls,
/// absent on `machine`'s calls).
fn strip_connection<'a>(args: &'a [&'a str]) -> &'a [&'a str] {
    match args {
        ["--connection", _, rest @ ..] => rest,
        _ => args,
    }
}

/// Derives a `podman.<subcommand>` key from connection-stripped args; per-root nesting depth is
/// hard-coded — see `docs/features/scriptable-mock-backend/02_dev.md`, "Key matching strategy".
fn derive_podman_key(stripped: &[&str]) -> String {
    let Some(&root) = stripped.first() else {
        return "podman.".to_string();
    };
    let depth = match root {
        "machine" => 2,
        "system" if stripped.get(1) == Some(&"connection") => 3,
        "image" => 2,
        _ => 1,
    };
    let depth = depth.min(stripped.len());
    format!("podman.{}", stripped[..depth].join("-"))
}

fn arg_after<'a>(args: &[&'a str], flag: &str) -> Option<&'a str> {
    args.iter().position(|&a| a == flag).and_then(|i| args.get(i + 1).copied())
}

/// Derives a `msb.<subcommand>` key. Depth-2 for `volume`/`image` (`msb volume create`, `msb image
/// ls`), matching `derive_podman_key`'s `machine`/`system connection`/`image` special-casing; depth-1
/// for every other call (`msb ls`, `msb create ...`, `msb start <name>`, `msb exec <name> -- ...`,
/// `msb inspect <name> ...`, `msb load ...`). A future two-word subcommand outside `volume`/`image`
/// (e.g. `msb ssh serve`, Phase H) needs its own case here.
fn derive_msb_key(args: &[&str]) -> String {
    let Some(&root) = args.first() else {
        return "msb.".to_string();
    };
    let depth = match root {
        "volume" | "image" => 2,
        _ => 1,
    };
    let depth = depth.min(args.len());
    format!("msb.{}", args[..depth].join("-"))
}

impl ScriptedPodmanRunner {
    pub fn new(director: Arc<MockDirector>) -> Self {
        Self { director, containers: std::sync::Mutex::new(Vec::new()) }
    }

    fn track_create(&self, stripped: &[&str]) -> ScriptedOutput {
        let name = arg_after(stripped, "--name").unwrap_or_default().to_string();
        let workspace_label = arg_after(stripped, "--label")
            .and_then(|l| l.strip_prefix("sandbox-mio.workspace="))
            .unwrap_or_default()
            .to_string();
        let id = format!("mock-{name}");
        self.containers.lock().unwrap().push(TrackedContainer { name, workspace_label, running: false });
        ScriptedOutput { exit_code: 0, stdout: id, stderr: String::new() }
    }

    fn set_running(&self, stripped: &[&str], running: bool) -> ScriptedOutput {
        let Some(&name) = stripped.last() else {
            return ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() };
        };
        if let Some(c) = self.containers.lock().unwrap().iter_mut().find(|c| c.name == name) {
            c.running = running;
        }
        ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }

    fn track_remove(&self, stripped: &[&str]) -> ScriptedOutput {
        if let Some(&name) = stripped.last() {
            self.containers.lock().unwrap().retain(|c| c.name != name);
        }
        ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }

    fn tracked_ps(&self) -> ScriptedOutput {
        let entries: Vec<serde_json::Value> = self
            .containers
            .lock()
            .unwrap()
            .iter()
            .map(|c| {
                serde_json::json!({
                    "Id": format!("mock-{}", c.name),
                    "Names": [c.name],
                    "State": if c.running { "running" } else { "created" },
                    "Labels": { "sandbox-mio.workspace": c.workspace_label },
                    "Image": "",
                })
            })
            .collect();
        let stdout = serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string());
        ScriptedOutput { exit_code: 0, stdout, stderr: String::new() }
    }
}

/// Built-in no-override defaults — a bare `""` doesn't hold for every key: `system-connection-list`
/// needs `"[]"` to parse as JSON, and `machine-list` needs the default connection already running,
/// or every light spec hits the podman-startup-recovery gate first (docs/features/scriptable-mock-backend/02_dev.md, "No-override invariant").
fn default_output(key: &str) -> ScriptedOutput {
    let stdout = match key {
        "podman.system-connection-list" => "[]".to_string(),
        "podman.machine-list" => {
            r#"[{"Name":"podman-machine-default","Running":true}]"#.to_string()
        }
        // Mirrors `podman.machine-list`'s "already running" default: an unconfigured `msb doctor`
        // reports ready so a light spec that never scripts it doesn't land on a setup screen.
        "msb.doctor" => "Host setup is ready".to_string(),
        _ => String::new(),
    };
    ScriptedOutput { exit_code: 0, stdout, stderr: String::new() }
}

/// Derives a `mutagen.<subcommand>` key: depth 2 under a command group (`sync`, `forward`,
/// `daemon`), depth 1 otherwise (`version`) — simpler than Podman's per-root table.
fn derive_mutagen_key(args: &[&str]) -> String {
    let Some(&root) = args.first() else {
        return "mutagen.".to_string();
    };
    let depth = match root {
        "sync" | "forward" | "daemon" => 2,
        _ => 1,
    };
    let depth = depth.min(args.len());
    format!("mutagen.{}", args[..depth].join("-"))
}

/// No-override default for a scripted Mutagen call: `sync-list` must answer `"[]"`, not `""` —
/// `MutagenCli::status` parses stdout as JSON and fails on empty; `sessions()` tolerates it.
fn default_mutagen_output(key: &str) -> ScriptedOutput {
    let stdout = match key {
        "mutagen.sync-list" => "[]".to_string(),
        _ => String::new(),
    };
    ScriptedOutput { exit_code: 0, stdout, stderr: String::new() }
}

impl ScriptedMutagenRunner {
    pub fn new(director: Arc<MockDirector>) -> Self {
        Self { director }
    }
}

#[async_trait::async_trait]
impl ProcessRunner for ScriptedMutagenRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        let key = derive_mutagen_key(args);
        let scripted = self
            .director
            .consult(&key)
            .unwrap_or_else(|| default_mutagen_output(&key));
        Ok(to_output(scripted))
    }
}

#[async_trait::async_trait]
impl ProcessRunner for ScriptedPodmanRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        let stripped = strip_connection(args);
        let key = derive_podman_key(stripped);
        if let Some(scripted) = self.director.consult(&key) {
            return Ok(to_output(scripted));
        }
        let scripted = match key.as_str() {
            "podman.create" => self.track_create(stripped),
            "podman.start" => self.set_running(stripped, true),
            "podman.stop" => self.set_running(stripped, false),
            "podman.rm" => self.track_remove(stripped),
            "podman.ps" => self.tracked_ps(),
            _ => default_output(&key),
        };
        Ok(to_output(scripted))
    }
}

impl ScriptedMsbRunner {
    pub fn new(director: Arc<MockDirector>) -> Self {
        Self {
            director,
            instances: std::sync::Mutex::new(Vec::new()),
            volumes: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// `msb volume create <name> --kind disk --size <n>`: rejects a duplicate name exactly as real
    /// `msb` 0.6.6 does, so `MsbProvider::ensure_volume`'s tolerance of a leftover volume is
    /// exercised under mock rather than only against the real binary.
    fn track_volume_create(&self, args: &[&str]) -> ScriptedOutput {
        let name = args.get(2).copied().unwrap_or_default().to_string();
        let mut volumes = self.volumes.lock().unwrap();
        if volumes.iter().any(|v| *v == name) {
            return ScriptedOutput {
                exit_code: 1,
                stdout: String::new(),
                stderr: format!("error: volume already exists: {name}"),
            };
        }
        volumes.push(name.clone());
        ScriptedOutput { exit_code: 0, stdout: name, stderr: String::new() }
    }

    /// Test-only view of the tracked volumes, so a provider test can assert a rollback removed one
    /// outright rather than inferring it from a later call happening to succeed.
    #[cfg(test)]
    pub(crate) fn tracked_volumes(&self) -> Vec<String> {
        self.volumes.lock().unwrap().clone()
    }

    fn track_volume_remove(&self, args: &[&str]) -> ScriptedOutput {
        if let Some(&name) = args.get(2) {
            self.volumes.lock().unwrap().retain(|v| v != name);
        }
        ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }

    /// `msb create <image> --name <name> ...`: the name and workspace label sit at fixed flags
    /// exactly like `track_create`'s Podman argv, just without a leading `--connection` pair to
    /// strip.
    fn track_create(&self, args: &[&str]) -> ScriptedOutput {
        let name = arg_after(args, "--name").unwrap_or_default().to_string();
        let workspace_label = arg_after(args, "--label")
            .and_then(|l| l.strip_prefix("sandbox-mio.workspace="))
            .unwrap_or_default()
            .to_string();
        self.instances.lock().unwrap().push(TrackedMsbInstance { name, workspace_label, running: false });
        ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }

    fn set_running(&self, args: &[&str], running: bool) -> ScriptedOutput {
        let Some(&name) = args.last() else {
            return ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() };
        };
        if let Some(i) = self.instances.lock().unwrap().iter_mut().find(|i| i.name == name) {
            i.running = running;
        }
        ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }

    fn track_remove(&self, args: &[&str]) -> ScriptedOutput {
        if let Some(&name) = args.last() {
            self.instances.lock().unwrap().retain(|i| i.name != name);
        }
        ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }

    /// `msb ls --format json` shape: exactly `name`/`status`/`created_at`/`image`, PascalCase
    /// status, no labels — `MsbProvider::list_instances` (Phase G) recovers
    /// `sandbox-mio.workspace` from `msb inspect` instead, which this tracked default does not yet
    /// model since no light spec drives that path.
    fn tracked_ls(&self) -> ScriptedOutput {
        let entries: Vec<serde_json::Value> = self
            .instances
            .lock()
            .unwrap()
            .iter()
            .map(|i| {
                serde_json::json!({
                    "name": i.name,
                    "status": if i.running { "Running" } else { "Stopped" },
                    "created_at": "",
                    "image": "",
                })
            })
            .collect();
        let stdout = serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string());
        ScriptedOutput { exit_code: 0, stdout, stderr: String::new() }
    }

    /// `msb inspect <name> --format json`: `args[0]` is the `inspect` root itself (no `--connection`
    /// pair to strip, unlike Podman), so the name is the first positional argument after it. Reports
    /// back the `sandbox-mio.workspace` label `track_create` recorded, at the same `active_config.
    /// labels` path the real spike-verified JSON shape uses
    /// (`docs/features/microsandbox-provider/spike/hands-on-cli.md` addendum).
    fn tracked_inspect(&self, args: &[&str]) -> ScriptedOutput {
        let name = args.get(1).copied().unwrap_or_default();
        let label = self
            .instances
            .lock()
            .unwrap()
            .iter()
            .find(|i| i.name == name)
            .map(|i| i.workspace_label.clone())
            .unwrap_or_default();
        let mut labels = serde_json::Map::new();
        labels.insert(crate::provider::WORKSPACE_LABEL.to_string(), serde_json::Value::String(label));
        let body = serde_json::json!({ "active_config": { "labels": serde_json::Value::Object(labels) } });
        ScriptedOutput { exit_code: 0, stdout: body.to_string(), stderr: String::new() }
    }
}

#[async_trait::async_trait]
impl ProcessRunner for ScriptedMsbRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        let key = derive_msb_key(args);
        if let Some(scripted) = self.director.consult(&key) {
            return Ok(to_output(scripted));
        }
        let scripted = match key.as_str() {
            "msb.create" => self.track_create(args),
            "msb.start" => self.set_running(args, true),
            "msb.stop" => self.set_running(args, false),
            "msb.rm" => self.track_remove(args),
            "msb.ls" => self.tracked_ls(),
            "msb.inspect" => self.tracked_inspect(args),
            "msb.volume-create" => self.track_volume_create(args),
            "msb.volume-rm" => self.track_volume_remove(args),
            _ => default_output(&key),
        };
        Ok(to_output(scripted))
    }
}
