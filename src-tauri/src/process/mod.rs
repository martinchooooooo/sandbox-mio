//! The production process layer: how this app spawns every external binary (`podman`, `msb`, the
//! Mutagen sidecar, `wsl`, `ssh`, ...) and the real [`ProcessRunner`] implementations the subsystem
//! structs drive in a shipped build.
//!
//! Scripted counterparts live in `src-tauri/src/mockctl/`, which depends on this module rather than
//! the other way round: the trait describes a production seam that the e2e `light` tier stands in
//! for, not a mock facility production borrows.

pub mod types;

use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use types::{ProcessRunner, SystemMsbRunner, SystemMutagenRunner, SystemPodmanRunner};

/// Gives the child a console with **no window**, rather than the no console at all `DETACHED_PROCESS`
/// grants.
///
/// The distinction is not cosmetic for the direct child — that one is invisible either way — but for
/// its grandchildren. A child with no console makes Windows allocate a brand-new *visible* console
/// for every grandchild it spawns with default creation flags, and every binary on this seam spawns
/// one: `podman` → `wsl.exe`/`gvproxy`, `msb` → its detached VM supervisor, Mutagen → its daemon. A
/// packaged build therefore flashed a terminal window per call under `DETACHED_PROCESS`. A windowless
/// console is inherited by those grandchildren instead, so nothing ever becomes visible.
///
/// Safe with respect to the hang this module exists to prevent, on two counts. The flag allocates a
/// *new* console rather than attaching the child to the launching terminal's, so the poisoned console
/// handle is not reachable through it; and creation flags never prevented console-handle inheritance
/// in the first place — only [`clear_inheritable_handles`] does (`inheritance-ab-test.rs` in
/// `docs/features/podman-process-isolation/spike/`). The spike also timed podman under this exact
/// flag with an inherited console stdin and saw no hang (`spike/README.md`, Captured results).
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Clears `HANDLE_FLAG_INHERIT` on every inheritable handle this process already owns, so no child
/// can inherit a console handle the launching terminal handed us. Returns how many handles were
/// cleared.
///
/// Load-bearing and ordering-sensitive: creation flags and fully explicit stdio do **not** prevent
/// console-handle inheritance — not even `DETACHED_PROCESS`, the strictest of them
/// (`inheritance-ab-test.rs` in
/// `docs/features/podman-process-isolation/spike/`), and a child spawned before this runs still
/// inherits the poisoned handles — so this must be the first statement of `run()`
/// (`src-tauri/src/lib.rs`).
///
/// Governs only handles present at call time; one created later by a dependency is out of reach
/// (`docs/features/podman-process-isolation/04_improvements.md`).
#[cfg(windows)]
pub(crate) fn clear_inheritable_handles() -> usize {
    let pid = unsafe { GetCurrentProcessId() };
    let Some(buf) = system_handle_snapshot() else {
        return 0;
    };
    // SYSTEM_HANDLE_INFORMATION_EX: a `ULONG_PTR` count, a `ULONG_PTR` reserved, then the entries.
    let count = buf[0] as usize;
    let entries =
        unsafe { std::slice::from_raw_parts(buf.as_ptr().add(2) as *const HandleEntry, count) };
    let mut cleared = 0;
    for entry in entries.iter().filter(|e| e.unique_process_id as u32 == pid) {
        let handle = entry.handle_value as *mut std::ffi::c_void;
        let mut flags = 0u32;
        if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
            continue;
        }
        if flags & HANDLE_FLAG_INHERIT != 0
            && unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } != 0
        {
            cleared += 1;
        }
    }
    cleared
}

#[cfg(not(windows))]
pub(crate) fn clear_inheritable_handles() -> usize {
    0
}

#[cfg(windows)]
const HANDLE_FLAG_INHERIT: u32 = 0x1;

/// `SystemExtendedHandleInformation`.
#[cfg(windows)]
const SYSTEM_EXTENDED_HANDLE_INFORMATION: u32 = 0x40;

#[cfg(windows)]
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004u32 as i32;

/// `SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX`.
#[cfg(windows)]
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

#[cfg(windows)]
#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(
        class: u32,
        info: *mut std::ffi::c_void,
        len: u32,
        ret: *mut u32,
    ) -> i32;
}

