// A/B: spawn a long-lived child before and after clearing HANDLE_FLAG_INHERIT, so an external
// handle dump can compare which console handles each child inherited.
include!("sanitize_lib.rs");

use std::os::windows::process::CommandExt;

fn sleeper(label: &str) -> u32 {
    let so = std::fs::File::create(std::env::temp_dir().join(format!("ab-{label}.out"))).unwrap();
    let se = std::fs::File::create(std::env::temp_dir().join(format!("ab-{label}.err"))).unwrap();
    let child = Command::new("cmd")
        .creation_flags(0x0000_0008) // DETACHED_PROCESS
        .args(["/c", "ping", "-n", "30", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::from(so))
        .stderr(Stdio::from(se))
        .spawn()
        .unwrap();
    let pid = child.id();
    println!("{label} child pid {pid}");
    std::mem::forget(child);
    pid
}

fn main() {
    sleeper("BEFORE-sanitise");
    let (found, cleared) = sanitize();
    println!("sanitised: {found} found, {cleared} cleared");
    sleeper("AFTER-sanitise");
    println!("both children live for ~30s");
}
