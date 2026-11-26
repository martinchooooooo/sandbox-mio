//! The bundled binaries (Mutagen sidecar, llama.cpp engine, Podman): pinned versions, fetch and
//! stage into `src-tauri/binaries`, and the manifest recording which version was staged.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::exec::run;
use crate::paths::{binaries_dir, root_dir};

/// Mutagen release staged as the Tauri sidecar. Pinned to match the e2e suite's Mutagen.
pub(crate) const MUTAGEN_VERSION: &str = "v0.18.1";

/// llama.cpp release whose prebuilt Windows Vulkan binaries are staged as the bundled inference
/// engine (`src-tauri/binaries/llama`). Pinned so every checkout stages the same `llama-server`.
pub(crate) const LLAMA_VERSION: &str = "b9835";

/// Podman release whose Windows remote-client archive is staged as the bundled container engine
/// (`src-tauri/binaries/podman`). Pinned so every checkout stages the same `podman`/`gvproxy`/
/// `win-sshproxy` that `process::podman::podman_command()` resolves at runtime.
pub(crate) const PODMAN_VERSION: &str = "v5.6.2";

/// Absolute path to the bsdtar that ships in `System32`. A bare `tar` on PATH may resolve to msys
/// or GNU tar (e.g. from Git for Windows), which cannot extract zip archives and reads `C:\...`
/// paths as a remote `host:path` (`tar: Cannot connect to C: resolve failed`). bsdtar handles both.
fn windows_tar() -> String {
    let root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    format!("{root}\\System32\\tar.exe")
}

/// Records which pinned version each `fetch_*` last staged into `src-tauri/binaries`, so
/// `setup_worktree()` can tell whether a sibling's binaries match *this* checkout's constants before
/// hard-linking them (a sibling worktree sits on its own commit; the file merely existing says
/// nothing about its version).
const STAGED_VERSIONS: &str = ".staged-versions.json";

/// The version `key` was staged at in `binaries`, or `None` when unrecorded — an unknown version is
/// never linkable, so the caller falls back to fetching.
///
/// Flat `{"key":"value"}` only, parsed by splitting on the quote character: every quoted token lands
/// at an odd index, so keys are at 1, 5, 9... and their values two tokens later. Version strings and
/// keys are `[A-Za-z0-9.-]`, so there is nothing to unescape.
pub(crate) fn staged_version(binaries: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(binaries.join(STAGED_VERSIONS)).ok()?;
    let tokens: Vec<&str> = text.split('"').collect();
    (1..tokens.len())
        .step_by(4)
        .find(|&i| tokens[i] == key)
        .and_then(|i| tokens.get(i + 2))
        .map(|v| v.to_string())
}

/// Upsert `key = version` into this checkout's manifest, leaving other keys intact.
fn record_staged_version(key: &str, version: &str) {
    record_staged_version_in(&binaries_dir(), key, version);
}

pub(crate) fn record_staged_version_in(binaries: &Path, key: &str, version: &str) {
    let mut entries: Vec<(String, String)> = ["mutagen", "llama", "podman"]
        .iter()
        .filter_map(|k| staged_version(binaries, k).map(|v| (k.to_string(), v)))
        .collect();
    match entries.iter_mut().find(|(k, _)| k == key) {
        Some(entry) => entry.1 = version.to_string(),
        None => entries.push((key.to_string(), version.to_string())),
    }
    let body = entries
        .iter()
        .map(|(k, v)| format!("\"{k}\":\"{v}\""))
        .collect::<Vec<_>>()
        .join(",");
    fs::create_dir_all(binaries).unwrap();
    let _ = fs::write(binaries.join(STAGED_VERSIONS), format!("{{{body}}}"));
}

/// Stages the three bundled binaries into `src-tauri/binaries` (each a no-op once present).
pub(crate) fn fetch_bundled_binaries() {
    fetch_mutagen();
    fetch_llama();
    fetch_podman();
}

