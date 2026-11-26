//! The production process layer: how this app spawns every external binary (`podman`, the
//! Mutagen sidecar, `wsl`, `ssh`, ...) and the real [`types::ProcessRunner`] implementations the
//! subsystem structs drive in a shipped build. A crate of its own that depends on no other
//! workspace crate and on no Tauri API, so nothing above it can be reached from here.
//!
//! This file holds the detached-command constructors and the blocking-thread runner; `podman` holds
//! that binary's resolution and real runner, `windows` the Win32 handle FFI. The Mutagen sidecar
//! resolves through Tauri, so its runner lives in `src-tauri/src/mutagen.rs`.
//!
//! Scripted counterparts live in `src-tauri/crates/mockctl/`, which depends on this crate rather
//! than the other way round: the trait describes a production seam that the e2e `light` tier stands
//! in for, not a mock facility production borrows.

pub mod podman;
pub mod types;
pub mod windows;

use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use windows::{duplicate_process_handle, terminate_process};

/// Gives the child a console with **no window**, rather than the no console at all `DETACHED_PROCESS`
/// grants.
///
/// The distinction is not cosmetic for the direct child — that one is invisible either way — but for
/// its grandchildren. A child with no console makes Windows allocate a brand-new *visible* console
/// for every grandchild it spawns with default creation flags, and every binary on this seam spawns
/// one: `podman` → `wsl.exe`/`gvproxy`, Mutagen → its daemon. A
/// packaged build therefore flashed a terminal window per call under `DETACHED_PROCESS`. A windowless
/// console is inherited by those grandchildren instead, so nothing ever becomes visible.
///
/// Safe with respect to the hang this module exists to prevent, on two counts. The flag allocates a
/// *new* console rather than attaching the child to the launching terminal's, so the poisoned console
/// handle is not reachable through it; and creation flags never prevented console-handle inheritance
/// in the first place — only [`windows::clear_inheritable_handles`] does (`inheritance-ab-test.rs` in
/// `docs/features/podman-process-isolation/spike/`). The spike also timed podman under this exact
/// flag with an inherited console stdin and saw no hang (`spike/README.md`, Captured results).
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Builds an already-detached `tokio::process::Command` for `program`: its own windowless console
/// ([`CREATE_NO_WINDOW`]) instead of this app's, and stdin bound to the null device so the child can
/// never read from a terminal this app may have inherited. stdout and stderr are left overridable,
/// because `listen_events` (`src-tauri/src/backends/mod.rs`) needs piped stdout for streaming.
///
/// A constructor rather than a modifier, deliberately: a modifier is a convention no compiler
/// enforces, and a call site that omits it silently reintroduces the hang this module exists to
/// prevent. Never call `Command::new` for an external binary outside this function and its
/// [`detached_command_std`] twin.
pub fn detached_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW).stdin(std::process::Stdio::null());
    cmd
}

/// [`detached_command`]'s `std::process` twin, for callers that need a blocking `Child` rather than
/// `tokio::process`'s async one ([`run_via_blocking_process`]).
pub fn detached_command_std(
    program: impl AsRef<std::ffi::OsStr>,
) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, CREATE_NO_WINDOW);
    cmd.stdin(std::process::Stdio::null());
    cmd
}

/// Applies the same detachment contract to a `Command` this app did not construct itself.
///
/// Exists for exactly one foreign constructor: `tauri_plugin_shell`'s sidecar `Command`, which
/// hard-codes [`CREATE_NO_WINDOW`] and piped stdio and exposes no hook to override either, but does
/// convert back to a `std::process::Command`. The flag is re-applied rather than relied upon, so the
/// contract holds if the plugin ever changes it. Every command this app builds itself must go through
/// [`detached_command`] instead.
pub fn detach(mut cmd: std::process::Command) -> tokio::process::Command {
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, CREATE_NO_WINDOW);
    cmd.stdin(std::process::Stdio::null());
    tokio::process::Command::from(cmd)
}

/// The resource monitor's `podman stats` call (`src-tauri/crates/provider/src/lib.rs`).
pub const PODMAN_STATS_ARGV: [&str; 4] = ["stats", "--no-stream", "--format", "{{json .}}"];

/// The resource monitor's machine-size call. A template distinct from the `info --format json`
/// other callers use, so only the monitor's call takes the quiet, breaker-exempt path.
pub const PODMAN_POOL_ARGV: [&str; 3] = ["info", "--format", "{{json .Host}}"];

/// Whether `args` is one of the resource monitor's call shapes, which log a success at `debug` and
/// never trip or close the podman breaker
/// (`docs/features/sandbox-resource-monitor/02_dev.md`, Resolved Decisions 7 and 8).
pub fn is_monitor_call(args: &[&str]) -> bool {
    let stripped = strip_connection(args);
    stripped == PODMAN_STATS_ARGV || stripped == PODMAN_POOL_ARGV
}

