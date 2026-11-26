//! `dev`: starts the Vite dev server, stages the bundled binaries next to the debug exe, builds and
//! launches the app directly.

use std::{
    env, fs,
    net::TcpStream,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use crate::exec::{run, spawn_background, try_run};
use crate::paths::{binaries_dir, debug_dir, root_dir};

/// Mirrors `tauri.conf.json`'s `build.devUrl`. `run_dev_directly` polls this to know when the Vite
/// dev server it started is ready for the app to load, since the app itself has no retry loop.
/// `localhost`, not a literal `127.0.0.1`: Vite's default bind can be IPv6-loopback-only, and
/// `TcpStream::connect` does no hostname fallback across families the way letting `localhost`
/// resolve does.
const DEV_SERVER_ADDR: &str = "localhost:1420";

/// `dev`: stage everything next to the debug exe, then build and launch the app.
pub(crate) fn dev() {
    kill_stale_dev_processes();
    set_dev_diagnostics_env();
    stage_llama();
    stage_podman();
    move_aside_locked_dev_files();
    stage_mutagen_agents();
    run_dev_directly();
}

fn set_dev_diagnostics_env() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "sandbox_mio_lib=debug,info");
    }
    if env::var("RUST_BACKTRACE").is_err() {
        env::set_var("RUST_BACKTRACE", "1");
    }
}

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

/// Kill lingering `podman`/`gvproxy`/`win-sshproxy` processes running out of *this worktree's own*
/// `target/debug` from a previous `dev` run. Unlike the app's normal shutdown (where these are meant
/// to outlive the app so the podman machine keeps running), a dev restart re-stages the same
/// binaries every time, so a stale process still holding one open causes the next `stage_podman`
/// copy - or a `cargo clean` - to fail with a Windows file-lock error.
///
/// Matched against the full absolute `src-tauri\target\debug\` path via an environment variable,
/// compared with `StartsWith`/`OrdinalIgnoreCase` rather than PowerShell `-like` (which gives `[`,
/// `]`, `*`, `?` wildcard meaning and breaks on a `'` in the path) — so a `cargo xtask dev` in one
/// worktree never kills another worktree's e2e podman processes, including one nested under this
/// worktree's own `.claude/worktrees/` (a root-prefix filter would still match those).
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
             Stage it there per src-tauri/binaries/podman/README.md (or run `cargo xtask \
             dev --setup`).",
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
             installation. Run `cargo xtask dev --setup` to fetch it.",
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
pub(crate) fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
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

/// Runs the dev workflow without going through `pnpm run tauri dev` (`cargo run`, under the hood),
/// which puts the app process in a Windows Job Object. This function carries duties `tauri dev`
/// does not: the locked-file move-aside and retry around `cargo build`, stdin to the null device
/// (`docs/features/podman-process-isolation/02_dev.md`, Resolved Decision 5) and the opt-in
/// `devtools` feature. Returning to `tauri dev` is deferred
/// (`docs/features/remove-microsandbox/04_improvements.md`).
///
/// Costs `tauri dev`'s auto-rebuild-and-relaunch on Rust file changes: rerun `cargo xtask dev` after
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
    // Not `run`/`try_run`: the app's own exit status must not panic the tool.
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
    // would die with the tool regardless — killed explicitly here so closing the app window ends
    // the dev server promptly instead of leaving it running until the tool itself is interrupted.
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