pub(crate) fn fetch_mutagen() {
    if !cfg!(windows) {
        todo!("Mutagen sidecar fetch is implemented for Windows only");
    }
    let triple = format!("{}-pc-windows-msvc", env::consts::ARCH);
    let dest = binaries_dir().join(format!("mutagen-{triple}.exe"));
    let agents_dest = binaries_dir().join("mutagen-agents.tar.gz");
    if dest.exists() && agents_dest.exists() {
        // Recorded on this path too: the manifest states what this checkout's constants assert about
        // the files present, which is exactly the claim `setup_worktree()` needs to compare against.
        // Without it, a worktree staged before the manifest existed could never be linked from.
        record_staged_version("mutagen", MUTAGEN_VERSION);
        println!("Mutagen sidecar already staged: {}", dest.display());
        return;
    }
    fs::create_dir_all(binaries_dir()).unwrap();

    let asset = format!("mutagen_windows_amd64_{MUTAGEN_VERSION}.zip");
    let url = format!(
        "https://github.com/mutagen-io/mutagen/releases/download/{MUTAGEN_VERSION}/{asset}"
    );
    let zip = env::temp_dir().join(&asset);
    let extract = env::temp_dir().join(format!("mutagen-{MUTAGEN_VERSION}"));
    let _ = fs::remove_dir_all(&extract);
    fs::create_dir_all(&extract).unwrap();

    // curl and the System32 bsdtar (which extracts zip) ship with Windows 10/11, so the download
    // and extraction route through the same `run` helper as every other command.
    let zip_arg = zip.to_string_lossy();
    let extract_arg = extract.to_string_lossy();
    println!("Downloading Mutagen {MUTAGEN_VERSION} for {triple}...");
    run(root_dir(), "curl", &["-fsSL", "-o", &zip_arg, &url]);
    run(root_dir(), &windows_tar(), &["-xf", &zip_arg, "-C", &extract_arg]);

    // The archive holds `mutagen.exe` plus `mutagen-agents.tar.gz`. Mutagen does not embed remote
    // agents in the exe (it's a 13 MB binary; the agents tarball alone is ~90 MB) — it looks for
    // the tarball next to its own exe at runtime, so both files must ship together.
    let produced = extract.join("mutagen.exe");
    let produced_agents = extract.join("mutagen-agents.tar.gz");
    assert!(
        produced.exists(),
        "expected mutagen.exe in extracted archive at {}",
        produced.display()
    );
    assert!(
        produced_agents.exists(),
        "expected mutagen-agents.tar.gz in extracted archive at {}",
        produced_agents.display()
    );
    // Remove an existing destination before writing: a hard-linked destination (from
    // `setup_worktree()`) is shared mutable state, and `fs::copy` on Windows writes into an
    // existing destination in place rather than replacing it, which would silently rewrite a
    // sibling worktree's staged asset too (docs/features/parallel-worktree-e2e/02_dev.md, Resolved
    // Decision 11).
    let _ = fs::remove_file(&dest);
    let _ = fs::remove_file(&agents_dest);
    fs::copy(&produced, &dest).unwrap();
    fs::copy(&produced_agents, &agents_dest).unwrap();
    let _ = fs::remove_dir_all(&extract);
    let _ = fs::remove_file(&zip);
    record_staged_version("mutagen", MUTAGEN_VERSION);
    println!("Staged Mutagen sidecar: {}", dest.display());
    println!("Staged Mutagen agent bundle: {}", agents_dest.display());
}

/// Download the prebuilt llama.cpp Vulkan release and extract its **entire** contents into
/// `src-tauri/binaries/llama` (`llama-server.exe` plus ~50 DLLs incl. the ~48 MB `ggml-vulkan.dll`).
/// The whole directory ships so the DLLs resolve at runtime; see `src-tauri/binaries/llama/README.md`.
pub(crate) fn fetch_llama() {
    if !cfg!(windows) {
        todo!("llama.cpp Vulkan engine fetch is implemented for Windows only");
    }
    let arch = match env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => panic!("unsupported arch for llama.cpp Vulkan release: {}", other),
    };
    let dest_dir = binaries_dir().join("llama");
    let server = dest_dir.join("llama-server.exe");
    if server.exists() {
        record_staged_version("llama", LLAMA_VERSION);
        println!("llama.cpp engine already staged: {}", server.display());
        return;
    }
    // A hard-linked destination is shared mutable state, and this extracts straight into
    // `dest_dir`, overwriting in place — remove it first so a re-fetch cannot rewrite a sibling
    // worktree's staged copy through the link (Resolved Decision 11, as above).
    let _ = fs::remove_dir_all(&dest_dir);
    fs::create_dir_all(&dest_dir).unwrap();

    let asset = format!("llama-{LLAMA_VERSION}-bin-win-vulkan-{arch}.zip");
    let url = format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{LLAMA_VERSION}/{asset}"
    );
    let zip = env::temp_dir().join(&asset);

    // curl and the System32 bsdtar (which extracts zip) ship with Windows 10/11. The release zip
    // holds its files at the archive root, so extracting straight into `dest_dir` lays the engine
    // out flat.
    let zip_arg = zip.to_string_lossy();
    let dest_arg = dest_dir.to_string_lossy();
    println!("Downloading llama.cpp {LLAMA_VERSION} (Vulkan, {arch})...");
    run(root_dir(), "curl", &["-fsSL", "-o", &zip_arg, &url]);
    run(root_dir(), &windows_tar(), &["-xf", &zip_arg, "-C", &dest_arg]);

    assert!(
        server.exists(),
        "expected llama-server.exe in extracted archive at {}",
        server.display()
    );
    let _ = fs::remove_file(&zip);
    record_staged_version("llama", LLAMA_VERSION);
    println!("Staged llama.cpp engine: {}", dest_dir.display());
}

