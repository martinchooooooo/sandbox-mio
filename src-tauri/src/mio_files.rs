//! The files the app keeps inside a workspace — mostly JSON documents under the workspace's `.mio`
//! directory. Named for that directory rather than for the workspace, to keep filesystem access
//! apart from the pane registry in `src-tauri/src/workspace/mod.rs`
//! (`docs/features/lib-decomposition/02_dev.md`, Resolved Decision 3).
//!
//! Exists so the path is formatted into the failure message in one place instead of at every call
//! site.

use std::path::{Path, PathBuf};

use crate::agents::types::AgentError;
use crate::backends::types::BackendError;

/// A failed read or write. Carries the path in its message, so a caller can wrap it in whatever
/// error type its own subsystem surfaces.
#[derive(Debug)]
pub struct FileError(String);

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<FileError> for BackendError {
    fn from(e: FileError) -> Self {
        BackendError::CommandFailed(e.0)
    }
}

impl From<FileError> for AgentError {
    fn from(e: FileError) -> Self {
        AgentError::ConfigWriteFailed(e.0)
    }
}

/// `<workspace_path>/.mio/<name>`.
pub fn mio_path(workspace_path: &str, name: &str) -> PathBuf {
    Path::new(workspace_path).join(".mio").join(name)
}

/// Reads a JSON object, treating an absent or empty file as an empty one. A file that exists but
/// does not parse is an error rather than an empty object — overwriting a user's broken config
/// silently would discard whatever they were editing.
pub fn read_object(path: &Path) -> Result<serde_json::Map<String, serde_json::Value>, FileError> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(serde_json::Map::new()),
        Ok(s) => serde_json::from_str(&s)
            .map_err(|e| FileError(format!("failed to parse existing {}: {e}", path.to_string_lossy()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Map::new()),
        Err(e) => Err(FileError(format!("failed to read {}: {e}", path.to_string_lossy()))),
    }
}

/// Reads any JSON value, or `None` when the file is absent, unreadable, or unparseable.
pub fn read_value(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Writes indented JSON, creating parent directories first. For documents a user reads or edits.
pub fn write_pretty(path: &Path, value: &impl serde::Serialize) -> Result<(), FileError> {
    let body = serde_json::to_string_pretty(value)
        .map_err(|e| FileError(format!("failed to serialize {}: {e}", path.to_string_lossy())))?;
    write_text(path, &body)
}

/// Writes compact JSON, creating parent directories first. For session descriptors the app writes
/// for itself.
pub fn write_compact(path: &Path, value: &serde_json::Value) -> Result<(), FileError> {
    write_text(path, &value.to_string())
}

/// Writes `contents`, creating parent directories first.
pub fn write_text(path: &Path, contents: &str) -> Result<(), FileError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            FileError(format!("failed to create {}: {e}", parent.to_string_lossy()))
        })?;
    }
    std::fs::write(path, contents)
        .map_err(|e| FileError(format!("failed to write {}: {e}", path.to_string_lossy())))
}