#[cfg(windows)]
extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn GetHandleInformation(handle: *mut std::ffi::c_void, flags: *mut u32) -> i32;
    fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
}

/// The system-wide handle table, or `None` if the query never succeeded. `u64`-backed so the entry
/// slice is pointer-aligned, which a `Vec<u8>` does not guarantee. The table grows between the
/// size query and the read, so the doubling retry is the documented way to call this — bounded
/// here rather than open-ended, since failing to sanitise is survivable but hanging startup is not.
#[cfg(windows)]
fn system_handle_snapshot() -> Option<Vec<u64>> {
    let mut words = 1 << 17;
    for _ in 0..8 {
        let mut buf = vec![0u64; words];
        let mut needed = 0u32;
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_EXTENDED_HANDLE_INFORMATION,
                buf.as_mut_ptr().cast(),
                (words * 8) as u32,
                &mut needed,
            )
        };
        if status == 0 {
            return Some(buf);
        }
        if status != STATUS_INFO_LENGTH_MISMATCH {
            return None;
        }
        words *= 2;
    }
    None
}

/// Builds an already-detached `tokio::process::Command` for `program`: its own windowless console
/// ([`CREATE_NO_WINDOW`]) instead of this app's, and stdin bound to the null device so the child can
/// never read from a terminal this app may have inherited. stdout and stderr are left overridable,
/// because `listen_events` (`src-tauri/src/backends/mod.rs`) needs piped stdout for streaming.
///
/// A constructor rather than a modifier, deliberately: a modifier is a convention no compiler
/// enforces, and a call site that omits it silently reintroduces the hang this module exists to
/// prevent. Never call `Command::new` for an external binary outside this function and its
/// [`detached_command_std`] twin.
pub(crate) fn detached_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW).stdin(std::process::Stdio::null());
    cmd
}

/// [`detached_command`]'s `std::process` twin, for callers that need a blocking `Child` rather than
/// `tokio::process`'s async one ([`run_via_blocking_process`]).
pub(crate) fn detached_command_std(
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
pub(crate) fn detach(mut cmd: std::process::Command) -> tokio::process::Command {
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, CREATE_NO_WINDOW);
    cmd.stdin(std::process::Stdio::null());
    tokio::process::Command::from(cmd)
}

/// Bound on the common shape of `podman` CLI call (`list`/`exists`/`info`/`ps`/`start`/`stop`/`rm`/
/// `exec`/...). Talking to a stopped machine's connection has been observed to hang indefinitely
/// rather than fail fast (confirmed: the identical argv run standalone against the same stopped
/// machine returns in well under a second).
///
/// A user watching a spinner reads anything past this as the app being broken, not "still working"
/// — this is a UX bound, not just a hang guard, which is why `machine start` and `build` (the two
/// calls on this seam that are legitimately slow, not stuck) get their own longer allowances below
/// instead of inheriting it. See [`call_timeout`].
const PODMAN_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// `podman machine start`: a cold WSL boot after the machine has sat stopped for days can take well
/// over a minute — confirmed in this codebase's own experience, not a guess. Long enough to cover
/// that, short enough that a truly stuck start still surfaces well inside a coffee break rather
/// than the old blanket 300s.
const MACHINE_START_TIMEOUT: Duration = Duration::from_secs(150);

/// `podman build` of the base image: installing mise, zellij, and apt packages easily exceeds 20s
/// and can run longer over a slow network or disk — bounded generously, but not open-ended.
const BUILD_TIMEOUT: Duration = Duration::from_secs(180);

/// Picks the timeout for `args` by call shape — `machine start` and `build` are the only calls on
/// this seam that are legitimately slow rather than stuck; everything else, including `machine`'s
/// other subcommands, gets the tight default. Connection-stripped so a remote `--connection <c>
/// build ...` still matches.
fn call_timeout(args: &[&str]) -> Duration {
    match strip_connection(args) {
        ["machine", "start", ..] => MACHINE_START_TIMEOUT,
        ["build", ..] => BUILD_TIMEOUT,
        _ => PODMAN_CALL_TIMEOUT,
    }
}

