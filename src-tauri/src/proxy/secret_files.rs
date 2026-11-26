//! The secret files of the sidecar's `secrets/` directory on the laptop: staging, the rollback
//! snapshot, and the stale sweep.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::types::ProxyError;
use super::{io_err, iron_config, write_atomic};
use mio_net_policy::schema::{NetPolicy, SecretBinding};

/// Writes one file per binding whose secret has a stored value into `dir`, holding the value's
/// exact bytes: a `file` source sends the content verbatim, so a trailing newline would reach the
/// upstream (Resolved Decision 15). Returns the variables written and the variables left out.
pub(super) fn materialise_secret_files(
    dir: &Path,
    policy: &NetPolicy,
    values: &HashMap<String, String>,
) -> Result<(Vec<String>, Vec<String>), ProxyError> {
    std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let mut written = Vec::new();
    let mut missing = Vec::new();
    for binding in &policy.secrets {
        if has_value(binding, values) {
            write_atomic(&dir.join(&binding.env_var), values[&binding.secret_id].as_bytes())?;
            written.push(binding.env_var.clone());
        } else {
            missing.push(binding.env_var.clone());
        }
    }
    Ok((written, missing))
}

/// Whether `binding` has a stored value to write. The one filter behind both the files written to
/// disk and the files a render names, so the two cannot drift.
fn has_value(binding: &SecretBinding, values: &HashMap<String, String>) -> bool {
    values.contains_key(&binding.secret_id)
}

/// The file names an apply leaves in the secrets directory and renders into the config: one per
/// workspace binding with a value, then the GitHub token file when `github`. Pure, so read-only
/// paths can ask what a write would produce.
pub(super) fn planned_files(policy: &NetPolicy, values: &HashMap<String, String>, github: bool) -> Vec<String> {
    let mut files: Vec<String> = policy
        .secrets
        .iter()
        .filter(|b| has_value(b, values))
        .map(|b| b.env_var.clone())
        .collect();
    if github {
        files.push(iron_config::GITHUB_BINDING_FILE.to_string());
    }
    files
}

/// Writes the secret files and the GitHub token file (deleted when `token` is `None`). Returns the
/// files now in place (`planned_files`) and the variables left out. A failure may leave files half
/// written, so callers restore from a snapshot taken beforehand.
pub(super) fn stage_files(
    dir: &Path,
    policy: &NetPolicy,
    values: &HashMap<String, String>,
    token: Option<&str>,
) -> Result<(Vec<String>, Vec<String>), ProxyError> {
    let (_, missing) = materialise_secret_files(dir, policy, values)?;
    stage_github_file(dir, token)?;
    Ok((planned_files(policy, values, token.is_some()), missing))
}

/// Deletes secret files no rendered binding names, so an unbound value does not stay on disk.
/// Runs only after a successful reload: the previous pipeline may still read them until then.
pub(super) fn remove_stale_secret_files(dir: &Path, keep: &[String]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !keep.contains(&name) {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                tracing::warn!(target: "proxy", file = %name, error = %e, "failed to remove a stale secret file");
            }
        }
    }
}

/// The secret files in `dir` with their bytes, taken before an apply writes any, so a failed apply
/// can put back exactly what the running pipeline was started or reloaded with.
pub(super) fn snapshot_secret_files(dir: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, ProxyError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_err(dir, e)),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| io_err(dir, e))?.path();
        let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
        files.push((path, bytes));
    }
    Ok(files)
}

/// Rolls `dir` back to `snapshot`: removes files the failed apply added and rewrites the rest.
/// `skip_github` leaves the GitHub token file as the apply left it (deleted), so a removal is never
/// undone (`docs/features/proxy-credential-secrets/02_dev.md`, Resolved Decision 12).
pub(super) fn restore_secret_files(dir: &Path, snapshot: &[(PathBuf, Vec<u8>)], skip_github: bool) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for path in entries.flatten().map(|e| e.path()) {
            if !snapshot.iter().any(|(p, _)| *p == path) {
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::error!(target: "proxy", file = %path.display(), error = %e, "failed to remove an unapplied secret file");
                }
            }
        }
    }
    for (path, bytes) in snapshot {
        if skip_github && path.file_name().is_some_and(|n| n == iron_config::GITHUB_BINDING_FILE) {
            continue;
        }
        if let Err(e) = write_atomic(path, bytes) {
            tracing::error!(target: "proxy", file = %path.display(), error = %e, "failed to restore a secret file");
        }
    }
}

