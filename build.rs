use std::{env, ffi::OsStr, fs, net::TcpStream, path::{Path, PathBuf}, process::Command, time::{Duration, Instant}};

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

/// Mirrors `tauri.conf.json`'s `build.devUrl`. `run_dev_directly` polls this to know when the Vite
/// dev server it started is ready for the app to load, since the app itself has no retry loop.
/// `localhost`, not a literal `127.0.0.1`: Vite's default bind can be IPv6-loopback-only, and
/// `TcpStream::connect` does no hostname fallback across families the way letting `localhost`
/// resolve does.
const DEV_SERVER_ADDR: &str = "localhost:1420";

/// `TAURI_CONFIG` merge patch for `ci checks`' cargo commands: drops `tauri.conf.json`'s bundle paths,
/// which tauri-build rejects when missing and which compiling and unit-testing never read, so no
/// stand-in files are written. Never set for `dev`, `e2e` or `package`, which need the real bundle.
const CHECKS_TAURI_CONFIG: &str = r#"{"bundle":{"externalBin":null,"resources":null}}"#;

/// `ci diagnostics`' output folder; no leading dot, since upload-artifact skips hidden paths.
const E2E_DIAGNOSTICS_DIR: &str = "e2e-diagnostics";

/// Evergreen WebView2 bootstrapper that `ensure_webview2` runs on CI when the runtime is missing.
const WEBVIEW2_BOOTSTRAPPER_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

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
            fetch_bundled_binaries();
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
            move_aside_locked_dev_files();
            stage_mutagen_agents();
            run_dev_directly();
        }
        Some("e2e") if args.get(2).map(String::as_str) == Some("--setup") => {
            setup_e2e();
        }
        // Any further `e2e --<flag>` arm belongs above this one, which forwards its second argument
        // straight into `pnpm run test:{suite}` — an arm added below would run `pnpm run test:--<flag>`.
        Some("e2e") => {
            run_e2e_suite(args.get(2).map(String::as_str).unwrap_or("standard"));
        }
        Some("docs") if matches!(args.get(2).map(String::as_str), None | Some("check")) => {
            std::process::exit(docs_check::run(&root_dir(), false));
        }
        Some("docs") if args.get(2).map(String::as_str) == Some("--fix") => {
            std::process::exit(docs_check::run(&root_dir(), true));
        }
        Some("ci")
            if matches!(
                args.get(2).map(String::as_str),
                None | Some("checks") | Some("e2e-light") | Some("diagnostics")
            ) =>
        {
            match args.get(2).map(String::as_str) {
                Some("checks") => ci_checks(),
                Some("e2e-light") => ci_e2e_light(),
                Some("diagnostics") => collect_e2e_diagnostics(),
                _ => {
                    ci_checks();
                    ci_e2e_light();
                }
            }
        }
        Some("package") => {
            // Bundling copies from src-tauri/binaries at build time (unlike `dev`, which needs
            // `stage_*` to mirror them into target/debug); make sure they're present first.
            fetch_bundled_binaries();
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
            eprintln!("                       (not via `tauri dev`/`cargo run` — see `run_dev_directly`'s doc");
            eprintln!("                       comment. Rerun after a Rust change; no auto-relaunch)");
            eprintln!("  build dev --setup  - install app dependencies and stage the Mutagen sidecar + llama.cpp engine + Podman");
            eprintln!("  build e2e          - run `pnpm test` in the e2e folder, first re-staging");
            eprintln!("                       tools/msedgedriver.exe if it no longer matches WebView2 and");
            eprintln!("                       appending ssh-keygen's folder to PATH if it is missing");
            eprintln!("  build e2e --setup  - install e2e test dependencies; on CI, first installs the");
            eprintln!("                       WebView2 runtime if it is missing");
            eprintln!("  build ci           - run the CI sequence locally: `ci checks`, then `ci e2e-light`");
            eprintln!("  build ci checks    - docs check, pnpm install/build/check, paseo-plugins typecheck, then");
            eprintln!("                       cargo check + cargo test with the bundled-binary paths dropped");
            eprintln!("  build ci e2e-light - stage the real bundled binaries, `e2e --setup`, then the light tier");
            eprintln!("  build ci diagnostics");
            eprintln!("                     - collect the light run's failure log, page dumps and mio.log files");
            eprintln!("                       into e2e-diagnostics/ (CI uploads it when e2e-light fails)");
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

/// Stages the bundled binaries into `src-tauri/binaries` (each a no-op once present). Off Windows only
/// Mutagen is bundled: the app runs the system `podman`, and `tauri.linux.conf.json` drops the llama.cpp
/// and Podman resources, so a Linux build has no local inference engine yet.
fn fetch_bundled_binaries() {
    fetch_mutagen();
    if cfg!(windows) {
        fetch_llama();
        fetch_podman();
    }
}

/// Runs one e2e tier through `pnpm run test:{suite}` in `e2e/`, after the pre-run checks every tier gets.
fn run_e2e_suite(suite: &str) {
    check_e2e_driver();
    ensure_ssh_keygen_on_path();

    // tsx caches transforms by file content under the OS temp dir but bakes each file's own
    // path in (`import.meta.url`), so worktrees sharing one temp dir run each other's
    // wdio.conf.ts and run-identity.ts paths. A per-worktree temp dir keeps the caches apart.
    let tmp = e2e_dir().join(".tmp");
    fs::create_dir_all(&tmp).unwrap();
    let script = format!("test:{}", suite);
    let envs = [("TEMP", tmp.as_os_str()), ("TMP", tmp.as_os_str())];
    // A headless Linux host (a CI runner) has no display for the app's GTK window, so give it a virtual one.
    let headless = cfg!(target_os = "linux") && env::var_os("DISPLAY").is_none() && env::var_os("WAYLAND_DISPLAY").is_none();
    if headless {
        println!("[e2e] no display; running the suite under xvfb-run");
        run_env(e2e_dir(), "xvfb-run", &["-a", "pnpm", "run", &script], &envs);
    } else {
        run_env(e2e_dir(), "pnpm", &["run", &script], &envs);
    }
}

fn setup_e2e() {
    if cfg!(windows) {
        // First: msedgedriver-tool reads the runtime's version to pick the driver to fetch.
        ensure_webview2();
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
    } else if cfg!(target_os = "linux") {
        // tauri-driver fronts WebKitWebDriver, the system package that matches the installed WebKitGTK.
        assert!(
            on_path("WebKitWebDriver"),
            "WebKitWebDriver not found on PATH; install it (Debian/Ubuntu: `apt install webkit2gtk-driver`)"
        );
        run(root_dir(), "cargo", &["install", "tauri-driver", "--locked"]);
    } else {
        todo!("e2e setup is implemented for Windows and Linux only")
    }
}

/// Whether an executable named `name` sits in a PATH directory.
fn on_path(name: &str) -> bool {
    env::split_paths(&env::var_os("PATH").unwrap_or_default()).any(|d| d.join(name).is_file())
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

/// Whether this run is a GitHub Actions job, whose runners are throwaway and elevated.
fn in_github_actions() -> bool {
    env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
}

/// Prints the installed WebView2 runtime version; when it is missing, installs the Evergreen runtime
/// on CI only (a silent machine-wide installer is not run on a developer machine) and panics otherwise.
fn ensure_webview2() {
    if let Some(version) = webview2_version() {
        println!("[e2e] WebView2 runtime {version}");
        return;
    }
    assert!(
        in_github_actions(),
        "WebView2 runtime not found; install the Evergreen runtime from \
         https://developer.microsoft.com/microsoft-edge/webview2 (CI installs it automatically)"
    );
    let setup = env::temp_dir().join("MicrosoftEdgeWebview2Setup.exe");
    println!("[e2e] WebView2 runtime missing; installing the Evergreen runtime");
    run(root_dir(), "curl", &["-fsSL", "-o", &setup.to_string_lossy(), WEBVIEW2_BOOTSTRAPPER_URL]);
    // Not `run`: that joins `win_job`'s kill-on-close job, which would kill an installer child that
    // outlives this process.
    let status = Command::new(&setup)
        .args(["/silent", "/install"])
        .status()
        .unwrap_or_else(|e| panic!("failed to run {}: {}", setup.display(), e));
    let _ = fs::remove_file(&setup);
    let version = webview2_version();
    assert!(version.is_some(), "WebView2 runtime install failed (installer exited with {})", status);
    println!("[e2e] WebView2 runtime {}", version.unwrap());
}

/// Appends ssh-keygen's folder to this process's PATH (inherited by the e2e children) when no PATH entry has it;
/// the app runs it by name on create (backlog T8). Appended so Git's msys tools never shadow System32's.
/// Never fails the run: with no ssh-keygen anywhere it warns and leaves the specs to report.
fn ensure_ssh_keygen_on_path() {
    let exe = if cfg!(windows) { "ssh-keygen.exe" } else { "ssh-keygen" };
    let path = env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = env::split_paths(&path).collect();
    if dirs.iter().any(|d| d.join(exe).is_file()) {
        return;
    }
    let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let program_files = env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let found = vec![
        PathBuf::from(system_root).join("System32").join("OpenSSH"),
        PathBuf::from(program_files).join("Git").join("usr").join("bin"),
    ]
    .into_iter()
    .find(|d| d.join(exe).is_file());
    let Some(dir) = found else {
        println!(
            "[e2e] warning: ssh-keygen not found; sandbox creates will fail with `failed to run \
             ssh-keygen`. Install the Windows OpenSSH client."
        );
        return;
    };
    println!("[e2e] ssh-keygen not on PATH; appended {} for this run", dir.display());
    dirs.push(dir);
    env::set_var("PATH", env::join_paths(dirs).unwrap());
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
    // Tauri's `externalBin` resolves `binaries/mutagen` by appending the target triple (and `.exe`).
    let (triple, exe, os) = if cfg!(windows) {
        (format!("{}-pc-windows-msvc", env::consts::ARCH), ".exe", "windows")
    } else if cfg!(target_os = "linux") {
        (format!("{}-unknown-linux-gnu", env::consts::ARCH), "", "linux")
    } else {
        todo!("Mutagen sidecar fetch is implemented for Windows and Linux only");
    };
    let mutagen_arch = match env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => panic!("unsupported arch for the Mutagen release: {}", other),
    };
    let dest = binaries_dir().join(format!("mutagen-{triple}{exe}"));
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

    let archive_ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    let asset = format!("mutagen_{os}_{mutagen_arch}_{MUTAGEN_VERSION}.{archive_ext}");
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
    let tar = if cfg!(windows) { windows_tar() } else { "tar".into() };
    run(root_dir(), &tar, &["-xf", &zip_arg, "-C", &extract_arg]);

    // The archive holds `mutagen[.exe]` plus `mutagen-agents.tar.gz`. Mutagen does not embed remote
    // agents in the exe (it's a 13 MB binary; the agents tarball alone is ~90 MB) — it looks for
    // the tarball next to its own exe at runtime, so both files must ship together.
    let produced = extract.join(format!("mutagen{exe}"));
    let produced_agents = extract.join("mutagen-agents.tar.gz");
    assert!(
        produced.exists(),
        "expected mutagen{exe} in extracted archive at {}",
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
/// Suffix marking a `target/debug` file renamed aside by [`move_aside_locked_dev_files`].
const LOCKED_ASIDE_MARKER: &str = ".locked-aside-";

/// Frees the `target/debug` files tauri-build rewrites (the Mutagen sidecar and the `podman` and
/// `llama` resources) while a process still runs from them: a lingering Mutagen daemon, or a
/// `podman exec … mio-sshd` relay that the Mutagen daemon's SSH transport spawned. tauri-build otherwise panics with `Access is denied` (os error 5)
/// or a sharing violation (os error 32).
///
/// Renamed aside rather than killed: Windows allows renaming an in-use image, the holder keeps
/// running from the renamed file, and killing would drop sync sessions or running sandboxes.
/// The copy restored in its place is unlocked, so tauri-build can overwrite it. Earlier asides are
/// deleted once their holder has exited.
fn move_aside_locked_dev_files() {
    let mut candidates = vec![debug_dir().join(if cfg!(windows) { "mutagen.exe" } else { "mutagen" })];
    let resource_dirs = ["podman", "llama"].map(|d| debug_dir().join(d));
    for dir in std::iter::once(debug_dir()).chain(resource_dirs.iter().cloned()) {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        let is_resource_dir = dir != debug_dir();
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains(LOCKED_ASIDE_MARKER) {
                let _ = fs::remove_file(&path);
            } else if is_resource_dir && path.is_file() {
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
    run_env(dir, cmd, args, &[]);
}

/// [`run`] with extra environment variables set on the child.
fn run_env(dir: PathBuf, cmd: &str, args: &[&str], envs: &[(&str, &OsStr)]) {
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
        .envs(envs.iter().copied())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to run `{}`: {}", cmd, e));
    #[cfg(windows)]
    win_job::assign(&child);
    let status = child
        .wait()
        .unwrap_or_else(|e| panic!("failed to wait for `{}`: {}", cmd, e));
    assert!(status.success(), "`{}` exited with {}", cmd, status);
}

/// Runs the dev workflow without going through `pnpm run tauri dev` (`cargo run`, under the hood),
/// which puts the app process in a Windows Job Object. This function carries duties `tauri dev`
/// does not: the locked-file move-aside and retry around `cargo build`, stdin to the null device
/// (`docs/features/podman-process-isolation/02_dev.md`, Resolved Decision 5) and the opt-in
/// `devtools` feature. Returning to `tauri dev` is deferred
/// (`docs/features/remove-microsandbox/04_improvements.md`).
///
/// Costs `tauri dev`'s auto-rebuild-and-relaunch on Rust file changes: rerun `build dev` after
/// editing Rust source. Frontend (Svelte/Vite) hot-reload is untouched either way, since it never
/// goes through this process tree.
fn run_dev_directly() {
    let mut vite = spawn_background(root_dir(), "pnpm", &["dev"]);
    wait_for_dev_server(&mut vite);
    let devtools = std::env::var("MIO_DEVTOOLS").ok().as_deref() == Some("1");
    let cargo_args: &[&str] = if devtools { &["build", "--features", "devtools"] } else { &["build"] };
    // Repeated here: the Mutagen daemon can respawn its `podman exec` relays from
    // `target/debug/podman/` while Vite starts, locking files again after `dev`'s first pass.
    // One retry covers a relay respawned mid-build; a second failure is a real build error.
    move_aside_locked_dev_files();
    if !try_run(root_dir().join("src-tauri"), "cargo", cargo_args) {
        println!("cargo build failed; moving aside any newly locked target/debug files and retrying once");
        move_aside_locked_dev_files();
        run(root_dir().join("src-tauri"), "cargo", cargo_args);
    }

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
    let _ = vite.kill();
}

/// Polls `DEV_SERVER_ADDR` until Vite accepts a connection, so `cargo build` (and the eventual app
/// launch) never race the dev server's own startup. Fails at once if Vite exits (e.g. port 1420
/// taken); otherwise waits while Vite is alive, since Vite opens no port until Paraglide compiles.
fn wait_for_dev_server(vite: &mut std::process::Child) {
    let start = Instant::now();
    let budget = Duration::from_secs(180);
    let mut hinted = false;
    while start.elapsed() < budget {
        if TcpStream::connect(DEV_SERVER_ADDR).is_ok() {
            return;
        }
        if let Ok(Some(status)) = vite.try_wait() {
            panic!("Vite dev server exited with {} before listening on {}", status, DEV_SERVER_ADDR);
        }
        if !hinted && start.elapsed() > Duration::from_secs(30) {
            hinted = true;
            eprintln!("Still waiting for Vite on {} (up to {}s)", DEV_SERVER_ADDR, budget.as_secs());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("Vite dev server never came up on {} after {}s", DEV_SERVER_ADDR, budget.as_secs());
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

/// Runs one CI step under a log group, ending the whole run at the first failing step. Every helper
/// signals failure by panicking; the panic is caught here so the group closes before the error
/// annotation, and the default panic hook has already printed the message.
fn ci_step(name: &str, step: impl FnOnce()) {
    let on_ci = in_github_actions();
    if on_ci {
        println!("::group::{name}");
    } else {
        println!("\n==> [ci] {name}");
    }
    let start = Instant::now();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(step));
    if on_ci {
        println!("::endgroup::");
    }
    match result {
        Ok(()) => println!("[ci] {name} passed in {:.0}s", start.elapsed().as_secs_f64()),
        Err(_) => {
            if on_ci {
                println!("::error title=ci::{name} failed");
            }
            eprintln!("[ci] {name} failed");
            std::process::exit(1);
        }
    }
}

/// The CI `checks` job: every verification command that needs no Podman machine or bundled binaries.
fn ci_checks() {
    ci_step("./build docs", || {
        assert!(docs_check::run(&root_dir(), false) == 0, "docs check reported errors");
    });
    ci_step("pnpm install", || run(root_dir(), "pnpm", &["install", "--frozen-lockfile"]));
    // svelte-check imports `$lib/paraglide/*`, which only the Paraglide Vite plugin generates.
    ci_step("pnpm build", || run(root_dir(), "pnpm", &["build"]));
    ci_step("pnpm check", || run(root_dir(), "pnpm", &["check"]));
    ci_step("paseo-plugins typecheck", || {
        run(root_dir(), "pnpm", &["--filter", "paseo-plugins", "typecheck"]);
    });
    let src_tauri = root_dir().join("src-tauri");
    let envs = [("TAURI_CONFIG", OsStr::new(CHECKS_TAURI_CONFIG))];
    ci_step("cargo check", || run_env(src_tauri.clone(), "cargo", &["check", "--locked"], &envs));
    ci_step("cargo test", || run_env(src_tauri.clone(), "cargo", &["test", "--locked"], &envs));
}

/// The CI `e2e-light` job's build-and-run sequence.
fn ci_e2e_light() {
    ci_step("pnpm install", || run(root_dir(), "pnpm", &["install", "--frozen-lockfile"]));
    // The real sidecar binaries, as on a developer machine: only one-shot podman/Mutagen calls are
    // scripted under MIO_MOCK_BACKEND, so a stand-in exe could be reached and fail only in CI.
    ci_step("fetch bundled binaries", fetch_bundled_binaries);
    ci_step("./build e2e --setup", setup_e2e);
    ci_step("./build e2e light", || run_e2e_suite("light"));
}

/// Mirrors `sanitize()` in `e2e/lib/run-identity.ts`: runs of `<>:"/\|?*` or whitespace become `-`,
/// outer `-` are trimmed, and an empty result is `root`. Keep the two in step.
fn slug_segment(name: &str) -> String {
    let mut out = String::new();
    let mut in_run = false;
    for c in name.chars() {
        if c.is_whitespace() || "<>:\"/\\|?*".contains(c) {
            if !in_run {
                out.push('-');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() { "root".into() } else { trimmed.into() }
}

/// Bundles the light run's failure log, page dumps and `mio.log*` files into `e2e-diagnostics/` for
/// CI to upload. Never fails, so the `failure() || cancelled()` step adds no second red error.
fn collect_e2e_diagnostics() {
    let out = root_dir().join(E2E_DIAGNOSTICS_DIR);
    let _ = fs::remove_dir_all(&out);
    if let Err(e) = fs::create_dir_all(&out) {
        println!("[ci] warning: cannot create {}: {e}", out.display());
        return;
    }
    let mut collected = 0;

    let log = e2e_dir().join("failures-light.log");
    if log.is_file() && fs::copy(&log, out.join("failures-light.log")).is_ok() {
        collected += 1;
    }
    // Copied as `diag`: upload-artifact skips hidden paths such as `.diag`.
    let diag = e2e_dir().join(".diag");
    if diag.is_dir() && copy_dir(&diag, &out.join("diag")).is_ok() {
        collected += 1;
    }

    // Narrowed to this worktree by its folder name (the app writes `mio.log*` under `logs/`); the slug's hash suffix needs sha256, which std lacks.
    let home = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let basename = root_dir().file_name().map(|n| slug_segment(&n.to_string_lossy())).unwrap_or_default();
    let prefix = format!(".sandbox-mio-e2e-appdata-{basename}-");
    for dir in home.and_then(|h| fs::read_dir(h).ok()).into_iter().flatten().flatten() {
        let name = dir.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) || !dir.path().is_dir() {
            continue;
        }
        // Exactly the 8-hex digest, so `mi-sandbox` skips sibling worktrees such as `mi-sandbox-base-<digest>`.
        let rest = &name[prefix.len()..];
        if rest.len() != 8 || !rest.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let dest = out.join(name.trim_start_matches('.'));
        let _ = fs::create_dir_all(&dest);
        for file in fs::read_dir(dir.path().join("logs")).into_iter().flatten().flatten() {
            if file.file_name().to_string_lossy().starts_with("mio.log")
                && fs::copy(file.path(), dest.join(file.file_name())).is_ok()
            {
                collected += 1;
            }
        }
    }

    if collected == 0 {
        println!("[ci] nothing to collect for {E2E_DIAGNOSTICS_DIR}/");
    } else {
        println!("[ci] collected {collected} item(s) into {}", out.display());
    }
}

/// A Windows job object that kills every process still assigned to it the instant this binary's
/// last handle to the job closes — which the OS does automatically on any exit path (normal
/// return, panic, Ctrl+C, or a hard crash of `build.exe` itself). Every process `run`/`try_run`
/// spawns joins the job (child processes inherit job membership from their parent, transitively),
/// so a crash deep in a `pnpm` -> `node` -> `wdio` -> `vite`/`tauri build` chain — observed as
/// `pnpm` exiting with `0xc0000005` mid-`e2e` run — no longer orphans the still-running
/// grandchildren to keep building/testing in the background after `build.rs` has already panicked.
///
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

    /// Creates the job with kill-on-close. A no-op (leaving `JOB` at 0, so
    /// `assign` harmlessly skips) if either Windows API call fails — cleanup then falls back to
    /// whatever each tool's own teardown does, same as before this existed.
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
