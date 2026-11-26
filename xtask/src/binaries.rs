//! The bundled binaries (Mutagen sidecar, llama.cpp engine, Podman): pinned versions and SHA-256
//! digests, verified fetch and stage into `src-tauri/binaries`, and the manifest recording which
//! version was staged.

use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::exec::run;
use crate::paths::{binaries_dir, root_dir};

// Bumping a version constant means bumping the `*_SHA256` beside it in the same change: a fetch
// refuses any download whose digest differs.

/// Mutagen release staged as the Tauri sidecar. Pinned to match the e2e suite's Mutagen.
pub(crate) const MUTAGEN_VERSION: &str = "v0.18.1";
/// SHA-256 of `mutagen_windows_amd64_<MUTAGEN_VERSION>.zip`, from the release's published `SHA256SUMS`.
const MUTAGEN_SHA256: &str = "76f8223d5e6b607efdd9516473669ae5492e4f142887352d59bc6934d1f07a2d";

/// llama.cpp release whose prebuilt Windows Vulkan binaries are staged as the bundled inference
/// engine (`src-tauri/binaries/llama`). Pinned so every checkout stages the same `llama-server`.
pub(crate) const LLAMA_VERSION: &str = "b9835";
/// SHA-256 of `llama-<LLAMA_VERSION>-bin-win-vulkan-x64.zip`, from the release asset's `digest` in
/// the GitHub releases API (llama.cpp publishes no checksum file).
const LLAMA_SHA256: &str = "3aaa9403ade87bc12519642139b7fcad9549f3fe045f6e0424c7b4640b49b386";

/// Podman release whose Windows remote-client archive is staged as the bundled container engine
/// (`src-tauri/binaries/podman`). Pinned so every checkout stages the same `podman`/`gvproxy`/
/// `win-sshproxy` that `mio_process::podman::podman_command()` resolves at runtime.
pub(crate) const PODMAN_VERSION: &str = "v5.6.2";
/// SHA-256 of `podman-remote-release-windows_amd64.zip`, from the release's published `shasums`.
const PODMAN_SHA256: &str = "9c86b2686dabc287af1de1b4fadc506310718bf84fccbd12f0831fc7726a7cd5";

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

/// What a `fetch_*` does about files already in `src-tauri/binaries`.
enum StagedAction {
    /// Nothing usable is staged, or what is staged is not the pinned version.
    Fetch,
    /// The recorded version is the pinned one.
    Skip,
    /// Files exist but no version was ever recorded (staged before the manifest): trust them once
    /// and record the pin, so `setup_worktree()` can link from this checkout.
    Adopt,
}

/// Decides from the files and the manifest alone. A recorded version that differs from the pin must
/// re-fetch: recording the pin over old files would make `setup_worktree()` link stale binaries.
fn staged_action(files_present: bool, recorded: Option<&str>, pinned: &str) -> StagedAction {
    match (files_present, recorded) {
        (false, _) => StagedAction::Fetch,
        (true, Some(v)) if v == pinned => StagedAction::Skip,
        (true, Some(_)) => StagedAction::Fetch,
        (true, None) => StagedAction::Adopt,
    }
}

/// Applies [`staged_action`] to `key`'s manifest entry; returns whether staging can be skipped.
fn already_staged(key: &str, pinned: &str, files_present: bool, label: &str, shown: &Path) -> bool {
    let recorded = staged_version(&binaries_dir(), key);
    match staged_action(files_present, recorded.as_deref(), pinned) {
        StagedAction::Skip => {}
        StagedAction::Adopt => record_staged_version(key, pinned),
        StagedAction::Fetch => {
            if let Some(old) = recorded.filter(|_| files_present) {
                println!("{label} staged at {old} but {pinned} is pinned; re-fetching");
            }
            return false;
        }
    }
    println!("{label} already staged: {}", shown.display());
    true
}

