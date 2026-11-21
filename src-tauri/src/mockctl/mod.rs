//! Runtime control plane for the e2e `light` tier (`MIO_MOCK_BACKEND=1`). Scripts the external
//! **process** each subsystem shells out to (`podman`, the Mutagen sidecar) rather than a
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
use std::time::Duration;
use std::sync::{Arc, LazyLock};

use crate::process::{exit_status, strip_connection};

use types::{
    Behaviour, MockDirector, ProcessRunner, RecordedCall, ScriptedMutagenRunner, ScriptedOutput,
    ScriptedPodmanRunner, TrackedContainer,
};

/// The longest scripted delay, so a typo cannot hang a spec past its own timeout.
const MAX_DELAY_MS: u64 = 30_000;

fn validate_behaviour(behaviour: &Behaviour) -> Result<(), String> {
    match behaviour {
        Behaviour::FailOnce { exit_code, .. } | Behaviour::FailAlways { exit_code, .. } => {
            if *exit_code == 0 {
                return Err("a fail behaviour must set a non-zero exit_code".to_string());
            }
        }
        Behaviour::Delay { ms, calls } => {
            if *ms == 0 || *ms > MAX_DELAY_MS {
                return Err(format!("a delay must be between 1 and {MAX_DELAY_MS} ms"));
            }
            if *calls == Some(0) {
                return Err("a delay's calls must be at least 1".to_string());
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
        // `MachineProvider::sidecar_logs` and `exec`, scripted to stand in for the egress sidecar's
        // audit lines and reload failures (docs/features/podman-egress-boundary/02_dev.md, Phase B).
        // An unscripted `podman.logs` reads back as no lines.
        "podman.logs",
        "podman.exec",
        // Mutagen's `sync` subcommands only — `forward`/`version`/`daemon` reach the sidecar via
        // `sync::run` (NetworkManager territory), unscripted for now (docs/features/scriptable-mock-backend/04_improvements.md).
        "mutagen.sync-create",
        "mutagen.sync-pause",
        "mutagen.sync-resume",
        "mutagen.sync-terminate",
        "mutagen.sync-flush",
        "mutagen.sync-list",
        // The resource monitor's samples (docs/features/sandbox-resource-monitor/02_dev.md,
        // Resolved Decision 15). Unscripted, both read back as no running instances.
        "podman.stats",
        // The native terminal's open step under the mock `Transport`
        // (`src-tauri/src/terminal/mock.rs`, docs/features/native-terminal/02_dev.md).
        "terminal.open",
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

    pub(crate) fn record(&self, key: &str, args: &[&str], env: &[(String, String)]) {
        self.calls.lock().unwrap().push(RecordedCall {
            key: key.to_string(),
            argv: args.iter().map(|a| a.to_string()).collect(),
            env_names: env.iter().map(|(k, _)| k.clone()).collect(),
        });
    }

    /// Serves a scripted call: holds it for an installed `Delay`, then returns the scripted result
    /// of any other override. Every runner goes through here, so a delay applies to every key.
    pub(crate) async fn script(&self, key: &str) -> Option<ScriptedOutput> {
        if let Some(hold) = self.take_delay(key) {
            tokio::time::sleep(hold).await;
        }
        self.consult(key)
    }

    /// The hold of an installed `Delay` for `key`, consuming one of its `calls`.
    fn take_delay(&self, key: &str) -> Option<Duration> {
        let mut overrides = self.overrides.lock().unwrap();
        let Some(Behaviour::Delay { ms, calls }) = overrides.get_mut(key) else {
            return None;
        };
        let hold = Duration::from_millis(*ms);
        let exhausted = match calls {
            Some(n) => {
                *n -= 1;
                *n == 0
            }
            None => false,
        };
        if exhausted {
            overrides.remove(key);
        }
        Some(hold)
    }

    /// Consult the override for `key`, if any. Consumes a `FailOnce` override on the call that
    /// returns it, and a `ReturnSequence` override once its last entry is served.
    fn consult(&self, key: &str) -> Option<ScriptedOutput> {
        let mut overrides = self.overrides.lock().unwrap();
        let behaviour = overrides.get_mut(key)?;
        match behaviour {
            Behaviour::Delay { .. } => None,
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
            .script(&key)
            .await
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
        if let Some(scripted) = self.director.script(&key).await {
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

    /// Scripted and journalled like any other call; the payload itself is not journalled.
    async fn run_with_stdin(&self, args: &[&str], _stdin: &[u8]) -> std::io::Result<Output> {
        self.run_with_env(args, &[]).await
    }
}
