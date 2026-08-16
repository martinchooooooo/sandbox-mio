// Clears HANDLE_FLAG_INHERIT on every inheritable handle the process owns, then verifies that
// std::process::Command can still hand a child explicit stdio afterwards.
use std::convert::TryInto;
use std::ffi::c_void;
use std::process::{Command, Stdio};
use std::time::Instant;

type H = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct HandleEntry {
    object: usize,
    unique_process_id: usize,
    handle_value: usize,
    granted_access: u32,
    creator_back_trace_index: u16,
    object_type_index: u16,
    handle_attributes: u32,
    reserved: u32,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
}
extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn GetHandleInformation(h: H, flags: *mut u32) -> i32;
    fn SetHandleInformation(h: H, mask: u32, flags: u32) -> i32;
}

const HANDLE_FLAG_INHERIT: u32 = 0x1;
const PODMAN: &str = r"C:\Users\krist\prog\mi-sandbox\src-tauri\target\debug\podman\podman.exe";
const DETACHED_PROCESS: u32 = 0x0000_0008;

fn sanitize() -> (usize, usize) {
    let pid = unsafe { GetCurrentProcessId() };
    let mut size = 1 << 20;
    let mut buf;
    loop {
        buf = vec![0u8; size];
        let mut ret = 0u32;
        let st = unsafe { NtQuerySystemInformation(0x40, buf.as_mut_ptr().cast(), size as u32, &mut ret) };
        if st == 0 {
            break;
        }
        size *= 2;
        assert!(size < (1 << 30));
    }
    let count = usize::from_le_bytes(buf[0..8].try_into().unwrap());
    let entries = unsafe { std::slice::from_raw_parts(buf.as_ptr().add(16) as *const HandleEntry, count) };
    let (mut found, mut cleared) = (0, 0);
    for e in entries.iter().filter(|e| e.unique_process_id as u32 == pid) {
        let h = e.handle_value as H;
        let mut flags = 0u32;
        if unsafe { GetHandleInformation(h, &mut flags) } == 0 {
            continue;
        }
        if flags & HANDLE_FLAG_INHERIT != 0 {
            found += 1;
            if unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0) } != 0 {
                cleared += 1;
            }
        }
    }
    (found, cleared)
}

fn spawn_podman(label: &str) {
    let out_path = std::env::temp_dir().join(format!("sanitize-{label}.out"));
    let so = std::fs::File::create(&out_path).unwrap();
    let se = std::fs::File::create(std::env::temp_dir().join(format!("sanitize-{label}.err"))).unwrap();
    use std::os::windows::process::CommandExt;
    let start = Instant::now();
    let status = Command::new(PODMAN)
        .creation_flags(DETACHED_PROCESS)
        .args(["machine", "list", "--format", "json"])
        .stdin(Stdio::null())
        .stdout(Stdio::from(so))
        .stderr(Stdio::from(se))
        .status()
        .unwrap();
    let out = std::fs::read_to_string(&out_path).unwrap_or_default();
    println!(
        "  {label}: {status} in {:?}, {} bytes, machine name present: {}",
        start.elapsed(),
        out.len(),
        out.contains("podman-machine-default")
    );
}

fn main() {
    println!("before sanitise:");
    spawn_podman("before");
    let (found, cleared) = sanitize();
    println!("sanitised: {found} inheritable handles found, {cleared} cleared");
    println!("after sanitise:");
    spawn_podman("after");
    spawn_podman("after-second");
}
