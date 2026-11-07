//! Runtime control plane for the e2e `light` tier (`MIO_MOCK_BACKEND=1`). Scripts the external
//! **process** each subsystem shells out to (`podman`, `msb`, the Mutagen sidecar) rather than a
//! subsystem-level mock, so the real orchestration structs (`CliBackend`,
//! `PodmanMachineManager`, ...) run unmodified against a scripted [`ProcessRunner`]. Exposed to
//! running specs via the `mock_configure`/`mock_reset` Tauri commands in `src-tauri/src/commands/mockctl.rs`.
//! See `docs/features/scriptable-mock-backend/02_dev.md`.
//!
//! Scripted halves only. The real [`ProcessRunner`] implementations and the spawn machinery they
//! drive live in `src-tauri/src/process/`, which this module depends on rather than the reverse.

pub mod types;

use std::collections::HashSet;
use std::process::Output;
use std::sync::{Arc, LazyLock};

use crate::process::{exit_status, strip_connection};

use types::{
    Behaviour, MockDirector, ProcessRunner, RecordedCall, ScriptedMsbRunner,
    ScriptedMutagenRunner, ScriptedOutput, ScriptedPodmanRunner, TrackedContainer,
    TrackedMsbInstance,
};

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
        // `MachineProvider::create_network`/`remove_network` (docs/features/network-proxy/02_dev.md,
        // Phase A).
        "podman.network-create",
        "podman.network-rm",
        "podman.network-inspect",
        // `MachineProvider::pull_image` (docs/features/network-proxy/02_dev.md, Resolved Decision 10).
        "podman.pull",
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
        // `MsbProvider::pull_image` for the mitmproxy sidecar
        // (docs/features/microsandbox-network-policy/02_dev.md, Phase E).
        "msb.pull",
        // `MsbMachineManager::readiness`/`prepare` (Phase I), plus the minimum-version check
        // (docs/features/microsandbox-network-policy/02_dev.md, Phase A).
        "msb.doctor",
        "msb.--version",
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

    /// Clear every installed override and the call journal.
    pub fn reset(&self) {
        self.overrides.lock().unwrap().clear();
        self.calls.lock().unwrap().clear();
    }

    /// Journalled calls whose key is `key`, oldest first.
    pub fn calls(&self, key: &str) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().iter().filter(|c| c.key == key).cloned().collect()
    }

    fn record(&self, key: &str, args: &[&str], env: &[(String, String)]) {
        self.calls.lock().unwrap().push(RecordedCall {
            key: key.to_string(),
            argv: args.iter().map(|a| a.to_string()).collect(),
            env_names: env.iter().map(|(k, _)| k.clone()).collect(),
        });
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

fn to_output(scripted: ScriptedOutput) -> Output {
    Output {
        status: exit_status(scripted.exit_code),
        stdout: scripted.stdout.into_bytes(),
        stderr: scripted.stderr.into_bytes(),
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
        "network" => 2,
        _ => 1,
    };
    let depth = depth.min(stripped.len());
    format!("podman.{}", stripped[..depth].join("-"))
}

fn arg_after<'a>(args: &[&'a str], flag: &str) -> Option<&'a str> {
    args.iter().position(|&a| a == flag).and_then(|i| args.get(i + 1).copied())
}