/// SHA-256 of everything `reader` yields, as lowercase hex.
fn sha256_hex(mut reader: impl Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Checks `file` against `expected` (lowercase hex SHA-256), deleting it on any mismatch so a bad
/// download can neither be extracted nor mistaken for a good one on the next run.
fn verify_sha256(file: &Path, expected: &str) -> Result<(), String> {
    let actual = fs::File::open(file)
        .and_then(sha256_hex)
        .map_err(|e| format!("cannot hash {}: {e}", file.display()))?;
    if actual.eq_ignore_ascii_case(expected) {
        return Ok(());
    }
    let _ = fs::remove_file(file);
    Err(format!(
        "SHA-256 mismatch for {} (download deleted)\n  expected: {expected}\n  actual:   {actual}\n\
         If the pinned version was bumped, bump its `*_SHA256` constant in xtask/src/binaries.rs too.",
        file.display()
    ))
}

/// Downloads `url` to `dest` and verifies it against `sha256` before anything extracts it.
///
/// curl ships with Windows 10/11, so the download routes through the same `run` helper as every
/// other command.
fn download_verified(url: &str, dest: &Path, sha256: &str) {
    run(root_dir(), "curl", &["-fsSL", "-o", &dest.to_string_lossy(), url]);
    if let Err(e) = verify_sha256(dest, sha256) {
        panic!("{e}");
    }
}

/// Copies `from` to `to` so that `to` only ever exists complete: the copy lands in `<to>.partial`
/// and is renamed into place, so an interrupted copy can never leave a truncated `to` that the next
/// run's [`staged_action`] adopts as staged.
///
/// Both paths are removed before writing: a hard-linked destination (from `setup_worktree()`) is
/// shared mutable state, and `fs::copy` on Windows writes into an existing destination in place
/// rather than replacing it, which would silently rewrite a sibling worktree's staged asset too
/// (docs/features/parallel-worktree-e2e/02_dev.md, Resolved Decision 11).
fn install_file(from: &Path, to: &Path) {
    let mut partial = to.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let _ = fs::remove_file(&partial);
    let _ = fs::remove_file(to);
    fs::copy(from, &partial).unwrap();
    fs::rename(&partial, to).unwrap();
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
    if already_staged("mutagen", MUTAGEN_VERSION, dest.exists() && agents_dest.exists(), "Mutagen sidecar", &dest) {
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

    let zip_arg = zip.to_string_lossy();
    let extract_arg = extract.to_string_lossy();
    println!("Downloading Mutagen {MUTAGEN_VERSION} for {triple}...");
    download_verified(&url, &zip, MUTAGEN_SHA256);
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
    install_file(&produced, &dest);
    install_file(&produced_agents, &agents_dest);
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
    // The pinned release has no arm64 Vulkan build, and `LLAMA_SHA256` covers the x64 zip only.
    if env::consts::ARCH != "x86_64" {
        panic!("unsupported arch for the pinned llama.cpp Vulkan release (x64 only): {}", env::consts::ARCH);
    }
    let dest_dir = binaries_dir().join("llama");
    let server = dest_dir.join("llama-server.exe");
    if already_staged("llama", LLAMA_VERSION, server.exists(), "llama.cpp engine", &server) {
        return;
    }

    let asset = format!("llama-{LLAMA_VERSION}-bin-win-vulkan-x64.zip");
    let url = format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{LLAMA_VERSION}/{asset}"
    );
    let zip = env::temp_dir().join(&asset);

    // Extracted beside `dest_dir` (same volume, so the final rename is atomic) rather than into it:
    // an interrupted extraction must not leave a `llama` directory that has `llama-server.exe` but
    // not every DLL, which the next run would take for a complete stage.
    let partial = binaries_dir().join("llama.partial");
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).unwrap();

    let zip_arg = zip.to_string_lossy();
    let partial_arg = partial.to_string_lossy();
    println!("Downloading llama.cpp {LLAMA_VERSION} (Vulkan, x64)...");
    download_verified(&url, &zip, LLAMA_SHA256);
    // The release zip holds its files at the archive root, so extracting straight into `partial`
    // lays the engine out flat.
    run(root_dir(), &windows_tar(), &["-xf", &zip_arg, "-C", &partial_arg]);

    assert!(
        partial.join("llama-server.exe").exists(),
        "expected llama-server.exe in extracted archive at {}",
        partial.display()
    );
    // A hard-linked destination is shared mutable state: remove it before replacing it so a re-fetch
    // cannot rewrite a sibling worktree's staged copy through the link (Resolved Decision 11, as above).
    let _ = fs::remove_dir_all(&dest_dir);
    fs::rename(&partial, &dest_dir).unwrap();
    let _ = fs::remove_file(&zip);
    record_staged_version("llama", LLAMA_VERSION);
    println!("Staged llama.cpp engine: {}", dest_dir.display());
}

/// The files [`fetch_podman`] stages into `src-tauri/binaries/podman`.
const PODMAN_FILES: [&str; 3] = ["podman.exe", "gvproxy.exe", "win-sshproxy.exe"];

/// Download the official Podman Windows remote-client archive and stage `podman.exe`,
/// `gvproxy.exe`, and `win-sshproxy.exe` flat into `src-tauri/binaries/podman` (see
/// `src-tauri/binaries/podman/README.md`). `mio_process::podman::podman_command()` resolves these
/// from the bundled resource dir at runtime instead of PATH.
pub(crate) fn fetch_podman() {
    if !cfg!(windows) {
        todo!("Podman fetch is implemented for Windows only");
    }
    let dest_dir = binaries_dir().join("podman");
    let podman_exe = dest_dir.join("podman.exe");
    let files_present = PODMAN_FILES.iter().all(|name| dest_dir.join(name).exists());
    if already_staged("podman", PODMAN_VERSION, files_present, "Podman", &podman_exe) {
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
    download_verified(&url, &zip, PODMAN_SHA256);
    run(root_dir(), &windows_tar(), &["-xf", &zip_arg, "-C", &extract_arg]);

    // The archive nests these three under varying subdirectories across releases (e.g.
    // `usr/bin/`); search recursively rather than hardcode a path.
    for name in PODMAN_FILES {
        let found = find_file(&extract, name).unwrap_or_else(|| {
            panic!("expected {} in extracted Podman archive at {}", name, extract.display())
        });
        install_file(&found, &dest_dir.join(name));
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
