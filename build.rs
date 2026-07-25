use std::{env, fs, path::{Path, PathBuf}, process::Command, sync::atomic::{AtomicU32, Ordering}};

/// Mutagen release staged as the Tauri sidecar. Pinned to match the e2e suite's Mutagen.
const MUTAGEN_VERSION: &str = "v0.18.1";

/// llama.cpp release whose prebuilt Windows Vulkan binaries are staged as the bundled inference
/// engine (`src-tauri/binaries/llama`). Pinned so every checkout stages the same `llama-server`.
const LLAMA_VERSION: &str = "b9835";

/// Podman release whose Windows remote-client archive is staged as the bundled container engine
/// (`src-tauri/binaries/podman`). Pinned so every checkout stages the same `podman`/`gvproxy`/
/// `win-sshproxy` that `backends::podman_command()` resolves at runtime.
const PODMAN_VERSION: &str = "v5.6.2";

static CHILD_PID: AtomicU32 = AtomicU32::new(0);

fn e2e_dir() -> PathBuf { PathBuf::from(env::current_dir().unwrap()).join("e2e") }
fn tools_dir() -> PathBuf { PathBuf::from(env::current_dir().unwrap()).join("tools") }
fn binaries_dir() -> PathBuf { root_dir().join("src-tauri").join("binaries") }
fn root_dir() -> PathBuf { env::current_dir().unwrap() }

/// Absolute path to the bsdtar that ships in `System32`. A bare `tar` on PATH may resolve to msys
/// or GNU tar (e.g. from Git for Windows), which cannot extract zip archives and reads `C:\...`
/// paths as a remote `host:path` (`tar: Cannot connect to C: resolve failed`). bsdtar handles both.
fn windows_tar() -> String {
    let root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    format!("{root}\\System32\\tar.exe")
}


fn main() {
    #[cfg(windows)]
    setup_ctrl_handler();

    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("dev") if args.get(2).map(String::as_str) == Some("--setup") => {
            run(root_dir(), "pnpm", &["install"]);
            fetch_mutagen();
            fetch_llama();
            fetch_podman();
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

fn fetch_mutagen() {
    if !cfg!(windows) {
        todo!("Mutagen sidecar fetch is implemented for Windows only");
    }
    let triple = format!("{}-pc-windows-msvc", env::consts::ARCH);
    let dest = binaries_dir().join(format!("mutagen-{triple}.exe"));
    let agents_dest = binaries_dir().join("mutagen-agents.tar.gz");
    if dest.exists() && agents_dest.exists() {
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
    fs::copy(&produced, &dest).unwrap();
    fs::copy(&produced_agents, &agents_dest).unwrap();
    let _ = fs::remove_dir_all(&extract);
    let _ = fs::remove_file(&zip);
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
        println!("llama.cpp engine already staged: {}", server.display());
        return;
    }
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
        fs::copy(&found, dest_dir.join(name)).unwrap();
    }
    let _ = fs::remove_dir_all(&extract);
    let _ = fs::remove_file(&zip);
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

/// Kill lingering `podman`/`gvproxy`/`win-sshproxy` processes running out of `target/debug` from a
/// previous `dev` run. Unlike the app's normal shutdown (where these are meant to outlive the app
/// so the podman machine keeps running), a dev restart re-stages the same binaries every time, so a
/// stale process still holding one open causes the next `stage_podman` copy - or a `cargo clean` -
/// to fail with a Windows file-lock error. Only processes whose exe path is under `target/debug`
/// are targeted, so a real Podman Desktop install is left alone.
fn kill_stale_dev_processes() {
    if !cfg!(windows) {
        return;
    }
    let script = r#"
        Get-Process -Name podman,win-sshproxy,gvproxy -ErrorAction SilentlyContinue |
            Where-Object { $_.Path -and $_.Path -like '*\target\debug\*' } |
            Stop-Process -Force -ErrorAction SilentlyContinue
    "#;
    let _ = Command::new("powershell")
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
    CHILD_PID.store(child.id(), Ordering::Relaxed);
    let status = child
        .wait()
        .unwrap_or_else(|e| panic!("failed to wait for `{}`: {}", cmd, e));
    CHILD_PID.store(0, Ordering::Relaxed);
    assert!(status.success(), "`{}` exited with {}", cmd, status);
}

#[cfg(windows)]
fn setup_ctrl_handler() {
    extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }
    unsafe extern "system" fn handler(_ctrl_type: u32) -> i32 {
        let pid = CHILD_PID.load(Ordering::Relaxed);
        if pid != 0 {
            Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .status()
                .ok();
        }
        0 // pass to default handler so build.exe also exits
    }
    unsafe {
        SetConsoleCtrlHandler(Some(handler), 1);
    }
}