/// Strips a leading `--connection <name>` pair (present on most `backends`/`runtime` calls,
/// absent on `machine`'s calls).
pub fn strip_connection<'a>(args: &'a [&'a str]) -> &'a [&'a str] {
    match args {
        ["--connection", _, rest @ ..] => rest,
        _ => args,
    }
}

static CAPTURE_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Runs `cmd` (plus `args`) on a plain OS thread and bounds it with `timeout`, capturing
/// stdout/stderr via temp files rather than pipes: a detached grandchild inheriting a piped
/// handle would otherwise hang `wait_with_output()` forever (even a bare `podman machine list`
/// did). Reading a file after the direct child exits doesn't care whether anything else still has
/// it open.
///
/// The file-redirect alone turned out not to be sufficient on every machine: even with output
/// captured via files, `tokio::process`'s async spawn/wait has been observed to hang on a bare
/// `podman machine list` under `cargo tauri dev` specifically — the same Windows Job Object
/// `tauri-cli` wraps this app's process tree in appears to also
/// interfere with `tokio::process`'s own OS-level exit-wait registration, not just detached
/// grandchildren. A plain `std::thread::spawn` + blocking `std::process::Command::status()` never
/// touches `tokio::process` or the tokio blocking pool, so it sidesteps that regardless of cause.
/// `run_host_wsl` in `machine/mod.rs` uses the same blocking pattern (via `spawn_blocking`) for the
/// same reason.
///
/// The blocking thread hands a duplicate of the child's process handle back before it blocks, so a
/// child that outlives `timeout` is terminated rather than abandoned. Terminating it also releases
/// the blocking wait, which is what lets the OS thread exit too.
///
/// `stdin`, when given, is a pipe a writer thread fills and then closes. Its EOF never waits on a
/// grandchild, because this app is the only writer. A grandchild holding the read end can block a
/// write larger than the pipe buffer, so the wait does not depend on the writer: a writer still
/// blocked once the child exits is detached and its outcome ignored.
async fn run_via_blocking_process(
    mut cmd: std::process::Command,
    args: &[&str],
    label: &str,
    timeout: Duration,
    stdin: Option<&[u8]>,
) -> std::io::Result<Output> {
    let id = CAPTURE_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let stem = format!("sandbox-mio-{label}-{}-{id}", std::process::id());
    let stdout_path = std::env::temp_dir().join(format!("{stem}.stdout"));
    let stderr_path = std::env::temp_dir().join(format!("{stem}.stderr"));
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    // Owned because the thread outlives this call when the timeout fires.
    let stdin_owned: Option<Vec<u8>> = stdin.map(<[u8]>::to_vec);

    let (tx, rx) = tokio::sync::oneshot::channel();
    let (child_tx, mut child_rx) = tokio::sync::oneshot::channel();
    let thread_stdout_path = stdout_path.clone();
    let thread_stderr_path = stderr_path.clone();
    std::thread::spawn(move || {
        let spawned = (|| -> std::io::Result<std::process::Child> {
            let stdout_file = std::fs::File::create(&thread_stdout_path)?;
            let stderr_file = std::fs::File::create(&thread_stderr_path)?;
            if stdin_owned.is_some() {
                cmd.stdin(std::process::Stdio::piped());
            }
            cmd.args(&args_owned)
                .stdout(std::process::Stdio::from(stdout_file))
                .stderr(std::process::Stdio::from(stderr_file))
                .spawn()
        })();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                let _ = tx.send(Err(e));
                return;
            }
        };
        // Sent before the stdin write and the wait, not after: a write blocked on a stalled child
        // is released only by the timeout branch terminating it with this handle.
        let _ = child_tx.send(duplicate_process_handle(&child));
        // Written off this thread so a grandchild holding the read end cannot keep the wait from
        // seeing the child exit.
        let writer = match (child.stdin.take(), stdin_owned) {
            (Some(mut pipe), Some(bytes)) => Some(std::thread::spawn(move || {
                use std::io::Write;
                match pipe.write_all(&bytes) {
                    // A child that exits without draining stdin is reported by its exit status.
                    Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Some(e),
                    _ => None,
                }
            })),
            _ => None,
        };
        let waited = child.wait();
        // A writer still blocked after the child exited is stuck on a grandchild: the exit status wins.
        let write_error = writer.filter(|w| w.is_finished()).and_then(|w| w.join().ok().flatten());
        let outcome = match (write_error, waited) {
            (Some(e), _) => Err(e),
            (None, waited) => waited.map(|status| Output {
                status,
                stdout: std::fs::read(&thread_stdout_path).unwrap_or_default(),
                stderr: std::fs::read(&thread_stderr_path).unwrap_or_default(),
            }),
        };
        let _ = tx.send(outcome);
    });

    let result = match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => Err(std::io::Error::other("blocking process thread dropped without a result")),
        Err(_) => {
            if let Ok(Some(handle)) = child_rx.try_recv() {
                terminate_process(&handle);
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("process timed out after {timeout:?}"),
            ))
        }
    };
    let _ = std::fs::remove_file(&stdout_path);
    let _ = std::fs::remove_file(&stderr_path);
    result
}

