// Does podman still work when spawned fully detached from any console?
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Instant;

const DETACHED_PROCESS: u32 = 0x0000_0008;
const PODMAN: &str = r"C:\Users\krist\prog\mi-sandbox\src-tauri\target\debug\podman\podman.exe";

fn main() {
    for (label, flags) in [("DETACHED_PROCESS", DETACHED_PROCESS), ("CREATE_NO_WINDOW", 0x0800_0000)] {
        let out_path = std::env::temp_dir().join(format!("detached-{label}.out"));
        let so = std::fs::File::create(&out_path).unwrap();
        let se = std::fs::File::create(std::env::temp_dir().join(format!("detached-{label}.err"))).unwrap();
        let start = Instant::now();
        let status = Command::new(PODMAN)
            .creation_flags(flags)
            .args(["machine", "list", "--format", "json"])
            .stdin(Stdio::null())
            .stdout(Stdio::from(so))
            .stderr(Stdio::from(se))
            .status()
            .unwrap();
        let out = std::fs::read_to_string(&out_path).unwrap_or_default();
        println!(
            "{label}: {status} in {:?}, {} bytes, contains machine name: {}",
            start.elapsed(),
            out.len(),
            out.contains("podman-machine-default")
        );
    }
}
