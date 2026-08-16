# Podman Process Isolation — Ideas

Every external process the app spawns on Windows inherits the console handles of whatever terminal
started the app, and a single poisoned console handle in that terminal wedges every `podman`
invocation at process startup — permanently, and with no recovery path in either the Rust backend or
the Svelte frontend. The feature covers four seams: the process-construction helper
(`src-tauri/src/process_ext.rs`), the timeout and capture helpers in `src-tauri/src/mockctl/mod.rs`,
the error types (`src-tauri/src/error.rs` and `src/lib/errors.ts`), and the welcome-flow readiness
components (`src/lib/welcome/CreateVmStep.svelte`, `src/lib/welcome/DownloadNecessitiesStep.svelte`).
The failure was diagnosed on 2026-08-16/17 against a live wedged session; the evidence trail lives in
`docs/features/podman-process-isolation/spike/`.

## Current State

`no_window()` in `src-tauri/src/process_ext.rs` is the single helper every raw process construction
routes through, and sets exactly one Windows creation flag, `CREATE_NO_WINDOW`, to suppress the
console window a GUI-subsystem app would otherwise pop up. `no_window()` sets no stdio disposition,
so stdin is inherited from the app process on every spawn.

`podman_command()` and `podman_command_std()` (`src-tauri/src/backends/mod.rs`) resolve the bundled
podman binary and call `no_window()`. `msb_command()` (`src-tauri/src/mockctl/mod.rs`) does the same
for the `msb` CLI. `run_host_wsl()` and `wsl_is_ready()` (`src-tauri/src/machine/mod.rs`) construct
`wsl` commands the same way.

`run_via_blocking_process()` (`src-tauri/src/mockctl/mod.rs`) runs a podman call on a plain OS thread
and bounds the wait with `tokio::time::timeout`, capturing stdout and stderr through temp files. The
helper exists because `tokio::process` was observed to hang on spawn/wait under a supervising Windows
Job Object, and because a detached grandchild inheriting a pipe handle blocks `wait_with_output()`
forever — both reasons are recorded in the helper's own doc comment. The child process handle stays
inside the spawned thread's closure, so no caller can reach the child once the wait is under way.

`run_capturing_via_files()` (`src-tauri/src/mockctl/mod.rs`) is the `msb` and Mutagen equivalent and
carries no timeout at all.

`SystemPodmanRunner::run()` (`src-tauri/src/mockctl/mod.rs`) is the one seam every real podman call
passes through, and already carries a pre-flight guard: `machine_running_state()` consults a cached
`podman machine list` snapshot and rejects a call whose `--connection` names a confirmed-stopped
machine. The guard matches on a leading `--connection` argument pair only, so bare `machine ...`
subcommands bypass the cache entirely.

`listen_events()` (`src-tauri/src/backends/mod.rs`) is the one podman call that does not use
`SystemPodmanRunner`: the streamed-child shape has no `ProcessRunner` representation, so the function
spawns `podman events` directly with piped stdout and stderr, unbounded, at app startup.

`MachineReadiness` and `MachineReadinessProvider` (`src-tauri/src/machine/types.rs`) back the
provider-neutral `machine_readiness` and `prepare_machine` commands (`src-tauri/src/lib.rs`).
`CreateVmStep.svelte` (`src/lib/welcome/`) calls both through `src/lib/machine/readiness.ts`;
`DownloadNecessitiesStep.svelte` independently calls `base_image_ready`. Both components probe on
mount, concurrently and without coordination.

`AppError` (`src-tauri/src/error.rs`) already distinguishes `PodmanUnavailable` and
`PodmanRebootRequired` from the catch-all `Backend`, mirrored by the `AppErrorKind` union in
`src/lib/errors.ts`. A podman call that never answers currently collapses into
`BackendError::CommandFailed` and therefore reaches the frontend as kind `Backend`.

`run_dev_directly()` (`build.rs`) launches `sandbox-mio.exe` from `build.exe dev` with fully inherited
stdio, deliberately bypassing the Windows Job Object assignment `run()` and `try_run()` apply, because
`msb create` must be able to break away from any job the app sits in.

## The failure being fixed

