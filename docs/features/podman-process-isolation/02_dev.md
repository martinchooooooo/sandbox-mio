---
type: dev-plan
title: "Podman Process Isolation — Build Plan"
feature: podman-process-isolation
---
# Podman Process Isolation — Build Plan

Build plan continuing `docs/features/podman-process-isolation/01_ideas.md`, which diagnosed why every
`podman` process spawned by `sandbox-mio.exe` hangs at process startup when the launching terminal
holds a console handle carrying a stalled I/O. Nothing is implemented yet: all seven phases below are
`Remaining`. The plan supersedes `01_ideas.md` wherever the two differ — most importantly on the
inheritance-control mechanism, which a follow-up spike on 2026-08-17 settled differently from the
leading option `01_ideas.md` recorded.

All phases are implemented. Phase 0 below was added during implementation and is not in the original
plan: the production spawn path it moves was cited throughout this document as living in
`src-tauri/src/mockctl/mod.rs`, and every such citation now reads `src-tauri/src/process/`.

## The concrete model

Four defences, in the order a spawned process encounters them.

**The app owns no inheritable handle it did not create.** A sanitiser runs as the first statement of
`run()` (`src-tauri/src/lib.rs`) and clears `HANDLE_FLAG_INHERIT` on every inheritable handle the
process owns, so the console handles inherited from the launching terminal can never reach a child.
The sanitiser is the load-bearing defence: `inheritance-ab-test.rs`
(`docs/features/podman-process-isolation/spike/`) proved a child inherits a `\Device\ConDrv` handle
*even when* spawned with `DETACHED_PROCESS` and all three standard handles bound explicitly, and
inherits none once the inherit flags are cleared.

**No child is given this app's console.** A detached-command constructor in
`src-tauri/src/process/mod.rs` replaces `no_window()`, allocating the child a fresh windowless console
via `CREATE_NO_WINDOW` and binding stdin to the null device. The child never touches the launching
terminal's console, which is the console-startup path the hang occurs in.

`DETACHED_PROCESS` was used here first, and reverted: a child with no console at all makes Windows
allocate a brand-new *visible* console for every grandchild it spawns with default creation flags,
and every binary on this seam spawns one (`podman` → `wsl.exe`/`gvproxy`, `msb` → its VM supervisor,
Mutagen → its daemon), so a packaged build flashed a terminal window per call. A windowless console is
inherited by those grandchildren, and stays invisible. Nothing is lost by the revert, because creation
flags were never the defence — the sanitiser is (Resolved Decision 1).

**No child outlives its timeout.** `run_via_blocking_process()`
(`src-tauri/src/process/mod.rs`) returns the spawned child to the async side before its OS thread
blocks, so the timeout branch terminates the child instead of abandoning it.

**No failure is paid for more than once.** `SystemPodmanRunner::run()`
(`src-tauri/src/process/mod.rs`) becomes the single health verdict for the podman seam: concurrent
callers share one in-flight probe, and a timed-out call trips a half-open breaker that fails
subsequent callers immediately until one probe succeeds. The condition reaches the frontend as its
own error kind, and every frontend state that can display the condition offers a way out of it.

## Resolved Decisions

1. **Is `DETACHED_PROCESS` sufficient on its own, or is `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` also
   required?** Neither: the spike proved `DETACHED_PROCESS` insufficient (a detached child with fully
   explicit stdio still inherited a console handle) and `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`
   unavailable (`CommandExt::raw_attribute` remains behind the unstable
   `windows_process_extensions_raw_attribute` feature on rustc 1.96.0 stable, verified by
   `std-handle-list-check.rs` in `docs/features/podman-process-isolation/spike/`). The resolution is
   startup handle sanitisation as the guarantee. `DETACHED_PROCESS` was retained as a second,
   independent layer and then reverted to `CREATE_NO_WINDOW` for the grandchild console-window
   regression described in Approach — a layer that prevented nothing measurable was not worth a
   visible terminal window per call.

2. **What clears the breaker?** A half-open probe, with no timer and no configurable threshold: while
   tripped, the breaker admits exactly one call at a time and fails every concurrent caller
   immediately; a success closes the breaker, a timeout leaves it tripped. Time-based expiry was
   rejected because it reintroduces a duration constant whose only effect is to periodically re-pay a
   full timeout.

3. **Should the welcome flow's two cards share one readiness query?** Yes. A module-scoped rune store
   at `src/lib/machine/readiness.svelte.ts` owns one readiness result, and
   `CreateVmStep.svelte` and `DownloadNecessitiesStep.svelte` (`src/lib/welcome/`) both read from the
   store instead of probing independently on mount.