/// Strips a leading `--connection <name>` pair (present on most `backends`/`runtime` calls,
/// absent on `machine`'s calls).
pub(crate) fn strip_connection<'a>(args: &'a [&'a str]) -> &'a [&'a str] {
    match args {
        ["--connection", _, rest @ ..] => rest,
        _ => args,
    }
}

/// Bound on the `machine list` probe itself — independent of, and much shorter than,
/// [`PODMAN_CALL_TIMEOUT`], since this is the cheap lookup [`SystemPodmanRunner::run`] uses to
/// decide whether to even attempt a connection-scoped call.
const MACHINE_LIST_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// The exact argv every caller of `podman machine list` in this crate uses. A call that differs
/// (a different `--format`, say) must not be served the shared result, since it expects other
/// output.
const MACHINE_LIST_ARGV: [&str; 4] = ["machine", "list", "--format", "json"];

/// Completed-probe counter, read before the lock is taken so a caller can tell whether the result
/// it finds behind the lock was produced after it started waiting.
static MACHINE_LIST_SEQ: AtomicU64 = AtomicU64::new(0);

type SharedOutput = Arc<std::io::Result<Output>>;

static MACHINE_LIST: LazyLock<tokio::sync::Mutex<Option<(u64, SharedOutput)>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));

/// One `podman machine list`, shared by every caller that asked while it was still in flight.
///
/// Single-flight rather than time-expired: a caller that started waiting before the running probe
/// finished reuses that probe's result, and a caller arriving afterwards runs a fresh one — so
/// concurrent callers collapse into one process (the welcome flow alone fires several at once)
/// without any window in which a stale answer is served to a caller that could have had a fresh
/// one.
///
/// Runs the probe directly against `podman_command_std()` rather than through
/// [`SystemPodmanRunner::run`], so it can never recurse into itself. See
/// [`run_via_blocking_process`] for why this goes through a blocking OS thread.
async fn machine_list() -> SharedOutput {
    let observed = MACHINE_LIST_SEQ.load(Ordering::Acquire);
    let mut guard = MACHINE_LIST.lock().await;
    if let Some((seq, result)) = guard.as_ref() {
        if *seq > observed {
            return result.clone();
        }
    }
    let cmd = crate::backends::podman_command_std();
    let result: SharedOutput = Arc::new(
        run_via_blocking_process(cmd, &MACHINE_LIST_ARGV, "podman-machine-list", MACHINE_LIST_PROBE_TIMEOUT)
            .await,
    );
    let seq = MACHINE_LIST_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    *guard = Some((seq, result.clone()));
    result
}

/// Whether `name` is a known, running machine — `Some(true)`/`Some(false)` when `machine list`
/// named it, `None` when it didn't (unknown connection) or the probe itself failed. `None` is
/// deliberately not treated as "not running": a failed or inconclusive probe must never block a
/// call the old code path would have attempted, only a *confirmed-stopped* machine should.
async fn machine_running_state(name: &str) -> Option<bool> {
    let probe = machine_list().await;
    let out = probe.as_ref().as_ref().ok().filter(|o| o.status.success())?;
    let machines: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).ok()?;
    machines
        .iter()
        .find(|m| m["Name"].as_str() == Some(name))
        .map(|m| m["Running"].as_bool().unwrap_or(false))
}

/// Whether the podman seam is currently believed unresponsive: set by a call that timed out,
/// cleared by any later call that got an answer. Process-local, so nothing is shared with another
/// worktree's run and the `light` tier's `ScriptedPodmanRunner` never sees it.
static PODMAN_UNRESPONSIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The single half-open probe slot. While the breaker is tripped exactly one call at a time is
/// admitted to find out whether podman answers again; every concurrent caller fails immediately.
/// Without the half-open slot a breaker that fails every call could never observe a success, and
/// would latch forever (`docs/features/podman-process-isolation/02_dev.md`, Resolved Decision 2).
static PODMAN_PROBE_SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Copies a shared probe result out for one caller. `std::io::Error` and `Output` are neither
/// `Clone`, so the shared value stays behind the `Arc` and each caller gets its own copy.
fn clone_shared_output(shared: &SharedOutput) -> std::io::Result<Output> {
    match shared.as_ref() {
        Ok(out) => Ok(Output {
            status: out.status,
            stdout: out.stdout.clone(),
            stderr: out.stderr.clone(),
        }),
        Err(e) => Err(std::io::Error::new(e.kind(), e.to_string())),
    }
}