/// Derives a `msb.<subcommand>` key. Depth-2 for `volume`/`image` (`msb volume create`, `msb
/// image ls`), matching `derive_podman_key`'s `machine`/`system connection`/
/// `image` special-casing; depth-1 for every other call (`msb ls`, `msb create ...`, `msb start
/// <name>`, `msb exec <name> -- ...`, `msb inspect <name> ...`, `msb load ...`). A future two-word
/// subcommand outside `volume`/`image` needs its own case here.
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
        // Must satisfy `MIN_MSB_VERSION` (`src-tauri/src/machine/msb.rs`) for the same reason.
        "msb.--version" => "msb 0.7.12".to_string(),
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
        self.run_with_env(args, &[]).await
    }

    /// Journals the variable names only; a value never leaves the caller.
    async fn run_with_env(
        &self,
        args: &[&str],
        env: &[(String, String)],
    ) -> std::io::Result<Output> {
        let stripped = strip_connection(args);
        let key = derive_podman_key(stripped);
        self.director.record(&key, stripped, env);
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
            images: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// `msb load --input <tar> --tag <ref>`: records the reference so a later `msb image ls`
    /// reports it, which is what makes `MsbProvider::ensure_base_image` idempotent under mock.
    fn track_load(&self, args: &[&str]) -> ScriptedOutput {
        if let Some(tag) = arg_after(args, "--tag") {
            let mut images = self.images.lock().unwrap();
            if !images.iter().any(|i| i == tag) {
                images.push(tag.to_string());
            }
        }
        ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }

    /// `msb pull <ref>`: records the reference like `track_load`, so a repeated `pull_image` is a
    /// no-op under mock as it is against the real store.
    fn track_pull(&self, args: &[&str]) -> ScriptedOutput {
        if let Some(&reference) = args.get(1) {
            let mut images = self.images.lock().unwrap();
            if !images.iter().any(|i| i == reference) {
                images.push(reference.to_string());
            }
        }
        ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }

    /// `msb image ls --format json`: real output carries digest/size/architecture too, but
    /// `reference` is the only field `MsbProvider` reads.
    fn tracked_image_ls(&self) -> ScriptedOutput {
        let entries: Vec<serde_json::Value> = self
            .images
            .lock()
            .unwrap()
            .iter()
            .map(|r| serde_json::json!({ "reference": r }))
            .collect();
        let stdout = serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string());
        ScriptedOutput { exit_code: 0, stdout, stderr: String::new() }
    }

    /// `msb volume create <name> --kind disk --size <n>`: rejects a duplicate name exactly as real
    /// `msb` 0.7.12 does, so `MsbProvider::ensure_volume`'s tolerance of a leftover volume is
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

    /// Test-only view of the tracked instance names, the record half of what a create rollback has
    /// to clear.
    #[cfg(test)]
    pub(crate) fn tracked_instance_names(&self) -> Vec<String> {
        self.instances.lock().unwrap().iter().map(|i| i.name.clone()).collect()
    }

    fn track_volume_remove(&self, args: &[&str]) -> ScriptedOutput {
        if let Some(&name) = args.get(2) {
            self.volumes.lock().unwrap().retain(|v| v != name);
        }
        ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }

    /// `msb create <image> --name <name> ...`: the name and workspace label sit at fixed flags
    /// exactly like `track_create`'s Podman argv, just without a leading `--connection` pair to
    /// strip. The instance is tracked as running because real `msb create` auto-starts it
    /// (`docs/features/microsandbox-provider/spike/hands-on-cli.md`, finding 3), unlike `podman
    /// create`.
    fn track_create(&self, args: &[&str]) -> ScriptedOutput {
        let name = arg_after(args, "--name").unwrap_or_default().to_string();
        let workspace_label = arg_after(args, "--label")
            .and_then(|l| l.strip_prefix("sandbox-mio.workspace="))
            .unwrap_or_default()
            .to_string();
        // `msb create <image> ...` — the image is the one positional, ahead of every flag.
        let image = args.get(1).copied().unwrap_or_default().to_string();
        self.instances.lock().unwrap().push(TrackedMsbInstance {
            name,
            workspace_label,
            running: true,
            image,
        });
        ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }

    /// `msb start <name>`: rejects an instance that is already running the way real `msb` 0.7.12
    /// does, so `MsbProvider::start`'s tolerance of that failure is exercised under mock rather than
    /// only against the real binary — the same reason `track_volume_create` models its duplicate.
    fn track_start(&self, args: &[&str]) -> ScriptedOutput {
        let Some(&name) = args.last() else {
            return ScriptedOutput { exit_code: 0, stdout: String::new(), stderr: String::new() };
        };
        let mut instances = self.instances.lock().unwrap();
        if let Some(i) = instances.iter_mut().find(|i| i.name == name) {
            if i.running {
                return ScriptedOutput {
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: format!(
                        "error: sandbox still running: cannot start sandbox '{name}': already running"
                    ),
                };
            }
            i.running = true;
        }
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
                    "image": i.image,
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
        self.run_with_env(args, &[]).await
    }

    async fn run_with_env(
        &self,
        args: &[&str],
        env: &[(String, String)],
    ) -> std::io::Result<Output> {
        let key = derive_msb_key(args);
        self.director.record(&key, args, env);
        if let Some(scripted) = self.director.consult(&key) {
            // Real `msb` 0.7.12 inserts the sandbox record before the step a failing `msb create`
            // dies on, so a scripted failure has to leave that record behind too — it is what
            // `MsbProvider::create_instance`'s rollback exists to remove.
            if key == "msb.create" && scripted.exit_code != 0 {
                self.track_create(args);
            }
            return Ok(to_output(scripted));
        }
        let scripted = match key.as_str() {
            "msb.create" => self.track_create(args),
            "msb.start" => self.track_start(args),
            "msb.stop" => self.set_running(args, false),
            "msb.rm" => self.track_remove(args),
            "msb.ls" => self.tracked_ls(),
            "msb.inspect" => self.tracked_inspect(args),
            "msb.volume-create" => self.track_volume_create(args),
            "msb.volume-rm" => self.track_volume_remove(args),
            "msb.load" => self.track_load(args),
            "msb.pull" => self.track_pull(args),
            "msb.image-ls" => self.tracked_image_ls(),
            _ => default_output(&key),
        };
        Ok(to_output(scripted))
    }
}
