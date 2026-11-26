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

/// A `Host` pattern in the user's own SSH config that a sandbox name would collide with.
pub struct UserHostMatch {
    /// The pattern as written in the file, original case kept.
    pub pattern: String,
    /// The file holding the `Host` line, which may be one the user config includes.
    pub path: PathBuf,
}

/// The first `Host` entry in the user's own SSH config (`~/.ssh/config` plus the files it
/// includes) that `name` would take over, because the managed include sits before every user entry
/// and OpenSSH keeps the first value it finds. Ignores the managed include and managed blocks,
/// catch-all patterns (only `*` and `?`) and `Match` blocks, and fails open: an unreadable config
/// reports no match. Backs the clash check in `services::sandbox::create` (`src-tauri/src/services/sandbox.rs`); see
/// `docs/features/host-tool-ssh-access/04_improvements.md`, "Namespaced per-sandbox alias".
pub fn user_host_matching(name: &str) -> Option<UserHostMatch> {
    match ssh_dir() {
        Ok(dir) => find_user_host_in(&dir, name),
        Err(e) => {
            tracing::warn!(target: "ssh", "skipping the SSH host clash check: {e}");
            None
        }
    }
}

const MAX_INCLUDE_DEPTH: usize = 8;

/// Takes `dir` as a parameter so unit tests can use a temp directory without setting
/// `MIO_SSH_DIR`, which would race between parallel tests.
fn find_user_host_in(dir: &std::path::Path, name: &str) -> Option<UserHostMatch> {
    let managed_dir = dir.join("sandbox-mio");
    let top = dir.join("config");
    let mut visited = std::collections::HashSet::new();
    scan_file(dir, &managed_dir, &top, name, 0, &mut visited)
}

fn scan_file(
    dir: &std::path::Path,
    managed_dir: &std::path::Path,
    path: &std::path::Path,
    name: &str,
    depth: usize,
    visited: &mut std::collections::HashSet<PathBuf>,
) -> Option<UserHostMatch> {
    if path.starts_with(managed_dir) || depth > MAX_INCLUDE_DEPTH {
        return None;
    }
    if !visited.insert(path.to_path_buf()) {
        return None;
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            if depth == 0 {
                tracing::warn!(target: "ssh", "could not read {path:?}, skipping the host clash check: {e}");
            } else {
                tracing::debug!(target: "ssh", "skipping unreadable include {path:?}: {e}");
            }
            return None;
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    let (found, includes) = scan_config(&text, name);
    if let Some(pattern) = found {
        return Some(UserHostMatch { pattern, path: path.to_path_buf() });
    }
    for arg in includes {
        for file in resolve_include(dir, &arg) {
            if let Some(m) = scan_file(dir, managed_dir, &file, name, depth + 1, visited) {
                return Some(m);
            }
        }
    }
    None
}

/// Files an `Include` argument names: `~`, absolute and `dir`-relative paths, with `*`/`?` allowed
/// only in the last component.
fn resolve_include(dir: &std::path::Path, arg: &str) -> Vec<PathBuf> {
    let expanded = if let Some(rest) = arg.strip_prefix("~/").or_else(|| arg.strip_prefix("~\\")) {
        match dirs::home_dir() {
            Some(home) => home.join(rest),
            None => return Vec::new(),
        }
    } else {
        let p = PathBuf::from(arg);
        if p.is_absolute() { p } else { dir.join(p) }
    };
    let Some(file_name) = expanded.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Vec::new();
    };
    let parent = expanded.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    if parent.to_string_lossy().chars().any(|c| c == '*' || c == '?') {
        tracing::debug!(target: "ssh", "skipping include with a wildcard directory: {arg}");
        return Vec::new();
    }
    if !file_name.contains('*') && !file_name.contains('?') {
        return vec![expanded];
    }
    let mut out: Vec<PathBuf> = match std::fs::read_dir(&parent) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            // OpenSSH's glob(3) skips dotfiles (editor swap and backup files) unless the pattern's
            // last component starts with a dot itself.
            .filter(|e| {
                let entry = e.file_name().to_string_lossy().into_owned();
                (!entry.starts_with('.') || file_name.starts_with('.')) && wildcard_match(&file_name, &entry)
            })
            .map(|e| e.path())
            .collect(),
        Err(e) => {
            tracing::debug!(target: "ssh", "cannot list {parent:?} for include {arg}: {e}");
            Vec::new()
        }
    };
    out.sort();
    out
}

/// The first reportable `Host` pattern matching `name`, plus the raw `Include` arguments in file
/// order. Skips managed-marker blocks, `Match` blocks and every other keyword.
fn scan_config(text: &str, name: &str) -> (Option<String>, Vec<String>) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut includes = Vec::new();
    let mut inside_managed = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with("# >>> sandbox-mio ") {
            inside_managed = true;
            continue;
        }
        if line.starts_with("# <<< sandbox-mio ") {
            inside_managed = false;
            continue;
        }
        if inside_managed || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let split = line.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(line.len());
        let (keyword, rest) = line.split_at(split);
        let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '=');
        if keyword.eq_ignore_ascii_case("host") {
            let patterns = split_args(rest);
            if let Some(p) = host_line_match(&patterns, name) {
                return (Some(p.to_string()), includes);
            }
        } else if keyword.eq_ignore_ascii_case("include") {
            includes.extend(split_args(rest));
        }
    }
    (None, includes)
}

