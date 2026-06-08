use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    setup_e2e();
}

fn setup_e2e() {
    let tools_dir = PathBuf::from(env::current_dir().unwrap()).join("tools");

    run("cargo", &["install", "tauri-driver", "--locked"]);
    if cfg!(windows) {
        run(
            "cargo",
            &[
                "install",
                "--git",
                "https://github.com/chippers/msedgedriver-tool",
            ],
        );
    }

    fs::create_dir_all(&tools_dir).unwrap();

    let prev = env::current_dir().unwrap();
    env::set_current_dir(&tools_dir).unwrap();

    let driver = if cfg!(windows) {
        "msedgedriver-tool.exe"
    } else {
        todo!()
    };
    run(driver, &[]);

    env::set_current_dir(prev).unwrap();
}

fn run(cmd: &str, args: &[&str]) {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("failed to run `{}`: {}", cmd, e));
    assert!(status.success(), "`{}` exited with {}", cmd, status);
}
