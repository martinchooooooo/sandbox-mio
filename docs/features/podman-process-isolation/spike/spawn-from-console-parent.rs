// Console-subsystem parent: inherits the launching PowerShell console, exactly like
// sandbox-mio.exe does when `build.exe dev` starts it from a PowerShell window.
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PODMAN: &str = r"C:\Users\krist\prog\mi-sandbox\src-tauri\target\debug\podman\podman.exe";

extern "system" {
    fn GetConsoleWindow() -> *mut std::ffi::c_void;
    fn GetFileType(h: *mut std::ffi::c_void) -> u32;
}

fn scenario(label: &str, null_stdin: bool, no_window: bool) {
    let out_path = std::env::temp_dir().join(format!("repro2-{label}.out"));
    let so = std::fs::File::create(&out_path).unwrap();
    let se = std::fs::File::create(std::env::temp_dir().join(format!("repro2-{label}.err"))).unwrap();

    let mut cmd = Command::new(PODMAN);
    if no_window {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.args(["machine", "list", "--format", "json"]);
    cmd.stdout(Stdio::from(so)).stderr(Stdio::from(se));
    if null_stdin {
        cmd.stdin(Stdio::null());
    }

    let start = Instant::now();
    let mut child = cmd.spawn().unwrap();
    let limit = Duration::from_secs(15);
    loop {
        match child.try_wait().unwrap() {
            Some(st) => {
                let n = std::fs::read(&out_path).unwrap_or_default().len();
                println!("{label}: exited {st} in {:?} ({n} bytes)", start.elapsed());
                std::io::stdout().flush().unwrap();
                return;
            }
            None => {
                if start.elapsed() >= limit {
                    println!("{label}: HUNG >15s (pid {})", child.id());
                    std::io::stdout().flush().unwrap();
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn main() {
    unsafe {
        println!("console window: {:?}", GetConsoleWindow());
        let stdin_h = std::io::stdin().as_raw_handle();
        // FILE_TYPE_CHAR == 2 means a real console handle.
        println!("stdin handle {stdin_h:?} GetFileType={}", GetFileType(stdin_h.cast()));
    }
    scenario("A-nowindow-inherit-stdin", false, true);
    scenario("B-nowindow-null-stdin", true, true);
    scenario("C-window-inherit-stdin", false, false);
}