4. **Should `run_capturing_via_files()` gain a timeout?** Not in this feature. The `msb` and Mutagen
   path (`src-tauri/src/process/mod.rs`) gains the sanitiser and the detached constructor like every
   other path, but bounding it requires characterising `msb create`'s legitimate duration, which is
   independent of the console-handle defect. Deferred to
   `docs/features/podman-process-isolation/04_improvements.md`.

5. **Should `run_dev_directly()` bind the app's stdin to the null device?** Yes, as defence in depth
   in `build.rs`, keeping the poisoned handle out of the app's table in dev. The app still sanitises
   its own handles, because a packaged build launched from a console inherits the same handles.

6. **How is the change covered by regression tests?** Three ways, because no single tier reaches the
   whole change: a `cargo test` unit test for the sanitiser (it creates a deliberately inheritable
   handle, runs the sanitiser, and asserts the inherit flag is cleared); the `light` tier for the
   breaker and the new error kind, both reachable through `ScriptedPodmanRunner`; and the `standard`
   tier for creation flags and stdio dispositions, which only real processes exercise. The precise
   specs are `03_e2e.md`'s to define.

## Implemented: Phase 0 — split the production spawn path out of `mockctl/`

Added during implementation, ahead of every other phase. `src-tauri/src/mockctl/` hosted both halves
of each `ProcessRunner` pair, so the production podman path — the very code Phases C, D and E deepen
— lived inside the module named for the e2e mock control plane.

`src-tauri/src/process/` now owns the production layer: `types.rs` declares the `ProcessRunner` trait
and the `SystemPodmanRunner`/`SystemMsbRunner`/`SystemMutagenRunner` structs, and `mod.rs` holds
their implementations plus `run_via_blocking_process()`, `run_capturing_via_files()`,
`msb_command()`, the timeout constants, `strip_connection()`, `exit_status()`, and everything Phases
A–E add. `src-tauri/src/process_ext.rs` was folded into it and deleted.

`src-tauri/src/mockctl/` keeps only the scripted halves: `MockDirector`, the `Scripted*` runners,
`KNOWN_CALLS`, the key-derivation functions and the no-override defaults. The dependency now points
one way — `mockctl` uses `process`, never the reverse — because the trait describes a production
seam the `light` tier stands in for, not a mock facility production borrows.

## Implemented: Phase A — startup handle sanitisation

Add a sanitiser to `src-tauri/src/process/mod.rs` that snapshots the system handle table via
`NtQuerySystemInformation` with `SystemExtendedHandleInformation`, filters the snapshot to the current
process id, and clears `HANDLE_FLAG_INHERIT` with `SetHandleInformation` on every handle reporting the
flag. `handle-sanitise-check.rs` and `sanitize_lib.rs`
(`docs/features/podman-process-isolation/spike/`) hold the working implementation to port.

Call the sanitiser as the first statement of `run()` (`src-tauri/src/lib.rs`), before the Tauri
builder is constructed and before `init_logging()` — the ordering is load-bearing, because any
process spawned before the sanitiser runs still inherits the poisoned handles, and `listen_events()`
(`src-tauri/src/backends/mod.rs`) plus the Mutagen daemon call both spawn early.

The sanitiser is Windows-only and compiles behind `cfg(windows)`, mirroring how
`src-tauri/src/process/mod.rs` is already structured around Windows creation flags.

Clearing the inherit flag does not interfere with `std::process::Command` binding stdio at spawn
time, because `std` sets the flag on the handles it passes as part of the spawn — confirmed by
`handle-sanitise-check.rs`, which spawned podman successfully after clearing all ten inheritable
handles it found.

Add the `cargo test` unit test named in Resolved Decision 6 alongside the sanitiser.

## Implemented: Phase B — the detached command constructor

Replace the `NoWindow` trait and its `no_window()` method (`src-tauri/src/process/mod.rs`) with a
constructor-shaped seam returning an already-detached `Command`, in both a `std::process` and a
`tokio::process` flavour, mirroring the existing `podman_command()` / `podman_command_std()` pairing
in `src-tauri/src/backends/mod.rs`. The constructor sets `CREATE_NO_WINDOW` (see Approach for why not
`DETACHED_PROCESS`) and binds stdin to the null device; stdout and stderr stay overridable, because
`listen_events()`
(`src-tauri/src/backends/mod.rs`) needs piped stdout for streaming.

