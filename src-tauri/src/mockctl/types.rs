use std::process::Output;
use std::sync::{Arc, Mutex};

/// Runs one external-tool invocation and returns its raw output. `args` excludes the binary name;
/// `args[0]` is the CLI subcommand (`"create"`, `"machine"`, `"sync"`, ...). Implemented once per
/// real tool (`SystemPodmanRunner`) and once per scripted tool (`ScriptedPodmanRunner`), so the
/// owning subsystem struct (`PodmanCliBackend`, `PodmanMachineManager`) is identical in both
/// production and the e2e `light` tier.
#[async_trait::async_trait]
pub trait ProcessRunner: Send + Sync {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output>;
}

/// Real `podman` invocations: wraps `podman_command()` (`src-tauri/src/backends/mod.rs`),
/// preserving its resource-dir/env resolution and `no_window()` behaviour byte-for-byte.
pub struct SystemPodmanRunner;

/// Real Mutagen sidecar invocations: wraps `sync::command()` (`src-tauri/src/sync/mod.rs`),
/// preserving its `MUTAGEN_DATA_DIRECTORY` isolation. Holds the [`AppHandle`] because the sidecar is
/// resolved through it, which is why this is a separate runner from [`SystemPodmanRunner`].
pub struct SystemMutagenRunner {
    pub(super) app: tauri::AppHandle,
}

/// One scripted behaviour installed against a `<binary>.<subcommand>` override key. The payload is
/// the process's raw text output, not a typed value — the real subsystem code parses it exactly
/// as it would parse genuine `podman`/`mutagen` output.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Behaviour {
    /// The next matching call exits non-zero with `stderr`, then the override is consumed.
    FailOnce { exit_code: i32, stderr: String },
    /// Every matching call exits non-zero with `stderr` until reset.
    FailAlways { exit_code: i32, stderr: String },
    /// Every matching call returns `stdout` (exit 0 unless `exit_code` overrides it).
    ReturnValue {
        stdout: String,
        #[serde(default)]
        exit_code: i32,
    },
    /// Each matching call consumes one entry; once exhausted, the default (no-override) path resumes.
    ReturnSequence { values: Vec<ReturnPayload> },
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ReturnPayload {
    pub stdout: String,
    #[serde(default)]
    pub exit_code: i32,
}

/// A scripted call's raw result, converted to a real [`Output`] by each runner.
pub(super) struct ScriptedOutput {
    pub(super) exit_code: i32,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

/// Mutex-guarded override registry, shared as `Arc<MockDirector>` Tauri state and handed into
/// every scripted [`ProcessRunner`]. Real runners never receive it.
#[derive(Default)]
pub struct MockDirector {
    pub(super) overrides: Mutex<std::collections::HashMap<String, Behaviour>>,
}

/// One `podman create`d container, tracked only so the built-in `podman.ps` default (no override
/// installed) can report it back — see [`ScriptedPodmanRunner`]'s doc comment.
pub(super) struct TrackedContainer {
    pub(super) name: String,
    pub(super) workspace_label: String,
    pub(super) running: bool,
}

/// Scripted `podman` invocations: derives the override key, consults `director`, and returns the
/// scripted [`Output`] when one is installed. With no override, `create`/`start`/`stop`/`rm` update
/// a small in-memory container list and `ps` reports it — **not** a second hand-maintained mock of
/// `PodmanCliBackend`'s logic, only enough state for `create` → `list` → `start` → `list` to round-
/// trip through the real struct's own parsing without every light spec having to script `podman.ps`
/// by hand for the common case. A spec that needs specific `ps` output (a fabricated status, a
/// foreign container) still overrides `podman.ps` directly, which always takes precedence over this
/// tracked state. See `docs/features/scriptable-mock-backend/02_dev.md`, "No-override invariant".
pub struct ScriptedPodmanRunner {
    pub(super) director: Arc<MockDirector>,
    pub(super) containers: Mutex<Vec<TrackedContainer>>,
}

/// Scripted Mutagen sidecar invocations. Stateless, unlike [`ScriptedPodmanRunner`]: no light-tier
/// flow creates a sync session and then expects to list it back, because deployment needs a real
/// remote host that the light tier never has. A spec that wants a session scripts
/// `mutagen.sync-list` directly.
pub struct ScriptedMutagenRunner {
    pub(super) director: Arc<MockDirector>,
}
