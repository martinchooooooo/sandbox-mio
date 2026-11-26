//! `e2e`: runs one e2e tier through `pnpm` in `e2e/` after the pre-run checks, and installs the
//! test dependencies (`e2e --setup`).

use std::{
    env, fs,
    path::PathBuf,
    process::Command,
};

use crate::exec::{run, run_env, try_run};
use crate::paths::{e2e_dir, root_dir, tools_dir};

/// Evergreen WebView2 bootstrapper that `ensure_webview2` runs on CI when the runtime is missing.
const WEBVIEW2_BOOTSTRAPPER_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// Runs one e2e tier through `pnpm run test:{suite}` in `e2e/`, after the pre-run checks every tier gets.
pub(crate) fn run_e2e_suite(suite: &str) {
    check_e2e_driver();
    ensure_ssh_keygen_on_path();

    // tsx caches transforms by file content under the OS temp dir but bakes each file's own
    // path in (`import.meta.url`), so worktrees sharing one temp dir run each other's
    // wdio.conf.ts and run-identity.ts paths. A per-worktree temp dir keeps the caches apart.
    let tmp = e2e_dir().join(".tmp");
    fs::create_dir_all(&tmp).unwrap();
    run_env(
        e2e_dir(),
        "pnpm",
        &["run", &format!("test:{}", suite)],
        &[("TEMP", tmp.as_os_str()), ("TMP", tmp.as_os_str())],
    );
}

pub(crate) fn setup_e2e() {
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
            "[e2e] warning: could not re-stage msedgedriver; run `cargo xtask e2e --setup` to \
             install msedgedriver-tool and fetch a matching driver. Light sessions will fail with \
             `session not created` until then."
        );
    }
}

/// Whether this run is a GitHub Actions job, whose runners are throwaway and elevated.
pub(crate) fn in_github_actions() -> bool {
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