#[cfg(windows)]
pub fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code as u32)
}

#[cfg(unix)]
pub fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code)
}

/// The sanitiser strips inheritance from every handle the process owns, so it must not run while
/// another test is spawning a child with piped or file stdio.
#[cfg(all(test, windows))]
static SPAWN_GATE: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());

#[cfg(all(test, windows))]
mod tests {
    use super::SPAWN_GATE;

    fn system32(exe: &str) -> std::process::Command {
        // Absolute path so a GNU `sort` earlier on PATH cannot shadow the Windows one.
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        super::detached_command_std(format!(r"{root}\System32\{exe}"))
    }

    #[tokio::test]
    async fn stdin_larger_than_the_pipe_buffer_round_trips() {
        let _gate = SPAWN_GATE.read().await;
        let payload: String = (0..20_000).map(|n| format!("{n:06}\r\n")).collect();
        // `sort` reads all of stdin before writing, so a write/wait deadlock would time out.
        let out = super::run_via_blocking_process(
            system32("sort.exe"),
            &[],
            "stdin-test",
            std::time::Duration::from_secs(30),
            Some(payload.as_bytes()),
        )
        .await
        .expect("run");
        assert!(out.status.success());
        assert_eq!(out.stdout.len(), payload.len());
        assert!(out.stdout == payload.as_bytes(), "sorted output differs from the input");
    }

    #[tokio::test]
    async fn a_child_that_ignores_stdin_still_reports_its_exit() {
        let _gate = SPAWN_GATE.read().await;
        let payload = vec![b'x'; 1 << 20];
        let started = std::time::Instant::now();
        let out = super::run_via_blocking_process(
            system32("cmd.exe"),
            &["/c", "exit 3"],
            "stdin-test",
            std::time::Duration::from_secs(30),
            Some(&payload),
        )
        .await
        .expect("a broken pipe must not surface as an error");
        assert_eq!(out.status.code(), Some(3));
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[tokio::test]
    async fn a_stalled_child_is_killed_at_the_timeout_while_stdin_is_blocked() {
        use std::os::windows::process::CommandExt;
        let _gate = SPAWN_GATE.read().await;
        let payload = vec![b'x'; 1 << 20];
        let marker = std::env::temp_dir().join(format!("mio-stdin-kill-{}.marker", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let mut cmd = system32("cmd.exe");
        // `<nul` keeps ping off the stdin pipe, so killing cmd alone closes the read end.
        cmd.raw_arg(format!("/d /c \"ping -n 5 127.0.0.1 >nul <nul & type nul > \"{}\"\"", marker.display()));
        let err = super::run_via_blocking_process(
            cmd,
            &[],
            "stdin-test",
            std::time::Duration::from_secs(2),
            Some(&payload),
        )
        .await
        .expect_err("a child that never reads stdin must time out");
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        // Past ping's natural end: a surviving cmd would have written the marker by now.
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let survived = marker.exists();
        let _ = std::fs::remove_file(&marker);
        assert!(!survived, "the timed-out child was not terminated");
    }

    #[tokio::test]
    async fn a_grandchild_holding_stdin_does_not_stall_a_child_that_exited() {
        use std::os::windows::process::CommandExt;
        let _gate = SPAWN_GATE.read().await;
        let payload = vec![b'x'; 1 << 20];
        let mut cmd = system32("cmd.exe");
        // The background ping inherits stdin's read end and outlives cmd; `>nul` frees stdout/stderr.
        cmd.raw_arg("/d /c \"start /b ping -n 8 127.0.0.1 >nul 2>&1\"");
        let started = std::time::Instant::now();
        let out = super::run_via_blocking_process(
            cmd,
            &[],
            "stdin-test",
            std::time::Duration::from_secs(30),
            Some(&payload),
        )
        .await
        .expect("the exit status must win over a writer blocked on a grandchild");
        assert!(out.status.success());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the call waited for the grandchild: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn empty_stdin_gives_the_child_eof() {
        let _gate = SPAWN_GATE.read().await;
        let out = super::run_via_blocking_process(
            system32("sort.exe"),
            &[],
            "stdin-test",
            std::time::Duration::from_secs(30),
            Some(&[]),
        )
        .await
        .expect("run");
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
    }
}
