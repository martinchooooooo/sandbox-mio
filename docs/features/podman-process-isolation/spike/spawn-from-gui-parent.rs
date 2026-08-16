#![windows_subsystem = "windows"]

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PODMAN: &str = r"C:\Users\krist\prog\mi-sandbox\src-tauri\target\debug\podman\podman.exe";
const DIR: &str = r"C:\Users\krist\prog\mi-sandbox\src-tauri\target\debug\podman";

fn scenario(label: &str, no_window: bool, null_stdin: bool, helper_dir: bool, log: &mut std::fs::File) {
    let out_path = std::env::temp_dir().join(format!("repro-{label}.out"));
    let err_path = std::env::temp_dir().join(format!("repro-{label}.err"));
    let so = std::fs::File::create(&out_path).unwrap();
    let se = std::fs::File::create(&err_path).unwrap();

    let mut cmd = Command::new(PODMAN);
    if helper_dir {
        cmd.env("CONTAINERS_HELPER_BINARY_DIR", DIR);
    }
    if no_window {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.args(["machine", "list", "--format", "json"]);
    cmd.stdout(Stdio::from(so)).stderr(Stdio::from(se));
    if null_stdin {
        cmd.stdin(Stdio::null());
    }

    let start = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            writeln!(log, "{label}: SPAWN FAILED {e}").unwrap();
            return;
        }
    };
    let mut waited = Duration::ZERO;
    let limit = Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let bytes = std::fs::read(&out_path).unwrap_or_default();
                writeln!(
                    log,
                    "{label}: exited {st} in {:?}, stdout {} bytes",
                    start.elapsed(),
                    bytes.len()
                )
                .unwrap();
                return;
            }
            Ok(None) => {}
            Err(e) => {
                writeln!(log, "{label}: wait error {e}").unwrap();
                return;
            }
        }
        if waited >= limit {
            writeln!(log, "{label}: HUNG >20s (pid {})", child.id()).unwrap();
            let _ = child.kill();
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
        waited += Duration::from_millis(200);
    }
}

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let mut log = std::fs::File::create(path).unwrap();
    writeln!(log, "pid {}", std::process::id()).unwrap();
    scenario("A-nowindow-inheritstdin-helper", true, false, true, &mut log);
    scenario("B-nowindow-nullstdin-helper", true, true, true, &mut log);
    scenario("C-plain-inheritstdin-helper", false, false, true, &mut log);
    scenario("D-nowindow-nullstdin-nohelper", true, true, false, &mut log);
    writeln!(log, "done").unwrap();
}
