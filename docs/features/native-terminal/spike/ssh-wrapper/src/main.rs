// Spike-only shim: forwards to the Windows OpenSSH client with a private config (-F), so the
// private Mutagen daemon resolves the spike's aliases without touching ~/.ssh/config.
// Invoked as ssh.exe or scp.exe; the real binary is chosen from this exe's own file name.
use std::process::{Command, exit};

fn main() {
    let exe = std::env::current_exe().unwrap();
    let stem = exe.file_stem().unwrap().to_string_lossy().to_string();
    let cfg = std::env::var("SPIKE_SSH_CONFIG").expect("SPIKE_SSH_CONFIG");
    let real = String::from(r"C:\Windows\System32\OpenSSH\") + &stem + ".exe";
    let status = Command::new(real)
        .arg("-F").arg(cfg)
        .args(std::env::args().skip(1))
        .status().unwrap();
    exit(status.code().unwrap_or(1));
}
