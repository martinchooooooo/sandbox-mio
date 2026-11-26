//! App-managed OpenSSH config for Mutagen transport via Host aliases and Include insertion.

pub mod identity;
pub mod types;

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use types::{SshAlias, SshError};

const INCLUDE_LINE: &str = "Include sandbox-mio/config";

// Serializes same-process callers cheaply before the cross-process file lock below; a `Mutex`
// alone cannot cover the e2e harness's app-process-boundary race, since two OS processes have
// independent `Mutex` instances (`docs/features/microsandbox-provider/04_improvements.md`,
// "ssh::write_alias does a read-modify-write with no lock").
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

/// The SSH directory the app reads and writes: `~/.ssh`, or `MIO_SSH_DIR` when set. The override
/// moves the user config, the managed config and the lock together so reads and writes always
/// agree; only the e2e harness sets it (`e2e/wdio.conf.ts`).
fn ssh_dir() -> Result<PathBuf, SshError> {
    if let Some(dir) = mio_env::env_dir("MIO_SSH_DIR") {
        return Ok(dir);
    }
    let home = dirs::home_dir().ok_or(SshError::HomeDirNotFound)?;
    Ok(home.join(".ssh"))
}

fn user_config() -> Result<PathBuf, SshError> {
    Ok(ssh_dir()?.join("config"))
}

fn managed_config() -> Result<PathBuf, SshError> {
    Ok(ssh_dir()?.join("sandbox-mio").join("config"))
}

/// A stable sibling path to `managed_config()`, held via `File::lock` (`flock`/`LockFileEx`
/// underneath) across the whole read-modify-write sequence. Deliberately never the config file
/// itself: `atomic_write` replaces that file's inode via `rename`, which would strand a lock taken
/// on the pre-rename inode.
fn lock_path() -> Result<PathBuf, SshError> {
    Ok(ssh_dir()?.join("sandbox-mio").join(".config.lock"))
}

/// Hold both the in-process `Mutex` and a cross-process advisory file lock for the duration of
/// `f`, so two Tauri app processes racing at an e2e session boundary (or any two processes on this
/// machine) serialize their read-modify-write of the managed config exactly like two threads in
/// one process would. Internal helpers that compose under an outer call (`write_alias` calling
/// `ensure_include`) must call the `_locked` variant directly instead of the public wrapper, or the
/// second `File::lock` on the same handle would deadlock.
fn with_config_lock<T>(f: impl FnOnce() -> Result<T, SshError>) -> Result<T, SshError> {
    let _guard = CONFIG_LOCK.lock().map_err(|_| SshError::LockPoisoned)?;
    let path = lock_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| SshError::WriteError(format!("failed to create {parent:?}: {e}")))?;
    }
    let file = File::create(&path).map_err(|e| SshError::WriteError(e.to_string()))?;
    file.lock().map_err(|e| SshError::WriteError(e.to_string()))?;
    let result = f();
    let _ = file.unlock();
    result
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

/// An OpenSSH config argument in double quotes, so a path under a home directory containing a space
/// stays one argument. Deliberately unescaped: the Windows 10 inbox client's older parser has no escapes.
fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