/// Writes the GitHub token file with the token's exact bytes, or deletes it when `token` is `None`.
pub(super) fn stage_github_file(dir: &Path, token: Option<&str>) -> Result<(), ProxyError> {
    let path = dir.join(iron_config::GITHUB_BINDING_FILE);
    match token {
        Some(token) => {
            std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
            write_atomic(&path, token.as_bytes())
        }
        None => match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_err(&path, e)),
        },
    }
}

/// Deletes the GitHub token file when it exists.
pub(super) fn remove_github_file(secrets: &Path) -> Result<(), ProxyError> {
    let path = secrets.join(iron_config::GITHUB_BINDING_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err(&path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_github_file_deletes_a_stale_token_and_tolerates_none() {
        let dir = std::env::temp_dir().join(format!("mio-rmgh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mio-github"), b"ghp_stale").unwrap();
        remove_github_file(&dir).unwrap();
        assert!(!dir.join("mio-github").exists());
        remove_github_file(&dir).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A trailing newline in a secret file would be sent upstream inside the credential.
    #[test]
    fn secret_files_hold_exact_bytes_and_skip_missing_values() {
        let dir = std::env::temp_dir().join(format!("mio-iron-test-{}", uuid::Uuid::new_v4()));
        let policy: NetPolicy = serde_json::from_str(
            r#"{"secrets":[{"secretId":"a","envVar":"A_KEY","hosts":["a.example.com"]},
                           {"secretId":"b","envVar":"B_KEY","hosts":["b.example.com"]}]}"#,
        )
        .unwrap();
        let values = HashMap::from([("a".to_string(), "sk-123".to_string())]);
        let (written, missing) = materialise_secret_files(&dir, &policy, &values).unwrap();
        assert_eq!(std::fs::read(dir.join("A_KEY")).unwrap(), b"sk-123");
        assert_eq!(written, ["A_KEY"]);
        assert_eq!(missing, ["B_KEY"]);
        assert!(!dir.join("B_KEY").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A failed apply must leave the files the running pipeline was started with.
    #[test]
    fn restore_undoes_a_rotated_value_and_an_added_binding() {
        let dir = std::env::temp_dir().join(format!("mio-iron-test-{}", uuid::Uuid::new_v4()));
        let policy: NetPolicy = serde_json::from_str(
            r#"{"secrets":[{"secretId":"a","envVar":"A_KEY","hosts":["a.example.com"]},
                           {"secretId":"b","envVar":"B_KEY","hosts":["b.example.com"]}]}"#,
        )
        .unwrap();
        let old = HashMap::from([("a".to_string(), "old".to_string())]);
        materialise_secret_files(&dir, &policy, &old).unwrap();
        let snapshot = snapshot_secret_files(&dir).unwrap();
        let new = HashMap::from([("a".to_string(), "new".to_string()), ("b".to_string(), "b".to_string())]);
        materialise_secret_files(&dir, &policy, &new).unwrap();
        restore_secret_files(&dir, &snapshot, false);
        assert_eq!(std::fs::read(dir.join("A_KEY")).unwrap(), b"old");
        assert!(!dir.join("B_KEY").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_github_file_holds_exact_bytes_and_survives_the_stale_sweep() {
        let dir = std::env::temp_dir().join(format!("mio-iron-test-{}", uuid::Uuid::new_v4()));
        stage_github_file(&dir, Some("ghp_realtoken")).unwrap();
        std::fs::write(dir.join("STALE_KEY"), b"x").unwrap();
        remove_stale_secret_files(&dir, &[iron_config::GITHUB_BINDING_FILE.to_string()]);
        assert_eq!(std::fs::read(dir.join("mio-github")).unwrap(), b"ghp_realtoken");
        assert!(!dir.join("STALE_KEY").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A removal must not be undone by the rollback of a failed reload.
    #[test]
    fn a_removed_github_file_stays_deleted_after_a_failed_apply() {
        let dir = std::env::temp_dir().join(format!("mio-iron-test-{}", uuid::Uuid::new_v4()));
        stage_github_file(&dir, Some("ghp_realtoken")).unwrap();
        let snapshot = snapshot_secret_files(&dir).unwrap();
        stage_github_file(&dir, None).unwrap();
        assert!(!dir.join("mio-github").exists());
        restore_secret_files(&dir, &snapshot, true);
        assert!(!dir.join("mio-github").exists());
        restore_secret_files(&dir, &snapshot, false);
        assert!(dir.join("mio-github").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
