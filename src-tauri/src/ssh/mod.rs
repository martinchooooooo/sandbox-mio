//! App-managed OpenSSH config for Mutagen's transport.
//!
//! Mutagen's scp-style `alias:path` URLs carry no port or identity, so the app writes a `Host`
//! alias per Podman host to its own `~/.ssh/sandbox-mio/config` and pulls it into the user's
//! `~/.ssh/config` via a one-time `Include`. The app rewrites its own file freely but touches the
//! user's config only for that idempotent insertion; both writes use [`atomic_write`] under
//! [`CONFIG_LOCK`]. Rust port of `writeSshAlias`/`removeSshAlias` in `e2e/lib/mutagen.js`.

pub mod types;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use types::SshAlias;

/// The one-time line inserted into `~/.ssh/config`. OpenSSH resolves a relative `Include`
/// against `~/.ssh`, so the path is given relative, not absolute.
const INCLUDE_LINE: &str = "Include sandbox-mio/config";

/// Serialises every write to the shared `~/.ssh/config`, so the one-time `Include` insertion can
/// never race another write and corrupt the user's personal configuration.
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

fn ssh_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "could not resolve the home directory".to_string())?;
    Ok(home.join(".ssh"))
}

/// The user's personal OpenSSH config, modified only by the one-time `Include` insertion.
fn user_config() -> Result<PathBuf, String> {
    Ok(ssh_dir()?.join("config"))
}

/// The app-owned config holding the managed `Host` aliases. Rewritten freely.
fn managed_config() -> Result<PathBuf, String> {
    Ok(ssh_dir()?.join("sandbox-mio").join("config"))
}

fn markers(alias: &str) -> (String, String) {
    (
        format!("# >>> sandbox-mio {alias} >>>"),
        format!("# <<< sandbox-mio {alias} <<<"),
    )
}

/// Drop the marker-delimited block for `alias` from `text`, leaving every other line intact.
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

/// Write `contents` to `path` via a temp file in the same directory and an atomic rename, so a
/// reader never observes a half-written file. Creates parent directories as needed.
fn atomic_write(path: &std::path::Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create {parent:?}: {e}"))?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(contents.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    // std::fs::rename replaces an existing destination on both Unix and Windows.
    std::fs::rename(&tmp, path).map_err(|e| format!("failed to replace {path:?}: {e}"))
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
pub fn ensure_include() -> Result<(), String> {
    let _guard = CONFIG_LOCK.lock().map_err(|e| e.to_string())?;
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
/// file. Safe to call on every session create — the block is keyed by alias, so re-writing the
/// same alias just refreshes the live port and identity.
pub fn write_alias(alias: &SshAlias) -> Result<(), String> {
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
pub fn remove_alias(alias: &str) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(name: &str) -> SshAlias {
        SshAlias {
            alias: name.to_string(),
            host: "127.0.0.1".to_string(),
            port: "2222".to_string(),
            user: "core".to_string(),
            identity: "C:/keys/id".to_string(),
        }
    }

    // Stripping one alias must leave other managed blocks and surrounding text intact, since
    // write_alias upserts by stripping then re-appending.
    #[test]
    fn strip_removes_only_the_named_block() {
        let text = format!("# user line\n{}\n{}", render_block(&alias("h1")), render_block(&alias("h2")));
        let stripped = strip_block(&text, "h1");
        assert!(stripped.contains("# user line"));
        assert!(!stripped.contains("Host h1"));
        assert!(stripped.contains("Host h2"));
    }
}