/// Render the managed `Host` block for `alias`. `Direct` and `Proxied` emit mutually exclusive
/// OpenSSH stanzas so a caller can never populate both `HostName`/`Port` and `ProxyCommand`.
fn render_block(a: &SshAlias) -> String {
    let (begin, end) = markers(a.alias());
    let body: Vec<String> = match a {
        SshAlias::Direct { alias, host, port, user, identity } => vec![
            format!("Host {alias}"),
            format!("    HostName {host}"),
            format!("    Port {port}"),
            format!("    User {user}"),
            format!("    IdentityFile {}", quoted(identity)),
            "    IdentitiesOnly yes".to_string(),
            "    StrictHostKeyChecking accept-new".to_string(),
        ],
        SshAlias::Proxied { alias, command, user, identity, known_hosts } => vec![
            format!("Host {alias}"),
            // Never wrapped in `quoted`: OpenSSH takes the rest of the line unparsed, so the command
            // carries its own quoting.
            format!("    ProxyCommand {command}"),
            format!("    User {user}"),
            format!("    IdentityFile {}", quoted(identity)),
            format!("    UserKnownHostsFile {}", quoted(known_hosts)),
            "    IdentitiesOnly yes".to_string(),
            "    StrictHostKeyChecking accept-new".to_string(),
            // Mirrors the image's `ClientAlive*` (`src-tauri/resources/Containerfile`): the exec pipe
            // never reports a dead peer, so both ends rely on probes and give up after about 45 s.
            "    ServerAliveInterval 15".to_string(),
            "    ServerAliveCountMax 3".to_string(),
        ],
    };
    std::iter::once(begin)
        .chain(body)
        .chain(std::iter::once(end))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Insert the `Include` at the top of `~/.ssh/config` exactly once; a no-op if already present.
/// Prepended because OpenSSH applies the first matching value for most keywords.
fn ensure_include_locked() -> Result<(), SshError> {
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

// `write_alias` now calls `ensure_include_locked()` directly (see its own lock), so this
// standalone entry point has no in-crate caller; kept public for a future direct call, matching
// the `remove_alias` precedent below.
#[allow(dead_code)]
pub fn ensure_include() -> Result<(), SshError> {
    with_config_lock(ensure_include_locked)
}

/// Write or replace the managed `Host` alias and ensure the user's config includes the managed
/// file. Safe to call on every deploy — the block is keyed by alias, so re-writing the same alias
/// just refreshes the live port and identity. The read-strip-append-write plus the `Include` check
/// run under one `with_config_lock` acquisition, so a second writer never observes (or clobbers)
/// a partial result.
pub fn write_alias(alias: &SshAlias) -> Result<(), SshError> {
    with_config_lock(|| {
        let path = managed_config()?;
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let base = strip_block(&existing, alias.alias());
        let base = base.trim_end();
        let block = render_block(alias);
        let updated = if base.is_empty() {
            format!("{block}\n")
        } else {
            format!("{base}\n{block}\n")
        };
        atomic_write(&path, &updated)?;
        ensure_include_locked()
    })
}

/// Strip the managed `Host` alias from the app-owned config. A no-op if the file or block is
/// absent. The `Include` line in `~/.ssh/config` is left in place. Touches only `alias`'s own
/// marker-delimited block, so concurrent sandboxes' blocks survive.
pub fn remove_alias(alias: &str) -> Result<(), SshError> {
    with_config_lock(|| {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxied(identity: &str, known_hosts: &str) -> SshAlias {
        SshAlias::Proxied {
            alias: "box".to_string(),
            command: r#""C:/Program Files/podman.exe" --connection m exec -i box mio-sshd"#.to_string(),
            user: "root".to_string(),
            identity: identity.to_string(),
            known_hosts: known_hosts.to_string(),
        }
    }

    fn line<'a>(block: &'a str, keyword: &str) -> &'a str {
        block
            .lines()
            .map(str::trim)
            .find(|l| l.split_once(' ').is_some_and(|(k, _)| k == keyword))
            .unwrap_or_else(|| panic!("no {keyword} line in:\n{block}"))
    }

    #[test]
    fn proxied_block_carries_keepalives() {
        let block = render_block(&proxied("/k/id_ed25519", "/k/known_hosts"));
        assert_eq!(line(&block, "ServerAliveInterval"), "ServerAliveInterval 15");
        assert_eq!(line(&block, "ServerAliveCountMax"), "ServerAliveCountMax 3");
    }

    #[test]
    fn proxied_block_quotes_space_bearing_paths() {
        let block = render_block(&proxied(
            "C:/Users/John Smith/AppData/ssh-keys/box/id_ed25519",
            "C:/Users/John Smith/AppData/ssh-keys/box/known_hosts",
        ));
        assert_eq!(
            line(&block, "IdentityFile"),
            r#"IdentityFile "C:/Users/John Smith/AppData/ssh-keys/box/id_ed25519""#
        );
        assert_eq!(
            line(&block, "UserKnownHostsFile"),
            r#"UserKnownHostsFile "C:/Users/John Smith/AppData/ssh-keys/box/known_hosts""#
        );
        assert_eq!(
            line(&block, "ProxyCommand"),
            r#"ProxyCommand "C:/Program Files/podman.exe" --connection m exec -i box mio-sshd"#
        );
    }

    #[test]
    fn direct_block_quotes_identity_without_keepalives() {
        let block = render_block(&SshAlias::Direct {
            alias: "remote".to_string(),
            host: "10.0.0.1".to_string(),
            port: "22".to_string(),
            user: "core".to_string(),
            identity: r"C:\Users\John Smith\.ssh\podman".to_string(),
        });
        assert_eq!(line(&block, "IdentityFile"), r#"IdentityFile "C:\Users\John Smith\.ssh\podman""#);
        assert!(!block.contains("ServerAlive"), "{block}");
    }

    #[test]
    fn quoted_block_is_still_stripped_by_its_markers() {
        let block = render_block(&proxied("C:/a b/id", "C:/a b/known_hosts"));
        let text = format!("keep\n{block}\nalso keep\n");
        assert_eq!(strip_block(&text, "box"), "keep\nalso keep");
    }
}