A GUI-subsystem image is never *attached* to the console of the process that launched it, but it does
inherit that console's raw `\Device\ConDrv` handles as its standard handles. `sandbox-mio.exe` under
`build.exe dev` therefore holds console handles belonging to a console it cannot use. Rust's
`std::process::Command` sets `bInheritHandles` to `TRUE` and does not populate
`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, so every child inherits *all* inheritable handles in the app —
not only the three standard handles a caller names explicitly.

When one of the inherited console handles carries a pending I/O that never completes, podman's
runtime initialisation blocks in `NtQueryInformationFile` against that handle and never reaches
argument parsing. The condition is terminal for the process and independent of the podman
subcommand: `machine list`, `ps` and `image exists` all hang identically, and the wedge survives app
restarts because the poisoned handle lives in the terminal session, not in the app.

## Proposed model: detach every spawned process from the console

Replace `no_window()` in `src-tauri/src/process_ext.rs` with a constructor-shaped seam that sets
`DETACHED_PROCESS` instead of `CREATE_NO_WINDOW` and pins all three standard handles explicitly, with
stdin bound to the null device. `DETACHED_PROCESS` means the child is given no console at all, so
Windows performs no console-handle reconciliation during child startup and no console handle can be
queried. Binding stdin to the null device removes the only console handle a caller currently leaves
implicit.

The seam must be a constructor rather than a modifier, so that no call site can obtain a `Command`
that has not been detached — the current `no_window()` contract relies on every author remembering to
call it, which the module comment in `src-tauri/src/process_ext.rs` states as a convention rather than
enforcing.

`listen_events()` (`src-tauri/src/backends/mod.rs`) must route through the same seam. The function
needs piped stdout for streaming, so the seam has to allow stdout and stderr to be overridden while
keeping stdin and the creation flags fixed.

## Proposed model: reap children that exceed their timeout

`run_via_blocking_process()` (`src-tauri/src/mockctl/mod.rs`) should send the spawned child back to
the async side over a channel *before* the OS thread blocks in its wait, so the timeout branch can
terminate the child rather than abandon it. Terminating the child also releases the blocking wait, so
the OS thread the helper currently leaks on every timeout exits as well.

The doc comment on `PODMAN_CALL_TIMEOUT` (`src-tauri/src/mockctl/mod.rs`) still states that
`kill_on_drop` on the spawned child reaps the process when the timeout elapses. The statement stopped
being true when the helper moved off `tokio::process`, and must be corrected as part of the change:
the leak is currently documented as fixed.

## Proposed model: one health verdict for the whole podman seam

Widen the existing pre-flight guard in `SystemPodmanRunner::run()` (`src-tauri/src/mockctl/mod.rs`)
from a connection-scoped check into the seam's single health verdict, with two properties.

Single-flight: concurrent callers asking the same probe question await one shared in-flight process
rather than spawning one process each. `MACHINE_LIST_CACHE` already provides the caching half of the
property but only for calls carrying a leading `--connection` pair, which is why `machine_readiness`,
`base_image_ready` and the projects-page probe each spawn their own `podman machine list`.

Breaker: a call that times out marks the seam unresponsive, and subsequent calls fail immediately
with the unresponsive condition instead of each paying the full timeout and each leaking a process.
An explicit user action clears the mark. The breaker is what turns a wedged terminal from an
open-ended multiplier into one bounded failure.

## Proposed model: an error kind meaning "podman never answered"

Add a `BackendError` variant (`src-tauri/src/backends/types.rs`) for a podman invocation that never
produced an exit status, surfaced as a new `AppError` variant and `kind()` arm
(`src-tauri/src/error.rs`) and mirrored into the `AppErrorKind` union (`src/lib/errors.ts`).

The existing `PodmanUnavailable` kind means the machine or socket is down, which is a different
condition with a different remedy: starting the machine fixes `PodmanUnavailable` and does nothing
for an unresponsive podman binary. Collapsing both into `Backend` is what produces the current
message, "is the machine running?", at a moment when the machine demonstrably is running.

## Proposed model: every probe-failure state offers a way out

`CreateVmStep.svelte` (`src/lib/welcome/`) renders its `probeError` state as a paragraph with no
control of any kind, so a failed mount probe leaves the onboarding card permanently dead and an app
restart is the user's only escape. The `probeError` branch needs a retry that re-runs the probe, and
the unresponsive-podman kind needs a distinct presentation naming the log file rather than implying a
machine-state remedy the user cannot act on.

The welcome flow's two cards probing independently (`src/lib/welcome/CreateVmStep.svelte` and
`src/lib/welcome/DownloadNecessitiesStep.svelte`, both on mount, via `WelcomeFlow.svelte`) is the
frontend half of the duplicate-probe problem the breaker addresses in the backend.

## Technology choices with tradeoffs

**`DETACHED_PROCESS` over `CREATE_NO_WINDOW`.** Both suppress the console window, which is the only
property `no_window()` was written for. `CREATE_NO_WINDOW` gives the child a new headless console and
therefore keeps the child on the console-startup path where the observed hang occurs, and spawns one
`conhost.exe` per call — seven orphaned `conhost.exe` processes accompanied the seven orphaned podman
processes in the wedged session. `DETACHED_PROCESS` gives the child no console at all.
`detached-process-check.rs` (`docs/features/podman-process-isolation/spike/`) confirms podman returns
correct output under `DETACHED_PROCESS`. The residual risk is a spawned CLI that requires a console —
none of podman, `msb`, `wsl`, `ssh` or Mutagen reads from a console once stdout, stderr and stdin are
all bound explicitly, but the risk is real for any future addition and is why the seam should be a
constructor that makes the disposition visible.

Reversed after the packaged build shipped. The property this choice overlooked is that a console-less
child makes Windows allocate a fresh *visible* console for every grandchild it spawns with default
creation flags, and each of podman, `msb` and Mutagen spawns one — so a packaged build flashed a
terminal window per call. The seam now sets `CREATE_NO_WINDOW`
(`src-tauri/src/process/mod.rs`); the console-startup-path and `conhost.exe` arguments above lost
their weight once the sanitiser, not the creation flag, turned out to be what stops console-handle
inheritance. See `02_dev.md`, Approach.

**`PROC_THREAD_ATTRIBUTE_HANDLE_LIST` evaluated and rejected for now.** Restricting inheritance to an
explicit handle list is the only mechanism that stops non-standard inheritable handles from reaching
a child, and would neutralise the poisoned handle at the source rather than avoiding the code path
that queries it. Rust's `std::process::Command` does not expose the attribute, so adopting the
attribute means calling `CreateProcessW` directly or taking a dependency on the `windows` crate, and
re-implementing the temp-file capture and the exit-status plumbing already working in
`src-tauri/src/mockctl/mod.rs`. The cost is not justified while `DETACHED_PROCESS` avoids the
console-startup path entirely, and the choice is recorded as Open Question 1 because the spike could
not prove `DETACHED_PROCESS` sufficient against a genuinely wedged handle.

**A Windows Job Object evaluated and rejected.** Assigning spawned processes to a job would make
orphaned children die with the app and would have limited the blast radius of the leak. The approach
is closed off by an existing constraint: `run_dev_directly()` (`build.rs`) exists specifically to keep
`sandbox-mio.exe` out of any job, because `msb create` spawns a detached VM supervisor that must break
away from the app's job and a job member cannot grant itself an exemption its owner did not set. The
constraint is documented in `msb_command()`'s doc comment (`src-tauri/src/mockctl/mod.rs`).
Reaping children explicitly on timeout achieves the same cleanup without reintroducing the job.

**Reaping through a returned child handle over `tokio::process` `kill_on_drop`.** `kill_on_drop` is
the idiomatic answer and is what the current `PODMAN_CALL_TIMEOUT` doc comment still describes, but
the helper deliberately left `tokio::process` behind: `run_via_blocking_process()`'s doc comment
records that `tokio::process`'s async spawn and wait were observed to hang on a bare
`podman machine list`. Returning the child handle over a channel keeps the blocking-thread strategy
intact and adds the missing termination path, at the cost of the helper holding a child handle across
a thread boundary.

**A breaker inside `SystemPodmanRunner::run()` over per-command guards.** Guarding each Tauri command
individually spreads the same check across `machine_readiness`, `base_image_ready`,
`check_podman_ready` and `prepare_machine` (`src-tauri/src/lib.rs`) and misses every future caller —
the pattern the existing guard's own doc comment already argues against. Guarding in the frontend was
also considered and rejected: `src/lib/podman/setup.svelte.ts` deliberately re-probes on every mount
because Podman can be stopped or started outside the app, and a frontend cache cannot see calls the
backend makes on its own behalf.

**A new error kind over reusing `PodmanUnavailable`.** Reusing `PodmanUnavailable` requires no new
variant and would route the failure into the existing recovery screen. Reuse is rejected because the
existing recovery screen offers "Start Podman", which cannot fix an unresponsive binary, and because
`checkPodman()` (`src/lib/podman/setup.svelte.ts`) branches on the `PodmanUnavailable` kind to decide
between `setupPodman()` and `startPodman()` — both wrong remedies for the condition.

**No host-global state is added.** Every proposed change is process-local: creation flags, stdio
dispositions, an in-process breaker, and error types. No named machine, connection, SSH alias, host
port or Windows service is introduced, so the feature adds nothing that would pin its tests to the
host-locked tier or serialise them against other real-infrastructure runs (`e2e/AGENTS.md`).

## Validation

Investigated on 2026-08-16/17 against a live wedged dev session. Probe sources and captured output
are in `docs/features/podman-process-isolation/spike/`, described by
`docs/features/podman-process-isolation/spike/README.md`.

Cleared: the podman binary and argv are innocent. The staged binary at
`src-tauri/target/debug/podman/podman.exe` returned the full machine list in 0.71 seconds from a
shell while seven app-spawned invocations of the identical argv had been hung for over an hour.

Cleared: the hang is at process startup, not in podman's own work. `thread-stack-sample.rs` put the
main thread at `ntdll.dll!ZwQueryInformationFile+0x14` with a stack about 2KB deep and only bootstrap
DLLs loaded.

Cleared: the blocking object is an inherited console handle. `handle-dump.rs` found handle `0x64`
reporting `<NAME QUERY HUNG>` in `build.exe`, in `sandbox-mio.exe` and in every podman child.
Inherited handles retain their numeric value across a spawn, so identical values across the three
processes establish inheritance.

Cleared: the app's environment is not a factor. `env-block-dump.rs` showed no `CONTAINERS_*`,
`PODMAN_*` or `XDG_*` variable in the app's inherited environment, and a complete, ordinary Windows
environment block otherwise.

Cleared: `DETACHED_PROCESS` does not break podman. `detached-process-check.rs` returned a correct
584-byte machine list in 576ms.

Cleared: the failure predates and is independent of the "restart onboarding" flow.
`resetOnboarding()` (`src/lib/settings/onboarding.svelte.ts`) only writes an `onboardingCompleted`
boolean, and the app log at `%LOCALAPPDATA%\com.tincho.sandbox-mio\logs\mio.log.2026-08-16` contains
no successful podman call in any session, across six separate app starts.

Not cleared: no synthetic reproduction of the hang. `spawn-from-gui-parent.rs`,
`spawn-from-console-parent.rs` and the `console-launcher.rs` plus `spawn-from-gui-child.rs` pair all
reproduce the app's process topology, including a GUI-subsystem child holding inherited console
handles it is not attached to, and podman completed normally in every combination. Reproducing the
hang requires a console handle already carrying a stalled I/O, which the spike could not manufacture
on demand.

## Open Questions

1. **Is `DETACHED_PROCESS` sufficient on its own, or is `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` also
   required?** A detached child is never given a console, which should remove the console-startup
   query the hang occurs in, but the child still inherits every inheritable handle in the app. The
   spike could not settle the question because it could not manufacture a wedged console handle. The
   question can only be closed by observing a detached build inside a genuinely wedged terminal
   session, or by accepting the reasoning without a live confirmation.

2. **What clears the breaker?** Candidates are: an explicit user retry only; a retry plus a
   time-based expiry; or a retry plus a successful call on any other seam. A time-based expiry
   restores automatic recovery when the wedge clears on its own, at the cost of periodically paying a
   full timeout again.

3. **Should the welcome flow's two cards share one readiness query?** Sharing removes the duplicate
   probe at its source and makes one failure produce one message, but couples
   `CreateVmStep.svelte` and `DownloadNecessitiesStep.svelte` (`src/lib/welcome/`), which are
   currently independent by design, and pushes shared state into a module-scoped rune store
   (`src/AGENTS.md`).

4. **Should `run_capturing_via_files()` gain a timeout?** The `msb` and Mutagen path
   (`src-tauri/src/mockctl/mod.rs`) is unbounded today, so an unresponsive `msb` binary hangs a
   command forever with no diagnostic. Adding a timeout means choosing bounds for `msb create`, whose
   legitimate duration is not yet characterised in the way `MACHINE_START_TIMEOUT` and
   `BUILD_TIMEOUT` characterise the podman calls.

5. **Should `run_dev_directly()` (`build.rs`) bind the app's stdin to the null device?** Doing so
   keeps the poisoned handle out of the app's handle table in the first place and costs nothing in
   dev, since the app never reads stdin. The change is defence in depth rather than a fix: a packaged
   build launched from a console inherits the same handles, so the app must still detach its own
   children.

6. **How is the change covered by regression tests?** `SystemPodmanRunner` is replaced by
   `ScriptedPodmanRunner` under `MIO_MOCK_BACKEND=1` (`src-tauri/src/mockctl/mod.rs`), so the entire
   spawn path the feature changes is bypassed by the `light` tier. Coverage of the breaker and the new
   error kind can reach the `light` tier through the scripted seam, while the creation flags and stdio
   dispositions can only be observed against real processes.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `podman machine list --format json timed out after 30s (is the machine running?)` while `podman machine ls` in a shell reports the machine as `Currently running` | The app inherited a console handle carrying a stalled I/O from the terminal that launched `build.exe dev`, and the podman child blocks in `NtQueryInformationFile` against the inherited handle before parsing arguments | Restart `./build.exe dev` from a newly opened terminal window; `kill_stale_dev_processes()` (`build.rs`) reaps the orphaned podman processes from the previous run on the way in |
| Orphaned `podman.exe` and `conhost.exe` processes accumulate and outlive the app | `run_via_blocking_process()` (`src-tauri/src/mockctl/mod.rs`) keeps the child handle inside its OS thread's closure, so the timeout branch abandons the child instead of terminating it | Terminate the orphans with `Get-Process podman,conhost`, and implement the child-handle channel described in the "reap children that exceed their timeout" section |
| The `PODMAN_CALL_TIMEOUT` doc comment (`src-tauri/src/mockctl/mod.rs`) promises `kill_on_drop` reaps a timed-out process, but orphans persist | The comment describes the superseded `tokio::process` implementation; `run_via_blocking_process()` uses a blocking `std::process::Command` that has no drop-time kill | Correct the `PODMAN_CALL_TIMEOUT` doc comment when implementing the reaping change, so the leak is not documented as already fixed |
| One stalled podman call produces three or more concurrent stalled processes | The pre-flight guard in `SystemPodmanRunner::run()` (`src-tauri/src/mockctl/mod.rs`) matches only a leading `--connection` argument pair, so bare `machine ...` subcommands from `machine_readiness`, `base_image_ready` and the projects-page probe each spawn their own process | Widen the guard into the single-flight and breaker model described in the "one health verdict" section |
| The "Create virtual machine" onboarding card shows an error and offers no control at all | The `probeError` branch of `CreateVmStep.svelte` (`src/lib/welcome/`) renders a paragraph with no retry, unlike the `rebootMessage` and default branches which both render a button | Add a retry control to the `probeError` branch of `CreateVmStep.svelte` |
| A new spawned CLI hangs the same way after the feature lands | The spawned command was constructed with a raw `Command::new` instead of the detached constructor in `src-tauri/src/process_ext.rs` | Route every process construction through the detached constructor in `src-tauri/src/process_ext.rs`; never call `Command::new` directly outside the constructor |
| A `light` tier run passes while the real spawn path is broken | `MIO_MOCK_BACKEND=1` swaps `SystemPodmanRunner` for `ScriptedPodmanRunner` (`src-tauri/src/mockctl/mod.rs`), so creation flags and stdio dispositions are never exercised | Verify spawn-path changes against a real Podman machine with `./build e2e` (`standard` tier), never with `./build e2e light` alone |
