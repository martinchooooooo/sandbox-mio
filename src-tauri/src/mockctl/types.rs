use std::sync::{Arc, Mutex};

pub(crate) use crate::process::types::ProcessRunner;

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
    /// Holds each matching call for `ms` milliseconds, then lets it take its default path. With
    /// `calls`, only that many calls are held and the override is then consumed. Mirrored by
    /// `Behaviour` in `e2e/lib/mock.ts`.
    Delay {
        ms: u64,
        #[serde(default)]
        calls: Option<u32>,
    },
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ReturnPayload {
    pub stdout: String,
    #[serde(default)]
    pub exit_code: i32,
}

/// A scripted call's raw result, converted to a real [`Output`] by each runner.
pub(crate) struct ScriptedOutput {
    pub(crate) exit_code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// Mutex-guarded override registry, shared as `Arc<MockDirector>` Tauri state and handed into
/// every scripted [`ProcessRunner`]. Real runners never receive it.
#[derive(Default)]
pub struct MockDirector {
    pub(super) overrides: Mutex<std::collections::HashMap<String, Behaviour>>,
    /// Every scripted call since the last reset, so a light spec can assert rendered argv.
    pub(super) calls: Mutex<Vec<RecordedCall>>,
}

/// One journalled call. Carries the names of the extra environment variables the call was given,
/// never their values.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecordedCall {
    pub key: String,
    pub argv: Vec<String>,
    pub env_names: Vec<String>,
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
/// `CliBackend`'s logic, only enough state for `create` → `list` → `start` → `list` to round-
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
