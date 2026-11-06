use std::{env, fs, net::TcpStream, path::{Path, PathBuf}, process::Command, time::{Duration, Instant}};

/// `build docs`: the documentation rules, as code (`docs_check.rs`).
mod docs_check;

/// Mutagen release staged as the Tauri sidecar. Pinned to match the e2e suite's Mutagen.
const MUTAGEN_VERSION: &str = "v0.18.1";

/// llama.cpp release whose prebuilt Windows Vulkan binaries are staged as the bundled inference
/// engine (`src-tauri/binaries/llama`). Pinned so every checkout stages the same `llama-server`.
const LLAMA_VERSION: &str = "b9835";

/// Podman release whose Windows remote-client archive is staged as the bundled container engine
/// (`src-tauri/binaries/podman`). Pinned so every checkout stages the same `podman`/`gvproxy`/
/// `win-sshproxy` that `backends::podman_command()` resolves at runtime.
const PODMAN_VERSION: &str = "v5.6.2";

/// microsandbox release whose per-platform bundle (`msb` plus its matching `libkrunfw`) is staged as
/// the bundled VM runtime (`src-tauri/binaries/msb`), resolved by `msb_executable()` in
/// `src-tauri/src/process/mod.rs`. Must be at least `MIN_MSB_VERSION` in `src-tauri/src/machine/msb.rs`.
const MSB_VERSION: &str = "v0.7.12";

/// Mirrors `tauri.conf.json`'s `build.devUrl`. `run_dev_directly` polls this to know when the Vite
/// dev server it started is ready for the app to load, since the app itself has no retry loop.
/// `localhost`, not a literal `127.0.0.1`: Vite's default bind can be IPv6-loopback-only, and
/// `TcpStream::connect` does no hostname fallback across families the way letting `localhost`
/// resolve does.
const DEV_SERVER_ADDR: &str = "localhost:1420";

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
            fetch_msb();
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
            // Fetched here, not only in `--setup`: `tauri.conf.json` lists `binaries/msb` as a
            // resource, so `cargo build` fails in a checkout staged before msb was bundled.
            fetch_msb();
            move_aside_locked_dev_files();
            stage_msb();
            stage_mutagen_agents();
            run_dev_directly();
        }
        Some("e2e") if args.get(2).map(String::as_str) == Some("--setup") => {
            setup_e2e();
        }
        // Any further `e2e --<flag>` arm belongs above this one, which forwards its second argument
        // straight into `pnpm run test:{suite}` — an arm added below would run `pnpm run test:--<flag>`.
        Some("e2e") => {
            check_e2e_driver();
            let mut suite = "standard";
            if args.len() > 2 {
                suite = args.get(2).map(String::as_str).unwrap();
            }

            run(e2e_dir(), "pnpm", &["run", &format!("test:{}", suite)]);
        }
        Some("docs") if matches!(args.get(2).map(String::as_str), None | Some("check")) => {
            std::process::exit(docs_check::run(&root_dir(), false));
        }
        Some("docs") if args.get(2).map(String::as_str) == Some("--fix") => {
            std::process::exit(docs_check::run(&root_dir(), true));
        }
        Some("package") => {
            // Bundling copies from src-tauri/binaries at build time (unlike `dev`, which needs
            // `stage_*` to mirror them into target/debug); make sure they're present first.
            fetch_mutagen();
            fetch_llama();
            fetch_podman();
            fetch_msb();
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
            eprintln!("  build dev          - start the Vite dev server, build, and launch the app directly");
            eprintln!("                       (not via `tauri dev`/`cargo run`: msb create needs to break away");
            eprintln!("                       from the Windows Job Object those put the app process in — see");
            eprintln!("                       `run_dev_directly`'s doc comment. Rerun after a Rust change; no");
            eprintln!("                       auto-relaunch)");
            eprintln!("  build dev --setup  - install app dependencies and stage the Mutagen sidecar + llama.cpp engine + Podman");
            eprintln!("                       + microsandbox (msb + libkrunfw for the host OS/arch)");
            eprintln!("  build e2e          - run `pnpm test` in the e2e folder, first re-staging");
            eprintln!("                       tools/msedgedriver.exe if it no longer matches WebView2");
            eprintln!("  build e2e --setup  - install e2e test dependencies");
            eprintln!("  build package      - stage bundled binaries and build the release NSIS installer");
            eprintln!("  build docs         - check docs/ against the documentation rules in docs_check.rs");
            eprintln!("  build docs --fix   - regenerate every docs/ index.md from frontmatter, then check");
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

/// Runs before every `e2e` tier: re-stages `tools/msedgedriver.exe` whenever its major version
/// differs from the installed WebView2 runtime's, and says so. WebView2 updates itself in the
/// background, and a stale driver fails every session with `session not created: This version of
/// Microsoft Edge WebDriver only supports Microsoft Edge version N` before any app code runs.
///
/// Never fails the run: when either version cannot be read, or `msedgedriver-tool` is missing or
/// errors, it prints what it found plus the `e2e --setup` remedy and leaves the suite to report.
fn check_e2e_driver() {
    if !cfg!(windows) {
        return;
    }
    let driver = tools_dir().join("msedgedriver.exe");
    let driver_version = Command::new(&driver)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| first_version_token(&String::from_utf8_lossy(&o.stdout)));
    let webview_version = webview2_version();

    let major = |v: &Option<String>| v.as_deref().and_then(|v| v.split('.').next()).map(str::to_string);
    let describe = |v: &Option<String>| v.clone().unwrap_or_else(|| "unknown".into());

    match (&driver_version, &webview_version) {
        (Some(_), Some(_)) if major(&driver_version) == major(&webview_version) => {
            println!(
                "[e2e] msedgedriver {} matches WebView2 {}",
                describe(&driver_version),
                describe(&webview_version)
            );
            return;
        }
        (_, None) => {
            println!(
                "[e2e] warning: could not read the installed WebView2 version; skipping the \
                 msedgedriver check (driver: {})",
                describe(&driver_version)
            );
            return;
        }
        _ => {}
    }

    println!(
        "[e2e] msedgedriver {} does not match WebView2 {}; re-staging tools/msedgedriver.exe",
        describe(&driver_version),
        describe(&webview_version)
    );
    fs::create_dir_all(tools_dir()).unwrap();
    // Removed first, as in the `fetch_*` helpers: `setup_worktree()` may have hard-linked this file
    // from a sibling worktree, and rewriting it in place would change the sibling's driver too.
    let _ = fs::remove_file(&driver);
    if try_run(tools_dir(), "msedgedriver-tool.exe", &[]) {
        println!("[e2e] re-staged msedgedriver for WebView2 {}", describe(&webview_version));
    } else {
        println!(
            "[e2e] warning: could not re-stage msedgedriver; run `./build.exe e2e --setup` to \
             install msedgedriver-tool and fetch a matching driver. Light sessions will fail with \
             `session not created` until then."
        );
    }
}

