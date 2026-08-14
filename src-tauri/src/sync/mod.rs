//! Mutagen file-sync engine, bundled as a Tauri sidecar.
//! Isolates the app's daemon via app-private MUTAGEN_DATA_DIRECTORY.

pub mod types;

use std::sync::Arc;

use tauri::{AppHandle, Runtime};
use tauri_plugin_shell::process::Command;
use tauri_plugin_shell::ShellExt;

use crate::mockctl::types::ProcessRunner;
use types::{MutagenCli, SessionInfo, SyncEngine, SyncError, SyncStatus};

/// Sidecar name as declared in `bundle.externalBin`.
const SIDECAR: &str = "mutagen";

/// `MIO_APP_DATA_DIR` relocates the Mutagen data directory (see [`crate::paths::app_data_root`]);
/// no subsystem-specific override exists for this one.
fn data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, SyncError> {
    let dir = crate::paths::app_data_root(app)
        .map_err(|e| SyncError::DataDir(e.to_string()))?
        .join("mutagen");
    std::fs::create_dir_all(&dir)
        .map_err(|e| SyncError::DataDir(format!("failed to create mutagen data dir: {e}")))?;
    Ok(dir)
}

pub fn command<R: Runtime>(app: &AppHandle<R>) -> Result<Command, SyncError> {
    let dir = data_dir(app)?;
    Ok(app
        .shell()
        .sidecar(SIDECAR)
        .map_err(|e| SyncError::Sidecar(e.to_string()))?
        .env("MUTAGEN_DATA_DIRECTORY", dir.to_string_lossy().to_string())
        // Defense-in-depth against MSYS/Git-Bash path mangling in the `ssh` → `sh -c "exec …"`
        // transport Mutagen spawns for a `Proxied` alias. NOT load-bearing for the exec transport:
        // `backends::ensure_sandbox_alias` references `mio-sshd` by bare name precisely so it carries
        // no manglable token, because these vars only reach the daemon that spawns the transport when
        // this CLI call auto-starts it (docs/features/podman-exec-transport/02_dev.md, Resolved
        // Decision 5). Kept for any future manglable argument on the sidecar's own command line.
        .env("MSYS_NO_PATHCONV", "1")
        .env("MSYS2_ARG_CONV_EXCL", "*"))
}