/// The condition Phase E maps to its own error kind: `TimedOut` is the marker the whole seam agrees
/// on, so callers need only test `ErrorKind` rather than match on message text.
fn unresponsive_error(args: &[&str]) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!(
            "podman is not responding — `podman {}` was not attempted because an earlier call \
             never returned",
            args.join(" ")
        ),
    )
}

static CAPTURE_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Runs `cmd` (plus `args`) on a plain OS thread and bounds it with `timeout`, capturing
/// stdout/stderr via temp files rather than pipes — the same file-redirect [`run_capturing_via_files`]
/// uses, and for the same reason (see its doc comment): a detached grandchild inheriting a piped
/// handle would otherwise hang `wait_with_output()` forever.
///
/// This exists as a *second*, independent workaround alongside [`run_capturing_via_files`] because
/// the file-redirect alone turned out not to be sufficient on every machine: even with output
/// captured via files, `tokio::process`'s async spawn/wait has been observed to hang on a bare
/// `podman machine list` under `cargo tauri dev` specifically — the same Windows Job Object
/// `tauri-cli` wraps this app's process tree in (see [`msb_command`]'s doc comment) appears to also
/// interfere with `tokio::process`'s own OS-level exit-wait registration, not just detached
/// grandchildren. A plain `std::thread::spawn` + blocking `std::process::Command::status()` never
/// touches `tokio::process` or the tokio blocking pool, so it sidesteps that regardless of cause.
/// `run_host_wsl` in `machine/mod.rs` uses the same blocking pattern (via `spawn_blocking`) for the
/// same reason.
///
/// The blocking thread hands a duplicate of the child's process handle back before it blocks, so a
/// child that outlives `timeout` is terminated rather than abandoned. Terminating it also releases
/// the blocking wait, which is what lets the OS thread exit too.
async fn run_via_blocking_process(
    mut cmd: std::process::Command,
    args: &[&str],
    label: &str,
    timeout: Duration,
) -> std::io::Result<Output> {
    let id = CAPTURE_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let stem = format!("sandbox-mio-{label}-{}-{id}", std::process::id());
    let stdout_path = std::env::temp_dir().join(format!("{stem}.stdout"));
    let stderr_path = std::env::temp_dir().join(format!("{stem}.stderr"));
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();

    let (tx, rx) = tokio::sync::oneshot::channel();
    let (child_tx, mut child_rx) = tokio::sync::oneshot::channel();
    let thread_stdout_path = stdout_path.clone();
    let thread_stderr_path = stderr_path.clone();
    std::thread::spawn(move || {
        let spawned = (|| -> std::io::Result<std::process::Child> {
            let stdout_file = std::fs::File::create(&thread_stdout_path)?;
            let stderr_file = std::fs::File::create(&thread_stderr_path)?;
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
        // Sent before the wait, not after: a handle delivered once the wait returns would only ever
        // reach the timeout branch too late to be of any use.
        let _ = child_tx.send(duplicate_process_handle(&child));
        let outcome = child.wait().map(|status| Output {
            status,
            stdout: std::fs::read(&thread_stdout_path).unwrap_or_default(),
            stderr: std::fs::read(&thread_stderr_path).unwrap_or_default(),
        });
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

/// An independently owned copy of `child`'s process handle, so the timeout branch can terminate the
/// child without racing the blocking thread's ownership of the [`std::process::Child`] — closing
/// that one out from under the wait would risk terminating an unrelated, recycled handle.
/// Non-inheritable, like every other handle this process holds after [`clear_inheritable_handles`].
#[cfg(windows)]
fn duplicate_process_handle(
    child: &std::process::Child,
) -> Option<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};

    const DUPLICATE_SAME_ACCESS: u32 = 0x2;
    let mut duplicate = std::ptr::null_mut();
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            child.as_raw_handle().cast(),
            GetCurrentProcess(),
            &mut duplicate,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    (ok != 0).then(|| unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(duplicate.cast()) })
}

#[cfg(not(windows))]
fn duplicate_process_handle(_child: &std::process::Child) -> Option<std::convert::Infallible> {
    None
}

/// Kills exactly the child this app spawned, addressed by its own handle — nothing is matched by
/// name, so no other process (another worktree's, say) is reachable from here.
#[cfg(windows)]
fn terminate_process(handle: &std::os::windows::io::OwnedHandle) {
    use std::os::windows::io::AsRawHandle;
    unsafe { TerminateProcess(handle.as_raw_handle().cast(), 1) };
}

#[cfg(not(windows))]
fn terminate_process(_handle: &std::convert::Infallible) {}

#[cfg(windows)]
extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn DuplicateHandle(
        source_process: *mut std::ffi::c_void,
        source: *mut std::ffi::c_void,
        target_process: *mut std::ffi::c_void,
        target: *mut *mut std::ffi::c_void,
        desired_access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn TerminateProcess(process: *mut std::ffi::c_void, exit_code: u32) -> i32;
}

/// Runs `cmd`, capturing stdout/stderr via temp files instead of the OS pipes `.output()` would use.
///
/// A successful `msb create` spawns a long-running detached VM supervisor that inherits `msb.exe`'s
/// own stdout/stderr handles — `msb` does not appear to redirect them away for its own Windows
/// daemonization. A piped `wait_with_output()` (what `.output()` does under the hood) reads until
/// EOF, which never comes while that supervisor keeps its inherited duplicate of the pipe open, so
/// every `msb create` hangs forever even though `msb.exe` itself already exited 0 and the sandbox is
/// already running (confirmed hands-on: `msb ls` shows the new sandbox while the hung call still
/// blocks). Reading a file after the direct child exits doesn't care whether some other process
/// still has it open, so redirecting there instead sidesteps the hang entirely.
///
/// The same symptom was later confirmed on the Podman side too: even a bare `podman machine list`
/// (near-instant run standalone) hung the full length of `SystemPodmanRunner`'s timeout when spawned
/// this way from inside the app — `podman.exe` on Windows evidently spawns something that inherits
/// the pipe handles on every call, not just `machine`/`create`-shaped ones. `label` namespaces the
/// temp-file stem per caller (`"msb"`, `"podman"`) so concurrent calls from both never collide.
///
/// Unbounded by design for now: see `docs/features/podman-process-isolation/02_dev.md`, Resolved
/// Decision 4.
async fn run_capturing_via_files(
    mut cmd: tokio::process::Command,
    args: &[&str],
    label: &str,
) -> std::io::Result<Output> {
    let id = CAPTURE_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let stem = format!("sandbox-mio-{label}-{}-{id}", std::process::id());
    let stdout_path = std::env::temp_dir().join(format!("{stem}.stdout"));
    let stderr_path = std::env::temp_dir().join(format!("{stem}.stderr"));

    let result = async {
        let stdout_file = std::fs::File::create(&stdout_path)?;
        let stderr_file = std::fs::File::create(&stderr_path)?;
        let status = cmd
            .args(args)
            .stdout(std::process::Stdio::from(stdout_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .status()
            .await?;
        Ok(Output {
            status,
            stdout: std::fs::read(&stdout_path).unwrap_or_default(),
            stderr: std::fs::read(&stderr_path).unwrap_or_default(),
        })
    }
    .await;

    let _ = std::fs::remove_file(&stdout_path);
    let _ = std::fs::remove_file(&stderr_path);
    result
}

#[async_trait::async_trait]
impl ProcessRunner for SystemPodmanRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        self.run_with_env(args, &[]).await
    }

    /// `env` lands in the child's environment only, never in `args`, so the argv logged below
    /// stays free of secret values (`docs/features/techdebt-sweep/02_dev.md`, Resolved Decision 4).
    async fn run_with_env(
        &self,
        args: &[&str],
        env: &[(String, String)],
    ) -> std::io::Result<Output> {
        // Every connection-scoped call (`image exists`, `create`, `exec`, `info`, ...) passes
        // through here — the one seam behind `PodmanProvider::run` and `PodmanMachineManager`'s
        // `podman`/`verify_host` alike — so a guard installed here covers every current and future
        // caller, rather than relying on each call site to remember to check first. `machine ...`
        // commands themselves carry no `--connection` and are unaffected.
        if let ["--connection", name, ..] = args {
            if machine_running_state(name).await == Some(false) {
                let e = std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    format!(
                        "podman connection {name:?} points at a machine that is not running \
                         (stopped or deleted) — run setup again before retrying `podman {}`",
                        args.join(" ")
                    ),
                );
                tracing::warn!(target: "podman", argv = %args.join(" "), error = %e, "podman guard rejected: machine not running");
                return Err(e);
            }
        }

        // While the breaker is tripped only the half-open probe proceeds; everyone else is failed
        // now rather than each paying another full timeout for the same broken seam.
        let _probe = if PODMAN_UNRESPONSIVE.load(Ordering::Acquire) {
            match PODMAN_PROBE_SLOT.try_acquire() {
                Ok(permit) => Some(permit),
                Err(_) => {
                    let e = unresponsive_error(args);
                    tracing::warn!(target: "podman", argv = %args.join(" "), "podman breaker open: call not attempted");
                    return Err(e);
                }
            }
        } else {
            None
        };

        // Every real Podman invocation — connection-scoped or not, from `PodmanProvider`,
        // `PodmanMachineManager`, or `verify_host` alike — passes through here, so logging it once
        // in this one seam covers every caller. Before this, a stuck or slow Podman call was
        // invisible in `mio.log`: nothing on the call path ever wrote a line, so a genuine hang and
        // a call that simply hadn't been invoked yet looked identical from the log alone.
        let started = std::time::Instant::now();
        tracing::info!(target: "podman", argv = %args.join(" "), "podman starting");
        let timeout_dur = call_timeout(args);
        let result = if strip_connection(args) == MACHINE_LIST_ARGV {
            // The one call several callers fire at once (the welcome flow's cards, the projects-page
            // probe), so it collapses into a single shared process instead of one each.
            clone_shared_output(&machine_list().await)
        } else {
            // Blocking-thread + file-captured, not `tokio::process`: see `run_via_blocking_process`'s
            // doc comment — `tokio::process`'s async spawn/wait has been observed to hang the full
            // timeout even on a bare `podman machine list` under `cargo tauri dev`.
            let mut cmd = crate::backends::podman_command_std();
            cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
            run_via_blocking_process(cmd, args, "podman", timeout_dur).await
        };
        let result = result.map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("podman {} timed out after {timeout_dur:?}", args.join(" ")),
                )
            } else {
                e
            }
        });

        // A timeout is the only outcome that trips the breaker; any answer at all — including a
        // non-zero exit or a failed spawn — proves the binary still responds and closes it.
        let timed_out = result.as_ref().err().is_some_and(|e| e.kind() == std::io::ErrorKind::TimedOut);
        PODMAN_UNRESPONSIVE.store(timed_out, Ordering::Release);

        let ms = started.elapsed().as_millis();
        match &result {
            Ok(out) if out.status.success() => {
                tracing::info!(target: "podman", argv = %args.join(" "), ms, "podman ok")
            }
            Ok(out) => tracing::warn!(
                target: "podman",
                argv = %args.join(" "),
                ms,
                code = out.status.code().unwrap_or(-1),
                stderr = %String::from_utf8_lossy(&out.stderr).trim(),
                "podman exited non-zero"
            ),
            Err(e) => {
                tracing::error!(target: "podman", argv = %args.join(" "), ms, error = %e, "podman call failed")
            }
        }
        result
    }
}