/// The installed Evergreen WebView2 runtime's version (`pv` under its EdgeUpdate client key),
/// checked machine-wide first, then per-user. `None` when neither key exists.
fn webview2_version() -> Option<String> {
    const CLIENT: &str = r"Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    [
        format!(r"HKLM\SOFTWARE\WOW6432Node\{CLIENT}"),
        format!(r"HKLM\SOFTWARE\{CLIENT}"),
        format!(r"HKCU\Software\{CLIENT}"),
    ]
    .iter()
    .find_map(|key| {
        let out = Command::new("reg").args(["query", key, "/v", "pv"]).output().ok()?;
        if !out.status.success() {
            return None;
        }
        first_version_token(&String::from_utf8_lossy(&out.stdout))
            .filter(|v| v != "0.0.0.0")
    })
}

/// The first whitespace-separated token shaped like a dotted numeric version (`156.0.4314.58`).
fn first_version_token(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|t| t.contains('.') && t.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())))
        .map(str::to_string)
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
    let mut entries: Vec<(String, String)> = ["mutagen", "llama", "podman", "msb"]
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
        AssetGroup {
            key: "msb",
            version: MSB_VERSION,
            sentinel: if cfg!(windows) { "msb/msb.exe" } else { "msb/msb" },
            paths: vec!["msb".into()],
            fetch: fetch_msb,
        },
    ]
}

/// Stages `tools/msedgedriver.exe` and all of `src-tauri/binaries/` into `target` (defaulting to
/// the invoking worktree), plus `pnpm install` (one install covers `e2e/` too, via
/// `pnpm-workspace.yaml`). A fresh worktree has neither: both are `.gitignore`d, so the e2e harness
/// has no `msedgedriver` to spawn and the Mutagen agent-bundle copy in `e2e/wdio.conf.ts`'s
/// `onPrepare` has no source file (docs/features/parallel-worktree-e2e/02_dev.md, Phase F).
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

