//! App-managed OpenSSH config for Mutagen transport via Host aliases and Include insertion.

pub mod types;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use types::{SshAlias, SshError};

const INCLUDE_LINE: &str = "Include sandbox-mio/config";

static CONFIG_LOCK: Mutex<()> = Mutex::new(());

fn ssh_dir() -> Result<PathBuf, SshError> {
    let home = dirs::home_dir().ok_or(SshError::HomeDirNotFound)?;
    Ok(home.join(".ssh"))
}

fn user_config() -> Result<PathBuf, SshError> {
    Ok(ssh_dir()?.join("config"))
}

fn managed_config() -> Result<PathBuf, SshError> {
    Ok(ssh_dir()?.join("sandbox-mio").join("config"))
}

fn markers(alias: &str) -> (String, String) {
    (
        format!("# >>> sandbox-mio {alias} >>>"),
        format!("# <<< sandbox-mio {alias} <<<"),
    )
}

fn strip_block(text: &str, alias: &str) -> String {
    let (begin, end) = markers(alias);
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == begin {
            inside = true;
            continue;
        }
        if trimmed == end {
            inside = false;
            continue;
        }
        if !inside {
            out.push(line);
        }
    }
    out.join("\n")
}

// Writes via temp file and atomic rename so reader never sees half-written file.
fn atomic_write(path: &std::path::Path, contents: &str) -> Result<(), SshError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| SshError::WriteError(format!("failed to create {parent:?}: {e}")))?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| SshError::WriteError(e.to_string()))?;
        f.write_all(contents.as_bytes()).map_err(|e| SshError::WriteError(e.to_string()))?;
        f.sync_all().map_err(|e| SshError::WriteError(e.to_string()))?;
    }
    // std::fs::rename replaces an existing destination on both Unix and Windows.
    std::fs::rename(&tmp, path)
        .map_err(|e| SshError::WriteError(format!("failed to replace {path:?}: {e}")))
}

/// Render the managed `Host` block for `alias`.
fn render_block(a: &SshAlias) -> String {
    let (begin, end) = markers(&a.alias);
    [
        begin.as_str(),
        &format!("Host {}", a.alias),
        &format!("    HostName {}", a.host),
        &format!("    Port {}", a.port),
        &format!("    User {}", a.user),
        &format!("    IdentityFile {}", a.identity),
        "    IdentitiesOnly yes",
        "    StrictHostKeyChecking accept-new",
        end.as_str(),
    ]
    .join("\n")
}

/// Insert the `Include` at the top of `~/.ssh/config` exactly once; a no-op if already present.
/// Prepended because OpenSSH applies the first matching value for most keywords.
pub fn ensure_include() -> Result<(), SshError> {
    let _guard = CONFIG_LOCK.lock().map_err(|_| SshError::LockPoisoned)?;
    let path = user_config()?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == INCLUDE_LINE) {
        return Ok(());
    }
    let updated = if existing.trim().is_empty() {
        format!("{INCLUDE_LINE}\n")
    } else {
        format!("{INCLUDE_LINE}\n\n{}\n", existing.trim_end())
    };
    atomic_write(&path, &updated)
}

/// Write or replace the managed `Host` alias and ensure the user's config includes the managed
/// file. Safe to call on every deploy — the block is keyed by alias, so re-writing the same alias
/// just refreshes the live port and identity.
pub fn write_alias(alias: &SshAlias) -> Result<(), SshError> {
    let path = managed_config()?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let base = strip_block(&existing, &alias.alias);
    let base = base.trim_end();
    let block = render_block(alias);
    let updated = if base.is_empty() {
        format!("{block}\n")
    } else {
        format!("{base}\n{block}\n")
    };
    atomic_write(&path, &updated)?;
    ensure_include()
}

/// Strip the managed `Host` alias from the app-owned config. A no-op if the file or block is
/// absent. The `Include` line in `~/.ssh/config` is left in place.
// Counterpart to `write_alias`, called when a Podman host is removed; no host-removal flow is
// wired yet, so it has no caller in this crate.
#[allow(dead_code)]
pub fn remove_alias(alias: &str) -> Result<(), SshError> {
    let path = managed_config()?;
    let Ok(existing) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let stripped = strip_block(&existing, alias);
    let stripped = stripped.trim_end();
    let contents = if stripped.is_empty() {
        String::new()
    } else {
        format!("{stripped}\n")
    };
    atomic_write(&path, &contents)
}
