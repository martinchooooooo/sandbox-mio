use std::{env, fs, path::{Path, PathBuf}, process::Command};

/// Mutagen release staged as the Tauri sidecar. Pinned to match the e2e suite's Mutagen.
const MUTAGEN_VERSION: &str = "v0.18.1";

/// llama.cpp release whose prebuilt Windows Vulkan binaries are staged as the bundled inference
/// engine (`src-tauri/binaries/llama`). Pinned so every checkout stages the same `llama-server`.
const LLAMA_VERSION: &str = "b9835";

/// Podman release whose Windows remote-client archive is staged as the bundled container engine
/// (`src-tauri/binaries/podman`). Pinned so every checkout stages the same `podman`/`gvproxy`/
/// `win-sshproxy` that `backends::podman_command()` resolves at runtime.
const PODMAN_VERSION: &str = "v5.6.2";

fn e2e_dir() -> PathBuf { PathBuf::from(env::current_dir().unwrap()).join("e2e") }
fn tools_dir() -> PathBuf { PathBuf::from(env::current_dir().unwrap()).join("tools") }
fn binaries_dir() -> PathBuf { root_dir().join("src-tauri").join("binaries") }
fn root_dir() -> PathBuf { env::current_dir().unwrap() }
fn debug_dir() -> PathBuf { root_dir().join("src-tauri").join("target").join("debug") }

/// Absolute path to the bsdtar that ships in `System32`. A bare `tar` on PATH may resolve to msys
/// or GNU tar (e.g. from Git for Windows), which cannot extract zip archives and reads `C:\...`
/// paths as a remote `host:path` (`tar: Cannot connect to C: resolve failed`). bsdtar handles both.
fn windows_tar() -> String {
    let root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    format!("{root}\\System32\\tar.exe")
}


fn main() {
    #[cfg(windows)]
    win_job::setup();

    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("dev") if args.get(2).map(String::as_str) == Some("--setup") => {
            run(root_dir(), "pnpm", &["install"]);
            fetch_mutagen();
            fetch_llama();
            fetch_podman();
        }
        // The entry point for a fresh worktree, ahead of every other arm: nothing else — `dev`,
        // `e2e`, `package` — can run until this has staged the gitignored assets.
        Some("dev") if args.get(2).map(String::as_str) == Some("--setup-worktree") => {
            setup_worktree(args.get(3).cloned());
        }
        Some("dev") => {
            kill_stale_dev_processes();
            set_dev_diagnostics_env();
            stage_llama();
            stage_podman();
            stage_mutagen_agents();
            run(root_dir(), "pnpm", &["run", "tauri", "dev"]);
        }
        Some("e2e") if args.get(2).map(String::as_str) == Some("--setup") => {
            setup_e2e();
        }
        // Any further `e2e --<flag>` arm belongs above this one, which forwards its second argument
        // straight into `pnpm run test:{suite}` — an arm added below would run `pnpm run test:--<flag>`.
        Some("e2e") => {
            let mut suite = "standard";
            if args.len() > 2 {
                suite = args.get(2).map(String::as_str).unwrap();
            }

            run(e2e_dir(), "pnpm", &["run", &format!("test:{}", suite)]);
        }
        Some("package") => {
            // Bundling copies from src-tauri/binaries at build time (unlike `dev`, which needs
            // `stage_*` to mirror them into target/debug); make sure they're present first.
            fetch_mutagen();
            fetch_llama();
            fetch_podman();
            // `--no-default-features` drops the `devtools` feature (see src-tauri/Cargo.toml) so
            // the shipped binary carries no devtools code.
            run(root_dir(), "pnpm", &["tauri", "build"]);
        }
        _ => {
            eprintln!("Usage:");
            eprintln!("  build dev --setup-worktree [<path>]");
            eprintln!("                     - first command to run in a fresh worktree: stage tools/ +");
            eprintln!("                       src-tauri/binaries/ (hard-linked from a sibling worktree when");
            eprintln!("                       possible) and run `pnpm install` into <path>, defaulting to");
            eprintln!("                       the invoking worktree");
            eprintln!("  build dev          - run `pnpm run tauri dev`");
            eprintln!("  build dev --setup  - install app dependencies and stage the Mutagen sidecar + llama.cpp engine + Podman");
            eprintln!("  build e2e          - run `pnpm test` in the e2e folder");
            eprintln!("  build e2e --setup  - install e2e test dependencies");
            eprintln!("  build package      - stage bundled binaries and build the release NSIS installer");
            std::process::exit(1);
        }
    }
}

