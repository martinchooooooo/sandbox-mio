use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("dev") if args.get(2).map(String::as_str) == Some("--setup") => {
            run(root_dir(), "pnpm", &["install"]);
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
fn root_dir() -> PathBuf { env::current_dir().unwrap() }
