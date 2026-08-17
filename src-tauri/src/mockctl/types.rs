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

/// One `msb create`d instance, tracked only so the built-in `msb.ls` default (no override
/// installed) can report it back — the microsandbox sibling of [`TrackedContainer`]. Status is
/// PascalCase (`Running`/`Stopped`), matching real `msb ls --format json` output rather than
/// Podman's lowercase vocabulary.
pub(super) struct TrackedMsbInstance {
    pub(super) name: String,
    pub(super) workspace_label: String,
    pub(super) running: bool,
    /// The `name:tag` the instance was created from, so `msb.ls` reports an `image` that parses
    /// into a `Sandbox::image_version` — the Podman side gets this for free from a real image.
    pub(super) image: String,
}

/// Scripted `msb` invocations, mirroring [`ScriptedPodmanRunner`]'s create→list→start→stop→list
/// round trip against `msb`-shaped JSON instead of Podman's.
pub struct ScriptedMsbRunner {
    pub(super) director: Arc<MockDirector>,
    pub(super) instances: Mutex<Vec<TrackedMsbInstance>>,
    /// Workspace volume names, tracked separately from `instances` because a volume outlives a
    /// failed `msb create` on the real binary — the leftover `MsbProvider::create_instance` rolls
    /// back. Modelled here so `msb volume create` can reject a duplicate name the way real `msb`
    /// 0.6.6 does (exit 1, `volume already exists: <name>`).
    pub(super) volumes: Mutex<Vec<String>>,
    /// Image references `msb load --tag <ref>` has imported, reported back by `msb image ls
    /// --format json` so `MsbProvider`'s base-image check is idempotent under mock the way
    /// `podman.image-exists` is for the Podman path.
    pub(super) images: Mutex<Vec<String>>,
}
