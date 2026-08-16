// Can stable std restrict child handle inheritance to an explicit list?
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Instant;

const DETACHED_PROCESS: u32 = 0x0000_0008;
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;
const PODMAN: &str = r"C:\Users\krist\prog\mi-sandbox\src-tauri\target\debug\podman\podman.exe";

fn main() {
    let out_path = std::env::temp_dir().join("handlelist.out");
    let so = std::fs::File::create(&out_path).unwrap();
    let se = std::fs::File::create(std::env::temp_dir().join("handlelist.err")).unwrap();
    let null = std::fs::OpenOptions::new().read(true).open("NUL").unwrap();

    // The list must include every handle the child is given as stdio, or the child gets none.
    let handles: Vec<isize> = vec![
        null.as_raw_handle() as isize,
        so.as_raw_handle() as isize,
        se.as_raw_handle() as isize,
    ];

    let start = Instant::now();
    let status = Command::new(PODMAN)
        .creation_flags(DETACHED_PROCESS | EXTENDED_STARTUPINFO_PRESENT)
        .raw_attribute(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, handles)
        .args(["machine", "list", "--format", "json"])
        .stdin(Stdio::from(null))
        .stdout(Stdio::from(so))
        .stderr(Stdio::from(se))
        .status()
        .unwrap();

    let out = std::fs::read_to_string(&out_path).unwrap_or_default();
    println!(
        "{status} in {:?}, {} bytes, machine name present: {}",
        start.elapsed(),
        out.len(),
        out.contains("podman-machine-default")
    );
}