fn set_dev_diagnostics_env() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "sandbox_mio_lib=debug,info");
    }
    if env::var("RUST_BACKTRACE").is_err() {
        env::set_var("RUST_BACKTRACE", "1");
    }
}

fn setup_e2e() {
    run(root_dir(), "cargo", &["install", "tauri-driver", "--locked"]);
    if cfg!(windows) {
        run(
            root_dir(),
            "cargo",
            &[
                "install",
                "--git",
                "https://github.com/chippers/msedgedriver-tool",
            ],
        );
        fs::create_dir_all(tools_dir()).unwrap();
        run(tools_dir(), "msedgedriver-tool.exe", &[]);
    } else {
        todo!()
    }
}

/// Sibling git worktrees of `target_root` (`git worktree list --porcelain`), `target_root` itself
/// excluded. Empty on any git failure rather than an error — a missing sibling just means
/// `setup_worktree()` falls back to `fetch_*`.
fn sibling_worktrees(target_root: &Path) -> Vec<PathBuf> {
    let Ok(output) = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(target_root)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let target_canonical = fs::canonicalize(target_root).ok();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .filter(|p| fs::canonicalize(p).ok() != target_canonical)
        .collect()
}

/// Hard-links `dst` from `src` (falling back to a byte copy across volumes), removing an existing
/// `dst` first. Hard links make staged assets shared mutable state between worktrees; without the
/// removal, `fs::copy` on Windows would write into an existing destination in place rather than
/// replacing it, silently rewriting through the link (Resolved Decision 11).
fn link_or_copy(src: &Path, dst: &Path) {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let _ = fs::remove_file(dst);
    if fs::hard_link(src, dst).is_err() {
        fs::copy(src, dst)
            .unwrap_or_else(|e| panic!("failed to stage {} -> {}: {}", src.display(), dst.display(), e));
    }
}