/// Run a Mutagen subcommand and return trimmed stdout, or the captured stderr as `Err`. Used by the
/// `forward`/`version` free functions only; [`MutagenCli`]'s own calls go through [`run_via`] so the
/// e2e `light` tier can script them (`docs/features/scriptable-mock-backend/02_dev.md`, Phase C).
async fn run<R: Runtime>(app: &AppHandle<R>, args: &[&str]) -> Result<String, SyncError> {
    let output = command(app)?
        .args(args)
        .output()
        .await
        .map_err(|e| SyncError::Sidecar(e.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(SyncError::CommandFailed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

/// [`run`]'s contract over a [`ProcessRunner`]: identical stdout-trimming and stderr-on-failure
/// mapping, so a scripted non-zero exit produces the same `SyncError::CommandFailed` a real sidecar
/// failure does.
async fn run_via(runner: &Arc<dyn ProcessRunner>, args: &[&str]) -> Result<String, SyncError> {
    let output = runner.run(args).await.map_err(|e| {
        tracing::error!(target: "sync", argv = %args.join(" "), error = %e, "mutagen spawn failed");
        SyncError::Sidecar(e.to_string())
    })?;
    if output.status.success() {
        // Success is `debug`, not `info`: `sync list` runs on a status-poll timer, and logging every
        // poll at the default level would bury the one call that matters in `mio.log`.
        tracing::debug!(target: "sync", argv = %args.join(" "), "mutagen ok");
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        tracing::warn!(
            target: "sync",
            argv = %args.join(" "),
            code = output.status.code().unwrap_or(-1),
            %stderr,
            "mutagen exited non-zero"
        );
        Err(SyncError::CommandFailed(stderr))
    }
}

/// [`run_via`] for a call whose failure the caller deliberately discards — the "terminate any stale
/// session of this name first" step every `create` opens with, which fails by design whenever the
/// name is new. Logs at `debug` so a WARN in `mio.log` keeps meaning something actually went wrong.
async fn run_via_discarding(runner: &Arc<dyn ProcessRunner>, args: &[&str]) {
    match runner.run(args).await {
        Ok(out) if out.status.success() => {
            tracing::debug!(target: "sync", argv = %args.join(" "), "mutagen ok")
        }
        Ok(out) => tracing::debug!(
            target: "sync",
            argv = %args.join(" "),
            code = out.status.code().unwrap_or(-1),
            stderr = %String::from_utf8_lossy(&out.stderr).trim(),
            "mutagen exited non-zero (ignored by caller)"
        ),
        Err(e) => {
            tracing::debug!(target: "sync", argv = %args.join(" "), error = %e, "mutagen spawn failed (ignored by caller)")
        }
    }
}

pub async fn version<R: Runtime>(app: &AppHandle<R>) -> Result<String, SyncError> {
    run(app, &["version"]).await
}

/// Serializes every terminate-then-create pair below.
static FORWARD_CREATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub async fn forward_create<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    local_port: u16,
    remote_alias: &str,
    remote_port: u16,
) -> Result<(), SyncError> {
    let _guard = FORWARD_CREATE_LOCK.lock().await;
    // Best-effort: a missing session is not an error, so ignore the result.
    let _ = run(app, &["forward", "terminate", name]).await;
    let source = format!("tcp:127.0.0.1:{local_port}");
    let dest = format!("{remote_alias}:tcp:127.0.0.1:{remote_port}");
    run(app, &["forward", "create", &format!("--name={name}"), &source, &dest])
        .await
        .map(|_| ())
}

pub const INFERENCE_VM_PORT: u16 = 51199;

/// Reverse of `forward_create`: listen on the remote, dial the host. The `0.0.0.0` bind is
/// required — a `127.0.0.1` bind is VM-loopback-only and unreachable from container netns via
/// `host.containers.internal` (spiked 2026-06-29). Idempotent across retries.
pub async fn forward_create_reverse<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    remote_alias: &str,
    remote_listen_port: u16,
    host_port: u16,
) -> Result<(), SyncError> {
    let _guard = FORWARD_CREATE_LOCK.lock().await;
    // Best-effort: a missing session is not an error, so ignore the result.
    let _ = run(app, &["forward", "terminate", name]).await;
    let source = format!("{remote_alias}:tcp:0.0.0.0:{remote_listen_port}");
    let dest = format!("tcp:127.0.0.1:{host_port}");
    run(app, &["forward", "create", &format!("--name={name}"), &source, &dest])
        .await
        .map(|_| ())
}

/// Terminate the Mutagen forwarding session named `name`. Best-effort: a missing session is
/// not an error (desired state already reached).
pub async fn forward_terminate<R: Runtime>(app: &AppHandle<R>, name: &str) {
    let _ = run(app, &["forward", "terminate", name]).await;
}

/// Terminates sessions orphaned by a previous app run; the Mutagen daemon outlives the app.
pub async fn sweep_stale_forwards<R: Runtime>(app: &AppHandle<R>, live_sandboxes: &[String]) {
    let json = match run(app, &["forward", "list", "--template={{json .}}"]).await {
        Ok(j) if !j.is_empty() => j,
        _ => return,
    };
    let Ok(serde_json::Value::Array(sessions)) = serde_json::from_str(&json) else {
        return;
    };
    for session in &sessions {
        let name = session
            .get("name")
            .or_else(|| session.get("Name"))
            .and_then(|v| v.as_str());
        let Some(name) = name else { continue };
        let orphaned = if name.starts_with("expose-") {
            true
        } else {
            let sandbox = name
                .strip_prefix("terminal-")
                .or_else(|| name.strip_prefix("agentweb-"));
            sandbox.is_some_and(|s| !live_sandboxes.iter().any(|live| live == s))
        };
        if orphaned {
            tracing::info!(target: "sync", session = name, "sweeping stale forward");
            let _ = run(app, &["forward", "terminate", name]).await;
        }
    }
}

/// Construct the managed [`SyncEngine`] for Tauri state. Mirrors `default_backend` in
/// `backends/mod.rs`: one real struct either way, parameterized by which [`ProcessRunner`] it holds,
/// so the `light` tier exercises `MutagenCli`'s real session parsing rather than a parallel mock.
pub fn default_sync_engine(runner: Arc<dyn ProcessRunner>) -> Arc<dyn SyncEngine> {
    Arc::new(MutagenCli { runner })
}

/// Collapses Mutagen session JSON to SyncStatus; handles version-specific key casing.
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
    async fn create(&self, name: &str, alpha: &str, beta: &str) -> Result<(), SyncError> {
        // Idempotent across restarts: drop any stale session of this name first.
        run_via_discarding(&self.runner, &["sync", "terminate", name]).await;
        run_via(
            &self.runner,
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

    async fn pause(&self, name: &str) -> Result<(), SyncError> {
        run_via(&self.runner, &["sync", "pause", name]).await.map(|_| ())
    }

    async fn resume(&self, name: &str) -> Result<(), SyncError> {
        run_via(&self.runner, &["sync", "resume", name]).await.map(|_| ())
    }

    async fn terminate(&self, name: &str) -> Result<(), SyncError> {
        run_via(&self.runner, &["sync", "terminate", name]).await.map(|_| ())
    }

    async fn flush(&self, name: &str) -> Result<(), SyncError> {
        run_via(&self.runner, &["sync", "flush", name]).await.map(|_| ())
    }

    async fn status(&self, name: &str) -> Result<SyncStatus, SyncError> {
        let json = run_via(
            &self.runner,
            &["sync", "list", "--template={{json .}}", name],
        )
        .await?;
        let parsed: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| SyncError::ParseError(e.to_string()))?;
        // The template renders the matched sessions as a JSON array; an unknown name yields [].
        let session = parsed
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| SyncError::SessionNotFound { name: name.to_string() })?;
        Ok(status_from_session(session))
    }

    async fn sessions(&self) -> Result<Vec<SessionInfo>, SyncError> {
        let json = run_via(&self.runner, &["sync", "list", "--template={{json .}}"]).await?;
        // An empty data directory can render nothing rather than `[]`; treat that as no sessions.
        if json.is_empty() {
            return Ok(Vec::new());
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| SyncError::ParseError(e.to_string()))?;
        Ok(parsed
            .as_array()
            .map(|a| a.iter().filter_map(parse_session).collect())
            .unwrap_or_default())
    }

    async fn shutdown(&self) {
        run_via_discarding(&self.runner, &["daemon", "stop"]).await;
    }
}