A constructor replaces the modifier deliberately. The module comment in
`src-tauri/src/process/mod.rs` currently states that every raw `Command::new` "should be piped
through" `no_window()` — a convention no compiler enforces, and the reason a future spawn site can
silently reintroduce the defect. Update the module comment to describe the console-handle contract
rather than only the console-window suppression `no_window()` was written for.

Migrate every construction site: `podman_command()` and `podman_command_std()`
(`src-tauri/src/backends/mod.rs`), `msb_command()` (`src-tauri/src/process/mod.rs`), `run_host_wsl()`
and `wsl_is_ready()` (`src-tauri/src/machine/mod.rs`), the Mutagen sidecar command in
`src-tauri/src/sync/mod.rs`, and `activate_podman_socket()` (`src-tauri/src/machine/mod.rs`).

`listen_events()` (`src-tauri/src/backends/mod.rs`) is the one podman call that bypasses
`SystemPodmanRunner` because `ProcessRunner` models one-shot calls only; it is constructed through
`podman_command()` and so gains the detachment with every other podman call.

The Mutagen sidecar needed a third constructor. `tauri_plugin_shell`'s own `Command` hard-codes
`CREATE_NO_WINDOW` and piped stdio and exposes no hook to override either, so `sync::command()`
(`src-tauri/src/sync/mod.rs`) resolves the sidecar through the plugin, converts it back to a
`std::process::Command` (the plugin's `From` impl), and puts it through `detach()`
(`src-tauri/src/process/mod.rs`) — which is why `sync::command()` now returns a
`tokio::process::Command` rather than the plugin's type.

Migration reached further than the plan listed: the `NoWindow` trait had call sites in
`src-tauri/src/inference/` (three files) and `src-tauri/src/ssh/mod.rs` too, all migrated. No
`Command::new` for an external binary remains outside `src-tauri/src/process/mod.rs`.

## Implemented: Phase C — reap children that exceed their timeout

Change `run_via_blocking_process()` (`src-tauri/src/process/mod.rs`) to spawn the child on its OS
thread and send the child back over a channel *before* the thread blocks in its wait, so the
`tokio::time::timeout` branch can terminate the child. Terminating the child releases the blocking
wait, so the OS thread the helper currently leaks on every timeout also exits.

Correct the doc comment on `PODMAN_CALL_TIMEOUT` (`src-tauri/src/process/mod.rs`), which still states
that `kill_on_drop` reaps the process when the timeout elapses. The statement described the
superseded `tokio::process` implementation and currently documents the leak as already fixed.

The reaping kills only children the app itself spawned, addressed by their own handles. No process is
matched by name, so the worktree-scoped `target\debug\` prefix filter that `kill_stale_dev_processes()`
(`build.rs`) uses is not needed here and no other worktree's processes are reachable.

Keep the blocking-thread strategy. `run_via_blocking_process()`'s doc comment records that
`tokio::process`'s async spawn and wait were observed to hang on a bare `podman machine list`, so
moving back to `tokio::process` for `kill_on_drop` would reintroduce a known hang.

## Implemented: Phase D — single-flight probes and a half-open breaker

Widen the pre-flight guard in `SystemPodmanRunner::run()` (`src-tauri/src/process/mod.rs`) from a
connection-scoped check into the seam's health verdict.

Single-flight: replace `MACHINE_LIST_CACHE` and `machine_running_state()`
(`src-tauri/src/process/mod.rs`) with a shared in-flight probe that concurrent callers await, rather
than a cache consulted only by calls carrying a leading `--connection` argument pair. The current
argument match is why `machine_readiness`, `base_image_ready` and the projects-page probe
(`src-tauri/src/lib.rs`) each spawn their own `podman machine list`.

Half-open breaker: a timed-out call marks the seam unresponsive. While marked, one call at a time is
admitted as a probe and every concurrent caller fails immediately with the unresponsive condition; a
successful probe clears the mark. Preserve the existing guard's stated principle that an inconclusive
or failed probe must never block a call the old code path would have attempted — only a confirmed
condition does.

The breaker state is process-local, so nothing is shared between worktrees and the `light` tier's
`ScriptedPodmanRunner` (`src-tauri/src/mockctl/mod.rs`) is unaffected.

As built, the single-flight is sequence-numbered rather than time-expired: a caller that started
waiting before the running probe finished reuses that probe's result, and one arriving afterwards
runs a fresh probe. `MACHINE_LIST_CACHE_TTL` is therefore gone rather than retuned — there is no
window in which a stale answer is served to a caller that could have had a fresh one. The
single-flight serves the `machine list` calls themselves, not only the connection-scoped guard, which
is what collapses the several `podman machine list` invocations the plan names.

## Implemented: Phase E — an error kind meaning "podman never answered"

Add a `BackendError` variant (`src-tauri/src/backends/types.rs`) for a podman invocation that produced
no exit status, replacing the `CommandFailed` string the timeout currently collapses into. Add the
matching `AppError` variant and `kind()` arm (`src-tauri/src/error.rs`), and mirror the discriminant
into the `AppErrorKind` union (`src/lib/errors.ts`) — omitting the `kind()` arm fails to compile,
omitting the TypeScript union member does not, so the union is the easier half to forget.

The variant is `PodmanUnresponsive`, and it needed one hop the plan did not name: `PodmanProvider::run`
(`src-tauri/src/provider/mod.rs`) maps into `ProviderError`, which would have flattened the condition
into `CommandFailed` before it ever reached `BackendError`, so `ProviderError::Unresponsive` carries
it across. `AppError`'s `Backend` variant also lost its `#[from]` in favour of a hand-written
`From<BackendError>` (`src-tauri/src/error.rs`), so the one `BackendError` variant with its own IPC
kind is not flattened into `"Backend"` on the way across the boundary. `std::io::ErrorKind::TimedOut`
is the marker the seam agrees on, converted centrally by `From<std::io::Error> for BackendError`
(`src-tauri/src/backends/types.rs`) rather than at each call site.

Do not reuse `PodmanUnavailable`. `checkPodman()` (`src/lib/podman/setup.svelte.ts`) branches on the
`PodmanUnavailable` kind to choose between `setupPodman()` and `startPodman()`, and both remedies are
wrong for an unresponsive binary.

Drop "(is the machine running?)" from the timeout message in `SystemPodmanRunner::run()`
(`src-tauri/src/process/mod.rs`). The question is misleading precisely when the condition occurs: the
machine was running throughout the diagnosed session.

## Implemented: Phase F — one readiness query, and a way out of every failure

Add a module-scoped rune store at `src/lib/machine/readiness.svelte.ts` owning one readiness result
and one in-flight request, following the store convention in `src/AGENTS.md` (component-local `$state`
is destroyed by the router; shared state belongs in a module-scoped `.svelte.ts` store under
`src/lib/`). The existing `src/lib/machine/readiness.ts` keeps the `invoke()` wrappers
`fetchMachineReadiness`, `fetchMachineSetupSteps` and `prepareMachine`; the new store calls them.

Point `CreateVmStep.svelte` (`src/lib/welcome/`) at the store instead of probing independently on
mount, per Resolved Decision 3.

`DownloadNecessitiesStep.svelte` turned out not to issue a machine-readiness query at all: its mount
probe is `base_image_ready`, a different question, so there was no second readiness query to
deduplicate on the frontend. The duplicate `podman machine list` the plan attributes to it is real
but originates in the Rust pre-flight guard, and is removed by Phase D's single-flight. The card
still gains the unresponsive rendering below.

Give the `probeError` branch of `CreateVmStep.svelte` a retry control. The branch currently renders a
paragraph and nothing else, while the sibling `rebootMessage` and default branches both render a
button — a failed mount probe therefore leaves the card permanently dead with an app restart as the
only escape.

Render the unresponsive kind distinctly from a machine-state failure, naming the log file rather than
implying a remedy the user cannot act on. New user-facing strings follow the `TODO(i18n)` convention
already used throughout `src/lib/welcome/`; Paraglide keys are added after the working feature is
approved, per the `feature-implement` skill.

## Implemented: Phase G — the dev launcher's stdio

Bind the app's stdin to the null device where `run_dev_directly()` (`build.rs`) spawns
`sandbox-mio.exe`, per Resolved Decision 5. Leave stdout and stderr inherited so dev output still
reaches the terminal, and leave the deliberate job-object bypass untouched — `run_dev_directly()`
exists to keep `sandbox-mio.exe` out of any Windows Job Object so `msb create` can break away, as its
doc comment records.

## Lifecycle and cross-cutting concerns

**Phase ordering.** Phase A must land before Phase B is meaningful and before any behaviour is
judged, because the sanitiser is the defence the spike proved necessary; `DETACHED_PROCESS` alone
does not stop console-handle inheritance. Phases C, D and E are independent of each other and of A.
Phase F depends on Phase E for the error kind it renders.

**No new host-global state.** Every change is process-local: creation flags, stdio dispositions,
handle flags, an in-process breaker, error types, and one frontend store. No new path, host port,
podman machine or connection name, SSH alias, Mutagen session, Windows service, bundled binary under
`src-tauri/binaries/`, teardown of host-global state, or harness environment variable is introduced.
The feature therefore adds no serialisation cost against other worktrees' runs, and its specs are not
pinned to the host-locked tier by anything in the plan (`e2e/AGENTS.md`).

**The mock seam is untouched.** `MIO_MOCK_BACKEND=1` swaps `SystemPodmanRunner` for
`ScriptedPodmanRunner` (`src-tauri/src/mockctl/mod.rs`), so Phases B, C and G are invisible to the
`light` tier by construction. Phase A runs in every configuration, including the `light` tier, because
the sanitiser sits in `run()` (`src-tauri/src/lib.rs`) ahead of the mock-backend branch — the
sanitiser must therefore be safe with no handles to clear.

**Supersedes.** `MACHINE_LIST_CACHE`, `MACHINE_LIST_CACHE_TTL` and `refresh_machine_list_cache()`
are replaced by Phase D's single-flight probe and breaker; `machine_running_state()` and
`MACHINE_LIST_PROBE_TIMEOUT` survive, now reading from the shared probe
(`src-tauri/src/process/mod.rs`). The replaced approach guarded only calls
carrying a leading `--connection` argument pair and had no concept of an unresponsive binary; do not
reintroduce a connection-scoped-only guard. The `NoWindow` trait and `no_window()`
(`src-tauri/src/process/mod.rs`) are replaced by Phase B's constructor; do not reintroduce a modifier
method that a call site can omit.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Console handles still reach spawned children after the detached constructor lands | `DETACHED_PROCESS` and explicit stdio do not control handle inheritance; `inheritance-ab-test.rs` (`docs/features/podman-process-isolation/spike/`) showed a detached child with all three standard handles bound explicitly still inheriting a `\Device\ConDrv` handle | Land Phase A: the sanitiser in `src-tauri/src/process/mod.rs`, called as the first statement of `run()` (`src-tauri/src/lib.rs`) |
| The sanitiser runs but some children still inherit console handles | A process was spawned before the sanitiser ran; `listen_events()` (`src-tauri/src/backends/mod.rs`) and the Mutagen daemon call both spawn early in startup | Keep the sanitiser call as the first statement of `run()` (`src-tauri/src/lib.rs`), ahead of the Tauri builder and `init_logging()` |
| `CommandExt::raw_attribute` does not compile | `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` support is behind the unstable `windows_process_extensions_raw_attribute` feature on rustc 1.96.0 stable | Use the Phase A sanitiser instead; re-run `std-handle-list-check.rs` (`docs/features/podman-process-isolation/spike/`) after a toolchain upgrade to see whether the attribute has stabilised |
| A new spawn site hangs the same way after the feature lands | The command was built with a raw `Command::new` rather than the detached constructor in `src-tauri/src/process/mod.rs` | Construct every command through the detached constructor in `src-tauri/src/process/mod.rs`; never call `Command::new` outside the constructor |
| A packaged build pops up a terminal window on every `podman`, `msb`, `wsl` or Mutagen call | The child was spawned with `DETACHED_PROCESS`, leaving it no console; Windows then allocates a fresh *visible* console for each grandchild the child spawns with default creation flags | Spawn with `CREATE_NO_WINDOW` instead (`src-tauri/src/process/mod.rs`), which gives the child a windowless console its grandchildren inherit |
| Adding a new `AppError` variant compiles but the frontend shows a generic error | `kind()` (`src-tauri/src/error.rs`) is an exhaustive match and fails to compile when a variant is missing, but neither the hand-written `From<BackendError> for AppError` nor the `AppErrorKind` union in `src/lib/errors.ts` has a compile-time link to it | Add the `kind()` arm, the `From<BackendError>` arm when the variant originates as a `BackendError`, and the discriminant in `src/lib/errors.ts` |
| The breaker never closes again after tripping | A breaker that fails every call cannot observe a success, so a design without a half-open state latches permanently | Implement Resolved Decision 2: while tripped, admit exactly one call at a time as a probe and fail only the concurrent callers |
| `./build e2e light` passes while the real spawn path is broken | `MIO_MOCK_BACKEND=1` swaps `SystemPodmanRunner` for `ScriptedPodmanRunner` (`src-tauri/src/mockctl/mod.rs`), so Phases B, C and G are never exercised | Verify spawn-path changes with `./build e2e` (`standard` tier) against a real Podman machine, and cover the Phase A sanitiser with the `cargo test` unit test named in Resolved Decision 6 |