/// Recursively `link_or_copy`s every entry of `src` into `dst`.
///
/// Adds and overwrites only — a file present in `dst` but absent from `src` is left in place, so a
/// renamed or dropped asset lingers in a target worktree that was staged before the rename. Harmless
/// today (every consumer resolves assets by name), but a full mirror would have to prune.
fn link_or_copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            link_or_copy_tree(&from, &to);
        } else {
            link_or_copy(&from, &to);
        }
    }
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
fn staged_version(binaries: &Path, key: &str) -> Option<String> {
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

fn record_staged_version_in(binaries: &Path, key: &str, version: &str) {
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

/// One independently stageable set of files under `src-tauri/binaries`. Probed and linked per group
/// rather than as one tree: a sibling commonly has Mutagen staged but not llama (`fetch_llama` skips
/// when already present, and the heavy tier treats llama as optional), and linking the whole tree off
/// a single sentinel silently produced a partially staged target whose `fetch_*` fallback never ran.
struct AssetGroup {
    key: &'static str,
    version: &'static str,
    /// Existence check, relative to `src-tauri/binaries`.
    sentinel: &'static str,
    /// Files and directories to link, relative to `src-tauri/binaries`.
    paths: Vec<String>,
    fetch: fn(),
}

fn asset_groups() -> Vec<AssetGroup> {
    let mutagen_exe = format!("mutagen-{}-pc-windows-msvc.exe", env::consts::ARCH);
    vec![
        AssetGroup {
            key: "mutagen",
            version: MUTAGEN_VERSION,
            sentinel: "mutagen-agents.tar.gz",
            paths: vec!["mutagen-agents.tar.gz".into(), mutagen_exe],
            fetch: fetch_mutagen,
        },
        AssetGroup {
            key: "llama",
            version: LLAMA_VERSION,
            sentinel: "llama/llama-server.exe",
            paths: vec!["llama".into()],
            fetch: fetch_llama,
        },
        AssetGroup {
            key: "podman",
            version: PODMAN_VERSION,
            sentinel: "podman/podman.exe",
            paths: vec!["podman".into()],
            fetch: fetch_podman,
        },
    ]
}

/// Stages `tools/msedgedriver.exe` and all of `src-tauri/binaries/` into `target` (defaulting to
/// the invoking worktree), plus `pnpm install` (one install covers `e2e/` too, via
/// `pnpm-workspace.yaml`). A fresh worktree has neither: both are `.gitignore`d, so `tauri-driver`
/// has no native driver and the Mutagen agent-bundle copy in `e2e/wdio.conf.ts`'s `onPrepare` has no
/// source file (docs/features/parallel-worktree-e2e/02_dev.md, Phase F).
///
/// Per asset group, prefers a hard link from the first sibling worktree whose `STAGED_VERSIONS`
/// manifest agrees with this checkout's pinned version — costs no disk for the large
/// llama.cpp/Podman/agent-bundle payloads. The manifest is what makes the link safe: parallel
/// feature branches are exactly the case where two worktrees sit on different commits, and linking
/// on mere existence would hand this checkout a binary its own `MUTAGEN_VERSION`/`LLAMA_VERSION`/
/// `PODMAN_VERSION` does not pin, which no later `fetch_*` would correct.
///
/// Falls back to `fetch_mutagen`/`fetch_llama`/`fetch_podman` per group when no sibling qualifies —
/// those are not safe to run concurrently with another worktree's fetch, since each extracts through
/// a fixed, shared `env::temp_dir()` path it removes first.
fn setup_worktree(target: Option<String>) {
    let target_root = target.map(PathBuf::from).unwrap_or_else(root_dir);
    let target_root = fs::canonicalize(&target_root)
        .unwrap_or_else(|e| panic!("cannot resolve target worktree {}: {}", target_root.display(), e));

    run(target_root.clone(), "pnpm", &["install"]);

    let siblings = sibling_worktrees(&target_root);

    let target_tools = target_root.join("tools");
    match siblings
        .iter()
        .map(|s| s.join("tools").join("msedgedriver.exe"))
        .find(|p| p.exists())
    {
        Some(src) => link_or_copy(&src, &target_tools.join("msedgedriver.exe")),
        None => {
            // `setup_e2e()` installs msedgedriver-tool into ~/.cargo/bin globally, so a missing
            // sibling is a fetch away rather than a dead end — mirroring how the binaries groups
            // below fall back to their own `fetch_*`.
            fs::create_dir_all(&target_tools).unwrap();
            println!("no sibling worktree has tools/msedgedriver.exe staged; fetching it directly");
            if !try_run(target_tools, "msedgedriver-tool.exe", &[]) {
                println!(
                    "could not run msedgedriver-tool.exe; run `cargo run --bin build -- e2e --setup` \
                     in {} to install it and fetch the driver",
                    target_root.display()
                );
            }
        }
    }

    let target_binaries = target_root.join("src-tauri").join("binaries");
    for group in asset_groups() {
        let source = siblings
            .iter()
            .map(|s| s.join("src-tauri").join("binaries"))
            .find(|b| {
                b.join(group.sentinel).exists()
                    && staged_version(b, group.key).as_deref() == Some(group.version)
            });
        match source {
            Some(src) => {
                for rel in &group.paths {
                    let from = src.join(rel);
                    let to = target_binaries.join(rel);
                    if from.is_dir() {
                        link_or_copy_tree(&from, &to);
                    } else if from.exists() {
                        link_or_copy(&from, &to);
                    }
                }
                record_staged_version_in(&target_binaries, group.key, group.version);
                println!("Linked {} {} from {}", group.key, group.version, src.display());
            }
            None => {
                println!(
                    "no sibling worktree has {} {} staged; fetching directly into {} — do not run \
                     this concurrently with another worktree's fetch (02_dev.md Phase F)",
                    group.key,
                    group.version,
                    target_root.display()
                );
                let cwd = env::current_dir().unwrap();
                env::set_current_dir(&target_root).unwrap();
                (group.fetch)();
                env::set_current_dir(cwd).unwrap();
            }
        }
    }

    println!("Worktree staged: {}", target_root.display());
}

fn fetch_mutagen() {
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
fn fetch_llama() {
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
/// `src-tauri/binaries/podman/README.md`). `backends::podman_command()` resolves these from the
/// bundled resource dir at runtime instead of PATH.
fn fetch_podman() {
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

/// Kill lingering `podman`/`gvproxy`/`win-sshproxy` processes running out of *this worktree's own*
/// `target/debug` from a previous `dev` run. Unlike the app's normal shutdown (where these are meant
/// to outlive the app so the podman machine keeps running), a dev restart re-stages the same
/// binaries every time, so a stale process still holding one open causes the next `stage_podman`
/// copy - or a `cargo clean` - to fail with a Windows file-lock error.
///
/// Matched against the full absolute `src-tauri\target\debug\` path via an environment variable,
/// compared with `StartsWith`/`OrdinalIgnoreCase` rather than PowerShell `-like` (which gives `[`,
/// `]`, `*`, `?` wildcard meaning and breaks on a `'` in the path) — so a `build dev` in one worktree
/// never kills another worktree's e2e podman processes, including one nested under this worktree's
/// own `.claude/worktrees/` (a root-prefix filter would still match those).
fn kill_stale_dev_processes() {
    if !cfg!(windows) {
        return;
    }
    let mut dir = debug_dir().to_string_lossy().into_owned();
    if !dir.ends_with('\\') {
        dir.push('\\');
    }
    let script = r#"
        Get-Process -Name podman,win-sshproxy,gvproxy -ErrorAction SilentlyContinue |
            Where-Object { $_.Path -and $_.Path.StartsWith($env:MIO_DEBUG_DIR, [StringComparison]::OrdinalIgnoreCase) } |
            Stop-Process -Force -ErrorAction SilentlyContinue
    "#;
    let _ = Command::new("powershell")
        .env("MIO_DEBUG_DIR", &dir)
        .args(["-NoProfile", "-Command", script])
        .status();
}

/// Stage the bundled Podman binaries next to the debug exe so `resource_dir()/podman` resolves
/// under `tauri dev`, mirroring `stage_llama`. Skipped with a warning when not staged in the repo
/// (see `src-tauri/binaries/podman/README.md`).
fn stage_podman() {
    let src = binaries_dir().join("podman");
    if !src.join("podman.exe").exists() {
        println!(
            "podman not staged at {}; Podman-backed sandboxes will fail at binary resolution. \
             Stage it there per src-tauri/binaries/podman/README.md (or run `cargo run --bin build \
             -- dev --setup`).",
            src.display()
        );
        return;
    }
    let dst = root_dir().join("src-tauri").join("target").join("debug").join("podman");
    copy_dir(&src, &dst)
        .unwrap_or_else(|e| panic!("failed to stage podman to {}: {}", dst.display(), e));
    println!("Staged Podman: {}", dst.display());
}

/// Stage the bundled llama.cpp Vulkan engine next to the debug exe so `resource_dir()/llama`
/// resolves under `tauri dev`: a dev / `--no-bundle` build does not copy Tauri `resources` (that
/// only happens at bundle time), yet the engine launches `llama-server` from `<resource_dir>/llama`
/// (`src-tauri/src/inference/llama.rs`). Mirrors the e2e harness staging in `e2e/wdio.conf.ts`.
///
/// Skipped with a warning when the binaries are not staged in the repo, so a checkout without the
/// (large, un-versioned) Vulkan release still runs; local inference then fails at sidecar
/// resolution with a clear path. See `src-tauri/binaries/llama/README.md`.
fn stage_llama() {
    let exe_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    let src = binaries_dir().join("llama");
    if !src.join(exe_name).exists() {
        println!(
            "llama-server not staged at {}; local inference will fail at sidecar resolution. \
             Stage the llama.cpp Vulkan release there per src-tauri/binaries/llama/README.md.",
            src.display()
        );
        return;
    }
    let dst = root_dir().join("src-tauri").join("target").join("debug").join("llama");
    copy_dir(&src, &dst)
        .unwrap_or_else(|e| panic!("failed to stage llama engine to {}: {}", dst.display(), e));
    println!("Staged llama.cpp engine: {}", dst.display());
}

/// Stage the Mutagen agent bundle next to the debug exe so the sidecar's own runtime search (it
/// looks for `mutagen-agents.tar.gz` beside itself) resolves under `tauri dev`, mirroring
/// `stage_llama`/`stage_podman`. Skipped with a warning when not staged (run `cargo run --bin
/// build -- dev --setup` first).
fn stage_mutagen_agents() {
    let src = binaries_dir().join("mutagen-agents.tar.gz");
    if !src.exists() {
        println!(
            "mutagen-agents.tar.gz not staged at {}; remote terminals will fail agent \
             installation. Run `cargo run --bin build -- dev --setup` to fetch it.",
            src.display()
        );
        return;
    }
    let dst = root_dir().join("src-tauri").join("target").join("debug").join("mutagen-agents.tar.gz");
    if let Err(e) = fs::copy(&src, &dst) {
        if dst.exists() {
            println!(
                "warning: could not update {} ({e}); it may be locked. Using the already-staged copy.",
                dst.display()
            );
        } else {
            panic!("failed to stage Mutagen agent bundle to {}: {}", dst.display(), e);
        }
    } else {
        println!("Staged Mutagen agent bundle: {}", dst.display());
    }
}

/// Recursively copy `src` into `dst`, overwriting existing files (std has no recursive copy).
///
/// A destination `.exe` may be locked by a running process (e.g. `podman.exe` kept alive by
/// Podman Desktop or a running Podman machine) - overwriting it fails with a Windows "the process
/// cannot access the file" error. When that happens and the file is already staged, skip it with a
/// warning instead of failing the whole `dev` build; the already-staged copy is still usable.
fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&from, &to)?;
        } else if let Err(e) = fs::copy(&from, &to) {
            if to.exists() {
                println!(
                    "warning: could not update {} ({e}); it may be locked by a running process. \
                     Using the already-staged copy.",
                    to.display()
                );
            } else {
                return Err(e);
            }
        }
    }
    Ok(())
}

/// `run` without the panic: returns whether the command both launched and exited successfully, for
/// callers that have a printable fallback.
fn try_run(dir: PathBuf, cmd: &str, args: &[&str]) -> bool {
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd).args(args);
        c
    } else {
        let mut c = Command::new(cmd);
        c.args(args);
        c
    };
    let Ok(mut child) = command.current_dir(dir).spawn() else {
        return false;
    };
    #[cfg(windows)]
    win_job::assign(&child);
    child.wait().map(|s| s.success()).unwrap_or(false)
}

