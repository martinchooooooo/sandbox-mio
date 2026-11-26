//! The per-sandbox JSON-lines history files under `paths::resource_history_dir`: append, prune
//! and the windowed read behind the history command.

use std::io::Write;
use std::path::{Path, PathBuf};

use tauri::AppHandle;

use super::rollup::{PoolRollup, Rollup};
use super::unix_now;
use mio_domain::types::BackendError;

const RETENTION_SECS: u64 = 14 * 24 * 3600;

/// A history file is pruned at monitor start and whenever it has grown this much since.
pub(super) const PRUNE_GROWTH_BYTES: u64 = 512 * 1024;

/// The Podman machine's history file stem. A leading underscore cannot collide with a sandbox,
/// whose name starts with an ASCII alphanumeric (`validate_name` in `src-tauri/crates/domain/src/lib.rs`).
const POOL_FILE_STEM: &str = "_podman-machine";

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryWindow {
    Day,
    Week,
}

impl HistoryWindow {
    fn secs(self) -> u64 {
        match self {
            HistoryWindow::Day => 24 * 3600,
            HistoryWindow::Week => 7 * 24 * 3600,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxHistory {
    pub name: String,
    pub rollups: Vec<Rollup>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceHistory {
    pub sandboxes: Vec<SandboxHistory>,
    pub podman_pool: Vec<PoolRollup>,
}

/// `name`'s history file under `dir`. The caller has validated `name`, which becomes a path segment.
pub(super) fn sandbox_file(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.jsonl"))
}

/// The Podman machine's history file under `dir`.
pub(super) fn pool_file(dir: &Path) -> PathBuf {
    dir.join(format!("{POOL_FILE_STEM}.jsonl"))
}

pub(super) fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

#[derive(serde::Deserialize)]
struct Stamped {
    ts: u64,
}

/// The lines of `text` stamped at or after `cutoff`; unreadable lines are dropped.
fn lines_since(text: &str, cutoff: u64) -> Vec<&str> {
    text.lines()
        .filter(|l| serde_json::from_str::<Stamped>(l).is_ok_and(|s| s.ts >= cutoff))
        .collect()
}

pub(super) fn prune_file(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let cutoff = unix_now().as_secs().saturating_sub(RETENTION_SECS);
    let kept = lines_since(&text, cutoff);
    let mut body = kept.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    let tmp = path.with_extension("jsonl.tmp");
    let written = std::fs::write(&tmp, body).and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        tracing::warn!(target: "resources", path = %path.display(), error = %e, "could not prune the resource history");
    }
}

pub(super) fn prune_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "jsonl") {
            prune_file(&path);
        }
    }
}

fn read_rollups<T: serde::de::DeserializeOwned>(path: &Path, cutoff: u64) -> std::io::Result<Vec<T>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(lines_since(&text, cutoff).into_iter().filter_map(|l| serde_json::from_str(l).ok()).collect())
}

/// The rollups of `name` over `window`, or of every sandbox and the Podman machine when `name` is
/// `None`.
pub fn history(
    app: &AppHandle,
    name: Option<&str>,
    window: HistoryWindow,
) -> Result<ResourceHistory, BackendError> {
    let dir = crate::paths::resource_history_dir(app)
        .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
    let io = |e: std::io::Error| BackendError::CommandFailed(e.to_string());
    let cutoff = unix_now().as_secs().saturating_sub(window.secs());
    let mut history = ResourceHistory::default();
    match name {
        Some(name) => {
            mio_domain::validate_name(name)?;
            let rollups = read_rollups(&sandbox_file(&dir, name), cutoff).map_err(io)?;
            history.sandboxes.push(SandboxHistory { name: name.to_string(), rollups });
        }
        None => {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(history),
                Err(e) => return Err(io(e)),
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.extension().is_some_and(|e| e == "jsonl") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
                if stem == POOL_FILE_STEM {
                    history.podman_pool = read_rollups(&path, cutoff).map_err(io)?;
                } else {
                    let rollups = read_rollups(&path, cutoff).map_err(io)?;
                    history.sandboxes.push(SandboxHistory { name: stem.to_string(), rollups });
                }
            }
            history.sandboxes.sort_by(|a, b| a.name.cmp(&b.name));
        }
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_and_unreadable_lines_are_dropped() {
        let text = "{\"ts\":5}\nnot json\n{\"ts\":20,\"x\":1}\n";
        assert_eq!(lines_since(text, 10), ["{\"ts\":20,\"x\":1}"]);
    }
}
