//! The `podman` seam: where the binary is and which connection it targets (the bundled client when
//! the app was started with a resource directory, else whatever `podman` the `PATH` resolves), and
//! [`SystemPodmanRunner`] with its per-call-shape timeouts, half-open breaker and shared
//! `machine list` probe.

use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use tokio::process::Command;

use super::types::ProcessRunner;
use super::{
    detached_command, detached_command_std, is_monitor_call, run_via_blocking_process,
    strip_connection,
};

/// The connection `podman machine init` creates for the local machine. Renaming this const (not its
/// value) breaks the regex parse in `e2e/lib/podman-machine.ts`, which mirrors it as `DEFAULT_MACHINE`.
pub(crate) const DEFAULT_MACHINE_NAME: &str = "podman-machine-default";

/// The Podman connection name container and runtime operations target: `PODMAN_CONNECTION` when set
/// (e2e points this at a non-default machine, e.g. `mio-tests`), else `DEFAULT_MACHINE_NAME`.
///
/// Does not steer machine lifecycle (`src-tauri/src/machine/mod.rs`, which has its own
/// `MIO_PODMAN_MACHINE` override): a connection name isn't guaranteed to name a local machine — see
/// `SandboxBackend::add_host` (`src-tauri/src/backends/hosts.rs`) — so `podman machine` subcommands
/// can't safely run against it.
pub(crate) fn connection_name() -> String {
    std::env::var("PODMAN_CONNECTION").unwrap_or_else(|_| DEFAULT_MACHINE_NAME.to_string())
}

static PODMAN_RESOURCE_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Record the bundled Podman resource directory for [`podman_command`] to use. Called exactly
/// once, at startup, before any subsystem shells out to podman.
pub fn set_podman_resource_dir(dir: std::path::PathBuf) {
    let _ = PODMAN_RESOURCE_DIR.set(dir);
}

pub(crate) fn podman_command() -> Command {
    match PODMAN_RESOURCE_DIR.get() {
        Some(dir) => {
            let mut cmd = detached_command(dir.join("podman.exe"));
            cmd.env("CONTAINERS_HELPER_BINARY_DIR", dir);
            cmd
        }
        None => detached_command("podman"),
    }
}

/// [`podman_command`]'s `std::process` twin, for callers that need a blocking `Child` rather than
/// `tokio::process`'s async one (`mockctl::run_via_blocking_process`) — same binary resolution, same
/// `CONTAINERS_HELPER_BINARY_DIR`, same detachment.
pub(crate) fn podman_command_std() -> std::process::Command {
    match PODMAN_RESOURCE_DIR.get() {
        Some(dir) => {
            let mut cmd = detached_command_std(dir.join("podman.exe"));
            cmd.env("CONTAINERS_HELPER_BINARY_DIR", dir);
            cmd
        }
        None => detached_command_std("podman"),
    }
}