static MSB_RESOURCE_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Record the bundled microsandbox resource directory (`msb` plus its adjacent `libkrunfw`) for
/// [`msb_executable`]. Called once at startup, before any `msb` call, mirroring
/// `backends::set_podman_resource_dir`.
pub fn set_msb_resource_dir(dir: std::path::PathBuf) {
    let _ = MSB_RESOURCE_DIR.set(dir);
}

/// The bundled `msb` executable — the only one this app ever runs. There is deliberately no
/// `MSB_PATH` override and no `PATH` fallback: a missing bundle fails the spawn with `NotFound`.
///
/// Before [`set_msb_resource_dir`] runs (unit tests only) this is the relative `msb/msb[.exe]`;
/// containing a separator, it is never searched for on `PATH` either.
pub(crate) fn msb_executable() -> std::path::PathBuf {
    let name = if cfg!(windows) { "msb.exe" } else { "msb" };
    match MSB_RESOURCE_DIR.get() {
        Some(dir) => dir.join(name),
        None => std::path::Path::new("msb").join(name),
    }
}

/// Builds an `msb` invocation against [`msb_executable`], pinning its runtime to the bundle
/// whatever the app inherited. `MSB_PATH` is the *executable* path (a directory fails every
/// `msb create` with `Access is denied`); `msb` spawns its VM supervisor from it and resolves the
/// adjacent `libkrunfw`, so the library and agentd overrides are cleared to keep the pair intact.
///
/// `msb create` also fails with `Access is denied` when any Windows Job Object above this app denies
/// breakaway, since `msb` starts its VM supervisor with `CREATE_BREAKAWAY_FROM_JOB`. That cannot be
/// fixed from here; `cargo run`'s job under `tauri dev` is one such job. A packaged build runs in
/// none (docs/features/microsandbox-provider/03_e2e.md, Step 0).
pub(crate) fn msb_command() -> tokio::process::Command {
    let exe = msb_executable();
    let mut cmd = detached_command(&exe);
    cmd.env("MSB_PATH", &exe)
        .env_remove("MSB_LIBKRUNFW_PATH")
        .env_remove("MSB_AGENTD_PATH");
    cmd
}

