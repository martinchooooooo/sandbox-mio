use std::{env, fs, path::PathBuf, process::Command};

/// Mutagen release staged as the Tauri sidecar. Pinned to match the e2e suite's Mutagen.
const MUTAGEN_VERSION: &str = "v0.18.1";

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("dev") if args.get(2).map(String::as_str) == Some("--setup") => {
            run(root_dir(), "pnpm", &["install"]);
            fetch_mutagen();
        }
        Some("dev") => {
            run(root_dir(), "pnpm", &["run", "tauri", "dev"]);
        }
        Some("e2e") if args.get(2).map(String::as_str) == Some("--setup") => {
            setup_e2e();
        }
        Some("e2e") => {
            run(e2e_dir(), "pnpm", &["test"]);
        }
        _ => {
            eprintln!("Usage:");
            eprintln!("  build dev          - run `pnpm run tauri dev`");
            eprintln!("  build dev --setup  - install app dependencies and stage the Mutagen sidecar");
            eprintln!("  build e2e          - run `pnpm test` in the e2e folder");
            eprintln!("  build e2e --setup  - install e2e test dependencies");
            std::process::exit(1);
        }
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
    if dest.exists() {
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

    // curl and tar (bsdtar, which extracts zip) ship with Windows 10/11, so the download and
    // extraction route through the same `run` helper as every other command.
    let zip_arg = zip.to_string_lossy();
    let extract_arg = extract.to_string_lossy();
    println!("Downloading Mutagen {MUTAGEN_VERSION} for {triple}...");
    run(root_dir(), "curl", &["-fsSL", "-o", &zip_arg, &url]);
    run(root_dir(), "tar", &["-xf", &zip_arg, "-C", &extract_arg]);

    // The archive holds a `mutagen.exe` plus a bundled agents tarball; only the executable is
    // the sidecar — Mutagen ships the remote agents inside it.
    let produced = extract.join("mutagen.exe");
    assert!(
        produced.exists(),
        "expected mutagen.exe in extracted archive at {}",
        produced.display()
    );
    fs::copy(&produced, &dest).unwrap();
    let _ = fs::remove_dir_all(&extract);
    let _ = fs::remove_file(&zip);
    println!("Staged Mutagen sidecar: {}", dest.display());
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
    let status = command
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to run `{}`: {}", cmd, e));
    assert!(status.success(), "`{}` exited with {}", cmd, status);
}

fn e2e_dir() -> PathBuf { PathBuf::from(env::current_dir().unwrap()).join("e2e") }
fn tools_dir() -> PathBuf { PathBuf::from(env::current_dir().unwrap()).join("tools") }
fn binaries_dir() -> PathBuf { root_dir().join("src-tauri").join("binaries") }
fn root_dir() -> PathBuf { env::current_dir().unwrap() }