/// The literal argv token for the bundled `podman.exe`, for embedding in an OpenSSH `ProxyCommand`
/// string — [`podman_command`] returns a `tokio::process::Command`, which cannot be. Forward-slashed
/// deliberately (`docs/features/podman-exec-transport/02_dev.md`, Resolved Decision 5).
pub(crate) fn podman_literal_path() -> String {
    match PODMAN_RESOURCE_DIR.get() {
        Some(dir) => {
            let path = dir.join("podman.exe").to_string_lossy().replace('\\', "/");
            format!("\"{path}\"")
        }
        None => "podman".to_string(),
    }
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

/// `podman build` of the base image: installing mise, Herdr, and apt packages easily exceeds 20s
/// and can run longer over a slow network or disk — bounded generously, but not open-ended.
const BUILD_TIMEOUT: Duration = Duration::from_secs(180);

/// Picks the timeout for `args` by call shape — `machine start` and `build` are the only calls on
/// this seam that are legitimately slow rather than stuck; everything else, including `machine`'s
/// other subcommands, gets the tight default. Connection-stripped so a remote `--connection <c>
/// build ...` still matches.
fn call_timeout(args: &[&str]) -> Duration {
    if is_monitor_call(args) {
        return MONITOR_CALL_TIMEOUT;
    }
    match strip_connection(args) {
        ["machine", "start", ..] => MACHINE_START_TIMEOUT,
        ["build", ..] => BUILD_TIMEOUT,
        ["exec", ..] if is_package_install(args) => PACKAGE_INSTALL_TIMEOUT,
        _ => PODMAN_CALL_TIMEOUT,
    }
}

/// An `exec` that downloads toolchains: `mise use`/`mise install` (a first install of node, an npm
/// CLI, or an agent pulls tens of MB, slower still through a sandbox's TLS-intercepting traffic
/// proxy) or an npm postinstall `install.cjs` that fetches an agent's native binary. Legitimately
/// slow rather than stuck, like `podman build`.
const PACKAGE_INSTALL_TIMEOUT: Duration = Duration::from_secs(240);

/// Whether an `exec` argv runs a package install. Matched as an adjacent pair anywhere in the argv
/// because podman's `exec` has no `--` before the in-container command.
fn is_package_install(args: &[&str]) -> bool {
    args.windows(2).any(|w| match w {
        ["mise", "use" | "install"] => true,
        ["node", script] => script.ends_with("/install.cjs"),
        _ => false,
    })
}

/// Shorter than the default bound: the monitor samples every 10 s and a missed sample is harmless.
const MONITOR_CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether the podman breaker is open, so the resource monitor can skip Podman rather than fail
/// fast on every tick.
pub(crate) fn podman_breaker_open() -> bool {
    PODMAN_UNRESPONSIVE.load(Ordering::Acquire)
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
    let cmd = podman_command_std();
    let result: SharedOutput = Arc::new(
        run_via_blocking_process(
            cmd,
            &MACHINE_LIST_ARGV,
            "podman-machine-list",
            MACHINE_LIST_PROBE_TIMEOUT,
            None,
        )
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

/// Real `podman` invocations: wraps [`podman_command_std`], preserving its resource-dir/env
/// resolution. Also the podman seam's single health verdict — see [`podman_breaker_open`] and
/// `docs/features/podman-process-isolation/02_dev.md`, Phase D.
pub struct SystemPodmanRunner;

#[async_trait::async_trait]
impl ProcessRunner for SystemPodmanRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        self.run_podman(args, &[], None).await
    }

    /// `env` lands in the child's environment only, never in `args`, so the argv logged below
    /// stays free of secret values (`docs/features/techdebt-sweep/02_dev.md`, Resolved Decision 4).
    async fn run_with_env(
        &self,
        args: &[&str],
        env: &[(String, String)],
    ) -> std::io::Result<Output> {
        self.run_podman(args, env, None).await
    }

    async fn run_with_stdin(&self, args: &[&str], stdin: &[u8]) -> std::io::Result<Output> {
        self.run_podman(args, &[], Some(stdin)).await
    }
}

impl SystemPodmanRunner {
    /// The one body behind every trait method, so the guard, breaker and logging below cover each.
    async fn run_podman(
        &self,
        args: &[&str],
        env: &[(String, String)],
        stdin: Option<&[u8]>,
    ) -> std::io::Result<Output> {
        let quiet = is_monitor_call(args);
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
                if quiet {
                    tracing::debug!(target: "podman", argv = %args.join(" "), error = %e, "podman guard rejected: machine not running");
                } else {
                    tracing::warn!(target: "podman", argv = %args.join(" "), error = %e, "podman guard rejected: machine not running");
                }
                return Err(e);
            }
        }

        // While the breaker is tripped only the half-open probe proceeds; everyone else is failed
        // now rather than each paying another full timeout for the same broken seam.
        // A monitor call never takes the probe slot: a user's call should be the one to find out.
        if quiet && PODMAN_UNRESPONSIVE.load(Ordering::Acquire) {
            tracing::debug!(target: "podman", argv = %args.join(" "), "podman breaker open: monitor call not attempted");
            return Err(unresponsive_error(args));
        }
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
        if quiet {
            tracing::debug!(target: "podman", argv = %args.join(" "), "podman starting");
        } else {
            tracing::info!(target: "podman", argv = %args.join(" "), "podman starting");
        }
        let timeout_dur = call_timeout(args);
        let result = if strip_connection(args) == MACHINE_LIST_ARGV {
            // The one call several callers fire at once (the welcome flow's cards, the projects-page
            // probe), so it collapses into a single shared process instead of one each.
            clone_shared_output(&machine_list().await)
        } else {
            // Blocking-thread + file-captured, not `tokio::process`: see `run_via_blocking_process`'s
            // doc comment — `tokio::process`'s async spawn/wait has been observed to hang the full
            // timeout even on a bare `podman machine list` under `cargo tauri dev`.
            let mut cmd = podman_command_std();
            cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
            run_via_blocking_process(cmd, args, "podman", timeout_dur, stdin).await
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
        // A monitor call is exempt both ways: a slow stats call on a struggling sandbox must not
        // fail-fast the user's Stop (`docs/features/sandbox-resource-monitor/02_dev.md`, Resolved
        // Decision 8).
        if !quiet {
            let timed_out =
                result.as_ref().err().is_some_and(|e| e.kind() == std::io::ErrorKind::TimedOut);
            PODMAN_UNRESPONSIVE.store(timed_out, Ordering::Release);
        }

        let ms = started.elapsed().as_millis();
        match &result {
            Ok(out) if out.status.success() && quiet => {
                tracing::debug!(target: "podman", argv = %args.join(" "), ms, "podman ok")
            }
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn monitor_calls_get_the_short_bound_and_nothing_else_does() {
        let stats = ["--connection", "c", "stats", "--no-stream", "--format", "{{json .}}"];
        assert!(is_monitor_call(&stats));
        assert_eq!(call_timeout(&stats), MONITOR_CALL_TIMEOUT);
        let pool = ["--connection", "c", "info", "--format", "{{json .Host}}"];
        assert_eq!(call_timeout(&pool), MONITOR_CALL_TIMEOUT);
        let info = ["--connection", "c", "info", "--format", "json"];
        assert!(!is_monitor_call(&info), "the readiness probe keeps its breaker-tripping shape");
        assert_eq!(call_timeout(&info), PODMAN_CALL_TIMEOUT);
    }

    #[test]
    fn podman_package_install_execs_get_the_long_bound() {
        let install = ["--connection", "c", "exec", "--workdir", "/root/work", "x", "mise", "use", "jq"];
        assert_eq!(call_timeout(&install), PACKAGE_INSTALL_TIMEOUT);
        let plain = ["--connection", "c", "exec", "--workdir", "/root/work", "x", "mise", "rm", "jq"];
        assert_eq!(call_timeout(&plain), PODMAN_CALL_TIMEOUT);
        // Only an exec qualifies: a container literally named `mise` in a `start` is still tight.
        assert_eq!(call_timeout(&["start", "mise", "use"]), PODMAN_CALL_TIMEOUT);
    }
}