#[async_trait::async_trait]
impl ProcessRunner for SystemMsbRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        run_capturing_via_files(msb_command(), args, "msb").await
    }

    async fn run_with_env(
        &self,
        args: &[&str],
        env: &[(String, String)],
    ) -> std::io::Result<Output> {
        let mut cmd = msb_command();
        cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        run_capturing_via_files(cmd, args, "msb").await
    }
}

impl SystemMutagenRunner {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl ProcessRunner for SystemMutagenRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        // Sidecar-resolution and data-dir errors collapse into one io::Error here; the caller
        // (`sync::run_via`) re-wraps it as `SyncError::Sidecar`, losing the `DataDir` distinction.
        let mut command = crate::sync::command(&self.app).map_err(std::io::Error::other)?;
        command.args(args).output().await.map_err(std::io::Error::other)
    }
}

#[cfg(windows)]
pub(crate) fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code as u32)
}

#[cfg(unix)]
pub(crate) fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code)
}

#[cfg(all(test, windows))]
mod tests {
    use std::os::windows::io::AsRawHandle;

    /// The sanitiser's contract in one round trip: a handle deliberately marked inheritable is not
    /// inheritable afterwards. Uses a real file handle rather than a synthetic one, because the
    /// snapshot the sanitiser walks only reports handles the kernel actually holds for us.
    #[test]
    fn clears_the_inherit_flag_on_an_inheritable_handle() {
        let path = std::env::temp_dir().join(format!("mio-sanitise-{}.tmp", std::process::id()));
        let file = std::fs::File::create(&path).expect("temp file");
        let handle = file.as_raw_handle().cast();

        assert_ne!(
            unsafe { super::SetHandleInformation(handle, super::HANDLE_FLAG_INHERIT, super::HANDLE_FLAG_INHERIT) },
            0,
            "could not mark the handle inheritable"
        );
        let mut flags = 0u32;
        assert_ne!(unsafe { super::GetHandleInformation(handle, &mut flags) }, 0);
        assert_eq!(flags & super::HANDLE_FLAG_INHERIT, super::HANDLE_FLAG_INHERIT);

        assert!(super::clear_inheritable_handles() > 0, "sanitiser cleared nothing");

        let mut flags = 0u32;
        assert_ne!(unsafe { super::GetHandleInformation(handle, &mut flags) }, 0);
        assert_eq!(flags & super::HANDLE_FLAG_INHERIT, 0, "inherit flag survived the sanitiser");

        drop(file);
        let _ = std::fs::remove_file(&path);
    }
}