/// Tokenise the arguments of a config line: double quotes group, an unquoted `#` ends the line.
fn split_args(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for c in rest.chars() {
        if in_quotes {
            if c == '"' {
                in_quotes = false;
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
            has_token = true;
        } else if c == '#' && !has_token {
            break;
        } else if c.is_whitespace() {
            if has_token {
                out.push(std::mem::take(&mut cur));
                has_token = false;
            }
        } else {
            cur.push(c);
            has_token = true;
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

/// OpenSSH `match_hostname` semantics: any matching negated pattern vetoes the whole line;
/// otherwise the first positive pattern that matches and has a literal character.
fn host_line_match<'a>(patterns: &'a [String], name: &str) -> Option<&'a str> {
    if patterns
        .iter()
        .filter_map(|p| p.strip_prefix('!'))
        .any(|neg| wildcard_match(neg, name))
    {
        return None;
    }
    patterns
        .iter()
        .filter(|p| !p.starts_with('!'))
        .find(|p| p.chars().any(|c| c != '*' && c != '?') && wildcard_match(p, name))
        .map(String::as_str)
}

/// `*` matches any run (empty included), `?` exactly one char, ASCII case-insensitive.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let t: Vec<char> = text.to_ascii_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_match(text: &str, name: &str) -> Option<String> {
        scan_config(text, name).0
    }

    #[test]
    fn literal_host_matches_case_insensitively() {
        assert_eq!(host_match("Host Work-Laptop\n", "work-laptop").as_deref(), Some("Work-Laptop"));
    }

    #[test]
    fn any_pattern_on_a_host_line_matches() {
        assert_eq!(host_match("Host alpha beta\n", "beta").as_deref(), Some("beta"));
    }

    #[test]
    fn wildcard_family_matches_but_catch_all_does_not() {
        assert_eq!(host_match("Host prod-*\n", "prod-api").as_deref(), Some("prod-*"));
        assert_eq!(host_match("Host *\n", "x"), None);
        assert_eq!(host_match("Host ?*\n", "x"), None);
        assert_eq!(host_match("Host ???\n", "abc"), None);
        assert_eq!(host_match("Host * box\n", "box").as_deref(), Some("box"));
    }

    #[test]
    fn negated_pattern_vetoes_the_line() {
        let text = "Host team-* !team-ci\n";
        assert_eq!(host_match(text, "team-web").as_deref(), Some("team-*"));
        assert_eq!(host_match(text, "team-ci"), None);
    }

    #[test]
    fn keyword_forms_and_comments() {
        for text in [
            "host=foo\n",
            "  HOST   foo  # note\n",
            "Host \"foo\"\n",
            "\u{feff}Host foo\n",
            "Host foo\r\nUser x\r\n",
        ] {
            assert_eq!(host_match(text, "foo").as_deref(), Some("foo"), "{text:?}");
        }
    }

    #[test]
    fn only_the_host_keyword_counts() {
        for text in ["HostName foo\n", "HostKeyAlias foo\n", "Match host foo\n"] {
            assert_eq!(host_match(text, "foo"), None, "{text:?}");
        }
    }

    #[test]
    fn managed_blocks_are_skipped() {
        let text = "# >>> sandbox-mio box >>>\nHost box\n# <<< sandbox-mio box <<<\n";
        assert_eq!(host_match(text, "box"), None);
    }

    #[test]
    fn includes_are_followed_except_the_managed_one() {
        let dir = std::env::temp_dir().join(format!("mio-ssh-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sandbox-mio")).unwrap();
        std::fs::create_dir_all(dir.join("conf.d")).unwrap();
        // `Include config` makes the file include itself: the cycle guard must end the scan.
        std::fs::write(dir.join("config"), "Include sandbox-mio/config\nInclude config\nInclude conf.d/*\n").unwrap();
        std::fs::write(dir.join("sandbox-mio").join("config"), "Host managed\n").unwrap();
        std::fs::write(dir.join("conf.d").join("a"), "Host inc\n").unwrap();
        std::fs::write(dir.join("conf.d").join(".a.swp"), "Host swapped\n").unwrap();
        let hit = find_user_host_in(&dir, "inc").expect("included host found");
        assert_eq!(hit.pattern, "inc");
        assert!(hit.path.ends_with("a"), "{:?}", hit.path);
        assert!(find_user_host_in(&dir, "managed").is_none());
        assert!(find_user_host_in(&dir, "swapped").is_none(), "a wildcard include skips dotfiles");
        let empty = std::env::temp_dir().join(format!("mio-ssh-test-{}", uuid::Uuid::new_v4()));
        assert!(find_user_host_in(&empty, "inc").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

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