fn run(dir: PathBuf, cmd: &str, args: &[&str]) {
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd).args(args);
        c
    } else {
        let mut c = Command::new(cmd);
        c.args(args);
        c
    };
    let mut child = command
        .current_dir(dir)
        .spawn()
        .unwrap_or_else(|e| panic!("failed to run `{}`: {}", cmd, e));
    #[cfg(windows)]
    win_job::assign(&child);
    let status = child
        .wait()
        .unwrap_or_else(|e| panic!("failed to wait for `{}`: {}", cmd, e));
    assert!(status.success(), "`{}` exited with {}", cmd, status);
}

/// A Windows job object that kills every process still assigned to it the instant this binary's
/// last handle to the job closes — which the OS does automatically on any exit path (normal
/// return, panic, Ctrl+C, or a hard crash of `build.exe` itself). Every process `run`/`try_run`
/// spawns joins the job (child processes inherit job membership from their parent, transitively),
/// so a crash deep in a `pnpm` -> `node` -> `wdio` -> `vite`/`tauri build` chain — observed as
/// `pnpm` exiting with `0xc0000005` mid-`e2e` run — no longer orphans the still-running
/// grandchildren to keep building/testing in the background after `build.rs` has already panicked.
#[cfg(windows)]
mod win_job {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::atomic::{AtomicIsize, Ordering};

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;

