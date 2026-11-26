//! `ci`: the CI sequence run locally, split into the same `checks` and `e2e-light` jobs the
//! workflows run, plus the failure-diagnostics collector.

use std::{
    env,
    ffi::OsStr,
    fs,
    path::PathBuf,
    time::Instant,
};

use crate::binaries::fetch_bundled_binaries;
use crate::dev::copy_dir;
use crate::docs_check;
use crate::e2e::{in_github_actions, run_e2e_suite, setup_e2e};
use crate::exec::{run, run_env};
use crate::paths::{e2e_dir, root_dir};

/// `TAURI_CONFIG` merge patch for `ci checks`' cargo commands: drops `tauri.conf.json`'s bundle paths,
/// which tauri-build rejects when missing and which compiling and unit-testing never read, so no
/// stand-in files are written. Never set for `dev`, `e2e` or `package`, which need the real bundle.
const CHECKS_TAURI_CONFIG: &str = r#"{"bundle":{"externalBin":null,"resources":null}}"#;

/// `ci diagnostics`' output folder; no leading dot, since upload-artifact skips hidden paths.
const E2E_DIAGNOSTICS_DIR: &str = "e2e-diagnostics";

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
pub(crate) fn ci_checks() {
    ci_step("cargo xtask docs", || {
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
pub(crate) fn ci_e2e_light() {
    ci_step("pnpm install", || run(root_dir(), "pnpm", &["install", "--frozen-lockfile"]));
    // The real sidecar binaries, as on a developer machine: only one-shot podman/Mutagen calls are
    // scripted under MIO_MOCK_BACKEND, so a stand-in exe could be reached and fail only in CI.
    ci_step("fetch bundled binaries", fetch_bundled_binaries);
    ci_step("cargo xtask e2e --setup", setup_e2e);
    ci_step("cargo xtask e2e light", || run_e2e_suite("light"));
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
pub(crate) fn collect_e2e_diagnostics() {
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