/// The microsandbox release bundle for the host this `build` runs on, as `(asset name, is_zip)`.
/// Staged for the build host, so an installer carries that host's runtime; cross-target packaging
/// would have to key this off the Tauri target triple instead.
fn msb_bundle_asset() -> (String, bool) {
    let os = match env::consts::OS {
        "windows" => "windows",
        "linux" => "linux",
        "macos" => "darwin",
        other => panic!("microsandbox publishes no runtime bundle for {}", other),
    };
    let arch = match env::consts::ARCH {
        a @ ("x86_64" | "aarch64") => a,
        other => panic!("microsandbox publishes no runtime bundle for {}", other),
    };
    assert!(
        !(os == "darwin" && arch == "x86_64"),
        "microsandbox {} publishes no darwin-x86_64 bundle (Apple Silicon only)",
        MSB_VERSION
    );
    if os == "windows" {
        (format!("microsandbox-{os}-{arch}.zip"), true)
    } else {
        (format!("microsandbox-{os}-{arch}.tar.gz"), false)
    }
}

/// Lowercase hex SHA-256 of `file`, via `certutil` on Windows and `shasum` elsewhere — both ship
/// with the OS, keeping `build` dependency-free like the `curl`/`tar` calls around it.
fn sha256_of(file: &Path) -> String {
    let output = if cfg!(windows) {
        Command::new("certutil").arg("-hashfile").arg(file).arg("SHA256").output()
    } else {
        Command::new("shasum").args(["-a", "256"]).arg(file).output()
    }
    .unwrap_or_else(|e| panic!("failed to hash {}: {}", file.display(), e));
    assert!(output.status.success(), "hashing {} failed", file.display());
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find(|t| t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| panic!("no SHA-256 digest in hash output for {}", file.display()))
}