    #[repr(C)]
    struct JobObjectBasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    struct JobObjectExtendedLimitInformation {
        basic_limit_information: JobObjectBasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    extern "system" {
        fn CreateJobObjectW(lp_job_attributes: *mut c_void, lp_name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(
            h_job: *mut c_void,
            job_object_information_class: u32,
            lp_job_object_information: *mut c_void,
            cb_job_object_information_length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(h_job: *mut c_void, h_process: *mut c_void) -> i32;
    }

    static JOB: AtomicIsize = AtomicIsize::new(0);

    /// Creates the job and turns on kill-on-close. A no-op (leaving `JOB` at 0, so `assign`
    /// harmlessly skips) if either Windows API call fails — cleanup then falls back to whatever
    /// each tool's own teardown does, same as before this existed.
    pub fn setup() {
        let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if handle.is_null() {
            return;
        }
        let mut info: JobObjectExtendedLimitInformation = unsafe { std::mem::zeroed() };
        info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
            )
        };
        if ok != 0 {
            JOB.store(handle as isize, Ordering::SeqCst);
        }
    }

    /// Assigns `child` to the job from `setup()`; a no-op if `setup()` was never called or failed.
    pub fn assign(child: &Child) {
        let job = JOB.load(Ordering::SeqCst);
        if job == 0 {
            return;
        }
        unsafe {
            AssignProcessToJobObject(job as *mut c_void, child.as_raw_handle() as *mut c_void);
        }
    }
}
