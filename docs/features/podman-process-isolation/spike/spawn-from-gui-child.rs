#![windows_subsystem = "windows"]
// GUI-subsystem child of a console app: inherits the console's ConDrv std handles but, being a GUI
// image, is never attached to that console. This is exactly sandbox-mio.exe under `build.exe dev`.
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

fn scenario(label: &str, null_stdin: bool, no_window: bool, log: &mut std::fs::File) {
    let out_path = std::env::temp_dir().join(format!("repro3-{label}.out"));
    let so = std::fs::File::create(&out_path).unwrap();
    let se = std::fs::File::create(std::env::temp_dir().join(format!("repro3-{label}.err"))).unwrap();

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
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            let n = std::fs::read(&out_path).unwrap_or_default().len();
            writeln!(log, "{label}: exited {st} in {:?} ({n} bytes)", start.elapsed()).unwrap();
            return;
        }
        if start.elapsed() >= Duration::from_secs(12) {
            writeln!(log, "{label}: HUNG >12s (pid {})", child.id()).unwrap();
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let mut log = std::fs::File::create(path).unwrap();
    unsafe {
        writeln!(log, "attached console window: {:?}", GetConsoleWindow()).unwrap();
        let h = std::io::stdin().as_raw_handle();
        writeln!(log, "inherited stdin {h:?} GetFileType={} (2 = FILE_TYPE_CHAR/console)", GetFileType(h.cast())).unwrap();
    }
    scenario("A-nowindow-INHERIT-stdin", false, true, &mut log);
    scenario("B-nowindow-NULL-stdin", true, true, &mut log);
    scenario("C-plainflags-INHERIT-stdin", false, false, &mut log);
    writeln!(log, "done").unwrap();
}