/// Download the microsandbox runtime bundle for the host OS/arch, verify it against the release's
/// `checksums.sha256`, and stage its contents flat into `src-tauri/binaries/msb`: `msb[.exe]` plus
/// `libkrunfw` under its per-OS name (`libkrunfw.dll`, `libkrunfw.5.dylib`, `libkrunfw.so.<ver>`).
/// The flat layout is load-bearing: `msb` resolves `libkrunfw` beside its own executable.
///
/// Unlike the other `fetch_*` helpers, an existing copy is re-fetched when the recorded version
/// differs, so bumping `MSB_VERSION` takes effect without deleting the directory by hand.
fn fetch_msb() {
    let dest_dir = binaries_dir().join("msb");
    let exe_name = if cfg!(windows) { "msb.exe" } else { "msb" };
    if dest_dir.join(exe_name).exists()
        && staged_version(&binaries_dir(), "msb").as_deref() == Some(MSB_VERSION)
    {
        println!("microsandbox {MSB_VERSION} already staged: {}", dest_dir.display());
        return;
    }
    let (asset, is_zip) = msb_bundle_asset();
    let base =
        format!("https://github.com/superradcompany/microsandbox/releases/download/{MSB_VERSION}");
    let work = env::temp_dir().join(format!("microsandbox-{MSB_VERSION}"));
    let _ = fs::remove_dir_all(&work);
    let extract = work.join("extract");
    fs::create_dir_all(&extract).unwrap();
    let archive = work.join(&asset);
    let sums = work.join("checksums.sha256");

    let archive_arg = archive.to_string_lossy();
    let sums_arg = sums.to_string_lossy();
    let extract_arg = extract.to_string_lossy();
    println!("Downloading microsandbox {MSB_VERSION} ({asset})...");
    run(root_dir(), "curl", &["-fsSL", "-o", &archive_arg, &format!("{base}/{asset}")]);
    run(root_dir(), "curl", &["-fsSL", "-o", &sums_arg, &format!("{base}/checksums.sha256")]);

    let expected = fs::read_to_string(&sums)
        .unwrap()
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let digest = parts.next()?;
            (parts.next()?.trim_start_matches('*') == asset).then(|| digest.to_ascii_lowercase())
        })
        .unwrap_or_else(|| {
            panic!("{} missing from microsandbox {} checksums.sha256", asset, MSB_VERSION)
        });
    assert_eq!(sha256_of(&archive), expected, "checksum mismatch for {asset}");

    let tar = if cfg!(windows) { windows_tar() } else { "tar".to_string() };
    let flags = if is_zip { "-xf" } else { "-xzf" };
    run(root_dir(), &tar, &[flags, &archive_arg, "-C", &extract_arg]);
    assert!(
        extract.join(exe_name).exists(),
        "expected {exe_name} in extracted microsandbox bundle at {}",
        extract.display()
    );
    assert!(
        fs::read_dir(&extract)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("libkrunfw")),
        "expected libkrunfw in extracted microsandbox bundle at {}",
        extract.display()
    );

    // Removed first, as in the other `fetch_*` helpers: a hard-linked destination from
    // `setup_worktree()` would otherwise be rewritten in place, changing a sibling's copy too.
    let _ = fs::remove_dir_all(&dest_dir);
    fs::create_dir_all(&dest_dir).unwrap();
    for entry in fs::read_dir(&extract).unwrap().flatten() {
        fs::copy(entry.path(), dest_dir.join(entry.file_name())).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dest_dir.join(exe_name), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _ = fs::remove_dir_all(&work);
    record_staged_version("msb", MSB_VERSION);
    println!("Staged microsandbox {MSB_VERSION}: {}", dest_dir.display());
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
/// Suffix marking a `target/debug` file renamed aside by [`move_aside_locked_dev_files`].
const LOCKED_ASIDE_MARKER: &str = ".locked-aside-";

/// Frees the `target/debug` files tauri-build rewrites (the Mutagen sidecar, the `msb` resources)
/// while a process still runs from them: a lingering Mutagen daemon or a running sandbox's `msb`
/// VM supervisor. tauri-build otherwise panics with `Access is denied` (os error 5).
///
/// Renamed aside rather than killed: Windows allows renaming an in-use image, the holder keeps
/// running from the renamed file, and killing would drop sync sessions or running sandboxes.
/// The copy restored in its place is unlocked, so tauri-build can overwrite it. Earlier asides are
/// deleted once their holder has exited.
fn move_aside_locked_dev_files() {
    let msb_dir = debug_dir().join("msb");
    let mut candidates = vec![debug_dir().join(if cfg!(windows) { "mutagen.exe" } else { "mutagen" })];
    for dir in [debug_dir(), msb_dir] {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains(LOCKED_ASIDE_MARKER) {
                let _ = fs::remove_file(&path);
            } else if dir.ends_with("msb") && path.is_file() {
                candidates.push(path);
            }
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    for path in candidates {
        // A running image or loaded DLL refuses a write open; an unlocked file is left untouched,
        // since tauri-build only recopies when its build script reruns.
        if !path.is_file() || fs::OpenOptions::new().write(true).open(&path).is_ok() {
            continue;
        }
        let mut aside = path.clone().into_os_string();
        aside.push(format!("{}{}", LOCKED_ASIDE_MARKER, stamp));
        // Copied back from the aside, so the path stays populated even when tauri-build skips.
        match fs::rename(&path, &aside).and_then(|_| fs::copy(&aside, &path).map(|_| ())) {
            Ok(()) => println!(
                "{} is in use by a running process; moved it aside so the build can replace it",
                path.display()
            ),
            Err(e) => println!(
                "warning: {} is in use and could not be moved aside ({}); the build may fail with \
                 `Access is denied` until the process holding it exits",
                path.display(),
                e
            ),
        }
    }
}

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

/// Stage the bundled microsandbox runtime next to the debug exe so `resource_dir()/msb` resolves
/// under `build dev`, mirroring `stage_podman`. A running sandbox's VM supervisor holds these files
/// open; `copy_dir` then keeps the already-staged copy rather than failing.
fn stage_msb() {
    let src = binaries_dir().join("msb");
    if !src.is_dir() {
        println!(
            "microsandbox not staged at {}; every msb call will fail, since the app never falls \
             back to a host install. Run `build dev --setup` to fetch it.",
            src.display()
        );
        return;
    }
    let dst = debug_dir().join("msb");
    copy_dir(&src, &dst)
        .unwrap_or_else(|e| panic!("failed to stage microsandbox to {}: {}", dst.display(), e));
    println!("Staged microsandbox: {}", dst.display());
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

/// Runs the dev workflow without ever putting the app process in a Windows Job Object, unlike
/// `pnpm run tauri dev` (`cargo run`, under the hood). `msb create` spawns its own detached VM
/// supervisor, and that spawn needs to break away from any job the app process itself sits in; the
/// job `cargo run` assigns on Windows never grants that (`rust-lang/cargo#4841`, open since 2017,
/// no known workaround besides not going through `cargo run` at all) — confirmed hands-on.
///
/// Costs `tauri dev`'s auto-rebuild-and-relaunch on Rust file changes: rerun `build dev` after
/// editing Rust source. Frontend (Svelte/Vite) hot-reload is untouched either way, since it never
/// goes through this process tree.
fn run_dev_directly() {
    let vite = spawn_background(root_dir(), "pnpm", &["dev"]);
    wait_for_dev_server();
    run(root_dir().join("src-tauri"), "cargo", &["build"]);

    let exe = debug_dir().join("sandbox-mio.exe");
    // Not `run`/`try_run`: the app's own exit status must not panic `build.exe`.
    // stdin to the null device, stdout/stderr left inherited so dev output still reaches the
    // terminal: keeps the launching console's (possibly wedged) stdin handle out of the app's own
    // table, on top of the app sanitising its handles itself
    // (`docs/features/podman-process-isolation/02_dev.md`, Resolved Decision 5).
    let mut app = Command::new(&exe)
        .current_dir(root_dir())
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to launch {}: {}", exe.display(), e));
    let _ = app.wait();

    // `vite` is a member of `win_job`'s job like every other `spawn_background`/`run` child, so it
    // would die with `build.exe` regardless — killed explicitly here so closing the app window ends
    // the dev server promptly instead of leaving it running until `build.exe` itself is interrupted.
    let mut vite = vite;
    let _ = vite.kill();
}

/// Polls `DEV_SERVER_ADDR` until Vite accepts a connection, so `cargo build` (and the eventual app
/// launch) never race the dev server's own startup. Panics past a generous timeout rather than
/// hanging forever on a dev server that failed to start.
fn wait_for_dev_server() {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if TcpStream::connect(DEV_SERVER_ADDR).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("Vite dev server never came up on {} after 60s", DEV_SERVER_ADDR);
}

/// `run`'s non-blocking counterpart: spawns `cmd`, joins it to `win_job`'s job the same way, but
/// returns the [`std::process::Child`] immediately instead of waiting — for a long-running sidecar
/// like the Vite dev server that has to keep running alongside whatever `dev` does next.
fn spawn_background(dir: PathBuf, cmd: &str, args: &[&str]) -> std::process::Child {
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd).args(args);
        c
    } else {
        let mut c = Command::new(cmd);
        c.args(args);
        c
    };
    let child = command
        .current_dir(dir)
        .spawn()
        .unwrap_or_else(|e| panic!("failed to run `{}`: {}", cmd, e));
    #[cfg(windows)]
    win_job::assign(&child);
    child
}

/// A Windows job object that kills every process still assigned to it the instant this binary's
/// last handle to the job closes — which the OS does automatically on any exit path (normal
/// return, panic, Ctrl+C, or a hard crash of `build.exe` itself). Every process `run`/`try_run`
/// spawns joins the job (child processes inherit job membership from their parent, transitively),
/// so a crash deep in a `pnpm` -> `node` -> `wdio` -> `vite`/`tauri build` chain — observed as
/// `pnpm` exiting with `0xc0000005` mid-`e2e` run — no longer orphans the still-running
/// grandchildren to keep building/testing in the background after `build.rs` has already panicked.
///
/// The job also grants explicit breakaway (not silent breakaway): `msb create` launches its VM
/// supervisor with `CREATE_BREAKAWAY_FROM_JOB`, which fails with `Access is denied` otherwise
/// (docs/features/microsandbox-provider/03_e2e.md, Step 0). Every other descendant stays a member.
#[cfg(windows)]
mod win_job {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::atomic::{AtomicIsize, Ordering};

    const JOB_OBJECT_LIMIT_BREAKAWAY_OK: u32 = 0x0800;
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

    /// Creates the job with kill-on-close and breakaway allowed. A no-op (leaving `JOB` at 0, so
    /// `assign` harmlessly skips) if either Windows API call fails — cleanup then falls back to
    /// whatever each tool's own teardown does, same as before this existed.
    pub fn setup() {
        let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if handle.is_null() {
            return;
        }
        let mut info: JobObjectExtendedLimitInformation = unsafe { std::mem::zeroed() };
        info.basic_limit_information.limit_flags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
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