/// Download the official Podman Windows remote-client archive and stage `podman.exe`,
/// `gvproxy.exe`, and `win-sshproxy.exe` flat into `src-tauri/binaries/podman` (see
/// `src-tauri/binaries/podman/README.md`). `process::podman::podman_command()` resolves these from the
/// bundled resource dir at runtime instead of PATH.
pub(crate) fn fetch_podman() {
    if !cfg!(windows) {
        todo!("Podman fetch is implemented for Windows only");
    }
    let dest_dir = binaries_dir().join("podman");
    let podman_exe = dest_dir.join("podman.exe");
    if podman_exe.exists() {
        record_staged_version("podman", PODMAN_VERSION);
        println!("Podman already staged: {}", podman_exe.display());
        return;
    }
    fs::create_dir_all(&dest_dir).unwrap();

    let asset = "podman-remote-release-windows_amd64.zip";
    let url = format!(
        "https://github.com/containers/podman/releases/download/{PODMAN_VERSION}/{asset}"
    );
    let zip = env::temp_dir().join(asset);
    let extract = env::temp_dir().join(format!("podman-{PODMAN_VERSION}"));
    let _ = fs::remove_dir_all(&extract);
    fs::create_dir_all(&extract).unwrap();

    let zip_arg = zip.to_string_lossy();
    let extract_arg = extract.to_string_lossy();
    println!("Downloading Podman {PODMAN_VERSION} for Windows...");
    run(root_dir(), "curl", &["-fsSL", "-o", &zip_arg, &url]);
    run(root_dir(), &windows_tar(), &["-xf", &zip_arg, "-C", &extract_arg]);

    // The archive nests these three under varying subdirectories across releases (e.g.
    // `usr/bin/`); search recursively rather than hardcode a path.
    for name in ["podman.exe", "gvproxy.exe", "win-sshproxy.exe"] {
        let found = find_file(&extract, name).unwrap_or_else(|| {
            panic!("expected {} in extracted Podman archive at {}", name, extract.display())
        });
        // See the Mutagen fetch above: remove a possibly hard-linked destination before writing.
        let _ = fs::remove_file(dest_dir.join(name));
        fs::copy(&found, dest_dir.join(name)).unwrap();
    }
    let _ = fs::remove_dir_all(&extract);
    let _ = fs::remove_file(&zip);
    record_staged_version("podman", PODMAN_VERSION);
    println!("Staged Podman: {}", dest_dir.display());
}

/// Recursively search `dir` for a file named `name`, returning its path if found.
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if entry.file_type().ok()?.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        } else if path.file_name().map(|f| f == name).unwrap_or(false) {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_versions_round_trip_and_keep_other_keys() {
        let dir = env::temp_dir().join(format!("xtask-staged-versions-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(staged_version(&dir, "mutagen"), None);
        record_staged_version_in(&dir, "mutagen", "v0.18.1");
        record_staged_version_in(&dir, "llama", "b9835");
        record_staged_version_in(&dir, "mutagen", "v0.18.2");
        assert_eq!(staged_version(&dir, "mutagen").as_deref(), Some("v0.18.2"));
        assert_eq!(staged_version(&dir, "llama").as_deref(), Some("b9835"));
        assert_eq!(staged_version(&dir, "podman"), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
