//! Mutagen file-sync engine, bundled as a Tauri sidecar.
//!
//! Mutagen ships as one self-contained binary that the app bundles via `externalBin` (see
//! `tauri.conf.json`) and invokes through the shell plugin. Tauri resolves the platform binary
//! (`mutagen-<target-triple>`) at runtime, so installs need no separately-installed Mutagen.
//!
//! Every invocation routes through [`command`], which pins `MUTAGEN_DATA_DIRECTORY` to an
//! app-private directory. Mutagen is daemon-based — the CLI is a thin client over a background
//! daemon that owns the sync sessions — and isolating the data directory keeps the app's daemon
//! and sessions from colliding with a Mutagen the user installed themselves.

pub mod types;

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_shell::process::Command;
use tauri_plugin_shell::ShellExt;

use types::{MutagenCli, SessionInfo, SyncEngine, SyncStatus};

/// Sidecar name as declared in `bundle.externalBin`.
const SIDECAR: &str = "mutagen";

/// App-private Mutagen state directory, created if absent. Isolating it from the default
/// data directory is what prevents the app's daemon and sessions clashing with a
/// user-installed Mutagen.
fn data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("mutagen");
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create mutagen data dir: {e}"))?;
    Ok(dir)
}

/// A Mutagen sidecar command with the app-private data directory applied. Callers append the
/// subcommand args and await `.output()`. The daemon auto-starts on the first session command,
/// so no explicit `daemon start` is needed.
pub fn command<R: Runtime>(app: &AppHandle<R>) -> Result<Command, String> {
    let dir = data_dir(app)?;
    Ok(app
        .shell()
        .sidecar(SIDECAR)
        .map_err(|e| e.to_string())?
        .env("MUTAGEN_DATA_DIRECTORY", dir.to_string_lossy().to_string()))
}

/// Run a Mutagen subcommand and return trimmed stdout, or the captured stderr as `Err`.
async fn run<R: Runtime>(app: &AppHandle<R>, args: &[&str]) -> Result<String, String> {
    let output = command(app)?
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Report the bundled Mutagen version. A smoke test that the sidecar is bundled, resolvable,
/// and executable on this platform.
pub async fn version<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    run(app, &["version"]).await
}

/// Construct the managed [`SyncEngine`] for Tauri state. Mirrors `default_backend` in
/// `backends/mod.rs`, but needs the [`AppHandle`] because the sidecar is resolved through it.
pub fn default_sync_engine(app: AppHandle) -> std::sync::Arc<dyn SyncEngine> {
    std::sync::Arc::new(MutagenCli { app })
}

/// Collapse one Mutagen session object (from `sync list --template='{{json .}}'`) into a
/// [`SyncStatus`]: conflicts win over problems, problems over transient states, only an idle
/// "watching" session is up to date. Key casing and the `status` type (string in recent Mutagen,
/// numeric enum in older) vary by version, so both are accepted defensively.
fn status_from_session(session: &serde_json::Value) -> SyncStatus {
    let field = |name: &str| {
        // Accept either camelCase (recent daemon JSON) or PascalCase (template field names).
        session
            .get(name)
            .or_else(|| session.get(pascal_case(name)))
    };

    let non_empty_array = |name: &str| {
        field(name)
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty())
    };

    if non_empty_array("conflicts") {
        return SyncStatus::Conflict;
    }

    let endpoint_has_problems = |endpoint: &str| {
        field(endpoint).is_some_and(|e| {
            ["scanProblems", "transitionProblems"]
                .iter()
                .any(|p| e.get(p).and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty()))
        })
    };
    if endpoint_has_problems("alpha") || endpoint_has_problems("beta") {
        return SyncStatus::Error;
    }

    match field("status") {
        Some(serde_json::Value::String(s)) => {
            let s = s.to_ascii_lowercase();
            if s.contains("halted") || s.contains("error") {
                SyncStatus::Error
            } else if s.contains("watching") {
                SyncStatus::UpToDate
            } else {
                SyncStatus::Syncing
            }
        }
        // Numeric Status enum (older Mutagen): Watching == 6; Halted-on-* == 1..=3.
        Some(serde_json::Value::Number(n)) => match n.as_u64() {
            Some(6) => SyncStatus::UpToDate,
            Some(1..=3) => SyncStatus::Error,
            _ => SyncStatus::Syncing,
        },
        _ => SyncStatus::Syncing,
    }
}

/// Extract deployment state from one Mutagen session object (from `sync list --template`): its
/// name, the host alias of its beta endpoint (the Podman connection it is deployed to), and the
/// collapsed status. Returns `None` for a session missing a name or a beta host, which cannot map
/// to a deployed sandbox. Both key casings are accepted, like [`status_from_session`].
fn parse_session(session: &serde_json::Value) -> Option<SessionInfo> {
    let name = session
        .get("name")
        .or_else(|| session.get("Name"))
        .and_then(|v| v.as_str())?
        .to_string();
    let beta = session.get("beta").or_else(|| session.get("Beta"))?;
    let host = beta
        .get("host")
        .or_else(|| beta.get("Host"))
        .and_then(|v| v.as_str())?
        .to_string();
    Some(SessionInfo {
        name,
        host,
        status: status_from_session(session),
    })
}

/// Lower-camel to PascalCase for the first character only — enough to map `conflicts` to
/// `Conflicts`, `alpha` to `Alpha`, etc.
fn pascal_case(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

#[async_trait::async_trait]
impl SyncEngine for MutagenCli {
    async fn create(&self, name: &str, alpha: &str, beta: &str) -> Result<(), String> {
        // Idempotent across restarts: drop any stale session of this name first.
        let _ = run(&self.app, &["sync", "terminate", name]).await;
        run(
            &self.app,
            &[
                "sync",
                "create",
                &format!("--name={name}"),
                "--sync-mode=two-way-resolved",
                alpha,
                beta,
            ],
        )
        .await
        .map(|_| ())
    }

    async fn pause(&self, name: &str) -> Result<(), String> {
        run(&self.app, &["sync", "pause", name]).await.map(|_| ())
    }

    async fn resume(&self, name: &str) -> Result<(), String> {
        run(&self.app, &["sync", "resume", name]).await.map(|_| ())
    }

    async fn terminate(&self, name: &str) -> Result<(), String> {
        run(&self.app, &["sync", "terminate", name]).await.map(|_| ())
    }

    async fn flush(&self, name: &str) -> Result<(), String> {
        run(&self.app, &["sync", "flush", name]).await.map(|_| ())
    }

    async fn status(&self, name: &str) -> Result<SyncStatus, String> {
        let json = run(
            &self.app,
            &["sync", "list", "--template={{json .}}", name],
        )
        .await?;
        let parsed: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        // The template renders the matched sessions as a JSON array; an unknown name yields [].
        let session = parsed
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| format!("no sync session named {name:?}"))?;
        Ok(status_from_session(session))
    }

    async fn sessions(&self) -> Result<Vec<SessionInfo>, String> {
        let json = run(&self.app, &["sync", "list", "--template={{json .}}"]).await?;
        // An empty data directory can render nothing rather than `[]`; treat that as no sessions.
        if json.is_empty() {
            return Ok(Vec::new());
        }
        let parsed: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        Ok(parsed
            .as_array()
            .map(|a| a.iter().filter_map(parse_session).collect())
            .unwrap_or_default())
    }

    async fn shutdown(&self) {
        let _ = run(&self.app, &["daemon", "stop"]).await;
    }
}
