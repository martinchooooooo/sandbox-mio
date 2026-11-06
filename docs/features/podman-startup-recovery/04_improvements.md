---
type: improvements
title: "Podman Startup Recovery — Deferred Improvements"
feature: podman-startup-recovery
---
# Podman Startup Recovery — Deferred Improvements

Findings from a review of how the app resolves its Podman target, and of the E2E cost the current resolution imposes on the `standard` tier. The subject is the split between `connection_name()` (`src-tauri/src/backends/mod.rs`) and the hardcoded `DEFAULT_MACHINE_NAME` used throughout `src-tauri/src/machine/mod.rs`. `02_dev.md` remains the record of the feature's settled design.

**Implemented as a follow-up:** Option B and Option C, per the recommendation below. `src-tauri/src/machine/mod.rs` now resolves every lifecycle call site through `machine_name()` (`MIO_PODMAN_MACHINE`, defaulting to `DEFAULT_MACHINE_NAME`); its module doc comment and `connection_name()`'s doc comment in `src-tauri/src/backends/mod.rs` both state the split is deliberate; the root `AGENTS.md` Environment Variables section documents both `PODMAN_CONNECTION` and `MIO_PODMAN_MACHINE` accordingly. `e2e/wdio.conf.ts`'s `beforeSession` sets `MIO_PODMAN_MACHINE=mio-tests` for every session except `podman-startup-recovery.spec.ts`'s own (decided from `specs`, mirroring `isLightSession`); `onPrepare` now calls the new `ensureMachineInitialized()` (`e2e/lib/podman-machine.ts`) for `podman-machine-default` instead of the full `provisionMachine()`, and `REQUIRED_MACHINES` no longer includes it. The "Unrelated fragility" section below was already fixed in the codebase prior to this pass — `podman-startup-recovery.spec.ts`'s `after()` hook already calls `activatePodmanSocket()` after the restart.

## Problem: machine lifecycle and container work resolve different targets

`connection_name()` (`src-tauri/src/backends/mod.rs:35`) returns `PODMAN_CONNECTION` when set, falling back to `DEFAULT_MACHINE_NAME` (`src-tauri/src/backends/mod.rs:27`). `default_backend()` (`src-tauri/src/backends/mod.rs:79`) and `default_runtime()` (`src-tauri/src/runtime/mod.rs:30`) both bind to the value `connection_name()` returns, so every container and in-container runtime operation follows `PODMAN_CONNECTION`.

Every method of `PodmanMachineManager` (`src-tauri/src/machine/mod.rs`) ignores `connection_name()` and names `DEFAULT_MACHINE_NAME` literally: `machine_is_running` (line 78), `check_other_machines_running` (line 93), `run_machine_start` (line 98), `activate_podman_socket` (line 121), `poll_ready` via `verify_host` (line 137), `machine_exists` (line 185), `init_machine` (line 189), `reset_stuck_machine` (lines 195 and 200).

Under the E2E `standard` tier, `beforeSession` (`e2e/wdio.conf.ts:266`) sets `PODMAN_CONNECTION=mio-tests`. The app therefore creates, starts and execs into containers on `mio-tests` while health-gating, starting, socket-activating and self-repairing `podman-machine-default` — a machine no sandbox operation ever touches during the run. The behaviour predates the rename from `DEFAULT_CONNECTION` to `DEFAULT_MACHINE_NAME`; the rename made the divergence legible without changing it.

## Evidence: the real topology of the `standard` tier

`check_podman_ready` (`src-tauri/src/lib.rs:612`) is invoked once per app session, from the `/projects` `onMount` handler (`src/routes/projects/+page.svelte:102`). A `PodmanUnavailable` rejection sets `podmanUnavailable` and returns before `loadAll()`, so the project list never renders and the "Start Podman" recovery screen replaces it.

13 of the 15 spec files under `e2e/test/specs/standard/` call `ProjectsPage.open()` and `createProject()`, so 13 sessions per run require `check_podman_ready` to succeed against `podman-machine-default`. The exceptions are `e2e/test/specs/standard/base-image-recipe.spec.ts` (a static regex guard over the Containerfiles, no app interaction beyond startup) and `e2e/test/specs/standard/forward-sweep.spec.ts` (asserts Mutagen forward state via the CLI only).

Their actual container work targets `mio-tests`, and `e2e/test/specs/standard/deploy-remote.spec.ts` additionally targets `mio-tests-remote`. No standard spec performs container work on `podman-machine-default`.

`e2e/test/specs/standard/podman-startup-recovery.spec.ts` is the sole spec with the opposite requirement: its `before()` hook (line 26) stops `podman-machine-default` so that the recovery screen renders, and its Step 7 (line 134) creates a sandbox on `mio-tests` specifically to prove the probe target and the backend target are independent. Its header comment (lines 18-21) records the two-machine topology as deliberate, and records that `check_other_machines_running()` returns true throughout because `mio-tests` stays running, which makes `start_podman_machine` (`src-tauri/src/lib.rs:642`) skip the `wsl --shutdown` branch.

The conflict between "13 specs need `podman-machine-default` running" and "one spec needs it stopped" is resolved today by spec ordering plus the recovery flow itself. WebdriverIO runs the `./test/specs/standard/**/*.ts` glob in alphabetical order with `maxInstances: 1`, placing `podman-startup-recovery.spec.ts` twelfth; the three specs after it (`project-start-stop`, `runtime-management`, `workspace-mount`) depend on the recovery having left the machine running with an activated socket, or on the spec's `after()` hook (line 43) having restarted it.

### The E2E cost

`onPrepare` (`e2e/wdio.conf.ts:171`) calls `provisionMachine(DEFAULT_MACHINE)` and aborts the whole suite on failure, with a comment (lines 165-169) naming the unconditional probe as the reason. `provisionMachine` (`e2e/lib/podman-machine.ts:103`) does far more than initialise: it runs `podman machine init` when absent, `podman machine start` with a three-tier WSL recovery ladder, then `startPodmanSocket` polled by `waitForConnection` up to four times. A third WSL2 virtual machine therefore boots on every `standard` run, beside `mio-tests` and `mio-tests-remote`, purely to satisfy a probe.

`03_e2e.md` (Harness Changes, and Environment Notes bullet 1) specified that `podman-machine-default` need only be *initialized*, with a non-fatal warning if absent. The shipped `onPrepare` diverges because the plan accounted only for `podman-startup-recovery.spec.ts`'s own needs and not for the other 12 `/projects` specs, which the unconditional probe also gates.

### The "init only, no start" idea is refuted

Provisioning `podman-machine-default` with `podman machine init` alone does not satisfy the probe, for two independent reasons:

1. `machine_is_running` (`src-tauri/src/machine/mod.rs:72-80`) matches on `Name == DEFAULT_MACHINE_NAME` **and** `Running == true` in `podman machine list --format json` output. An initialised-but-stopped machine reports `Running: false`.
2. `check_podman_ready` (`src-tauri/src/lib.rs:620`) additionally calls `poll_ready(1)`, which runs `verify_host` (`src-tauri/src/machine/mod.rs:56`) — `podman --connection podman-machine-default info --format json`. Success requires the machine booted *and* the rootless API socket at `/run/user/1000/podman/podman.sock` answering. The socket does not come up on boot on the WSL image (see the `PODMAN_SOCKET` comment at `src-tauri/src/machine/mod.rs:16-18` and the `startPodmanSocket` comment at `e2e/lib/podman-machine.ts:72-88`), which is why `provisionMachine` activates it explicitly.

Only `machine_needs_init` (`src-tauri/src/lib.rs:626`), which calls `machine_exists`, is satisfied by initialisation alone. The idea is refuted: any fix that keeps the probe pointed at `podman-machine-default` for all 13 `/projects` specs must boot and socket-activate the machine.

### What each method would do if it followed `connection_name()`

Evaluated against the `standard` tier, where `PODMAN_CONNECTION=mio-tests` and `mio-tests` is up:

- `machine_is_running` would match `mio-tests` and return true, so `check_podman_ready` would pass even with `podman-machine-default` stopped. `podman-startup-recovery.spec.ts` Step 2 would never see the recovery screen and Steps 3-6 would have nothing to recover.
- `poll_ready` / `verify_host` would probe the `mio-tests` socket, which `provisionMachine` already activated — success, again bypassing the recovery path under test.
- `check_other_machines_running` would treat `podman-machine-default` and `mio-tests-remote` as the "other" machines. Semantics improve rather than degrade: the guard exists to avoid `wsl --shutdown` tearing down unrelated WSL instances, and comparing against the machine actually being started is the more accurate test. In production, with `PODMAN_CONNECTION` unset, behaviour is byte-identical to today.
- `run_machine_start`, `init_machine`, `activate_podman_socket` and `reset_stuck_machine` would act on the connection named by `PODMAN_CONNECTION`, which is only meaningful when the connection *is* a local Podman machine. `add_host` (`src-tauri/src/backends/mod.rs:641`) creates connections via `podman system connection add <name> <ssh-url>` for arbitrary remote SSH endpoints, where `podman machine start <name>` and `wsl --unregister <name>` are meaningless or destructive. Connection names and machine names are overlapping but distinct namespaces.

## Shape of fix

### Option A — point machine lifecycle at `connection_name()` and rework the recovery spec

Replace `DEFAULT_MACHINE_NAME` with `connection_name()` throughout `src-tauri/src/machine/mod.rs`, then give `podman-startup-recovery.spec.ts` a target it may legitimately stop. Stopping `mio-tests` is not viable: the machine holds every other spec's containers, images and workspace bind mounts, and `ensureMiseBaseImage` staging (`e2e/lib/podman-machine.ts:192`) would have to survive the restart. The workable variant is a per-session `PODMAN_CONNECTION` override in `beforeSession` pointing the recovery spec's app session at `podman-machine-default` while leaving it stopped.

Tradeoff: the reworked spec proves recovery against a real machine and a real socket, so the decisive Step 6 assertion survives — but Step 7 does not. With the probe and the backend resolving the same name, "sandbox operations are independent of the probe target" ceases to be a property that exists, and the step would have to be deleted or repointed at `podman-machine-default`, which has no staged base image. Option A also makes machine lifecycle follow a value that may name a remote SSH connection, per the `add_host` evidence above. Adopting Option A means accepting that `PODMAN_CONNECTION` is only ever a local machine.

### Option B — declare the split deliberate and document it

Treat machine lifecycle as intentionally scoped to the host's default Podman machine, and `PODMAN_CONNECTION` as a backend-target override rather than a machine selector. Under Option B the current code in `src-tauri/src/machine/mod.rs` is correct as written and only two things need changing: the `PODMAN_CONNECTION` entry in the root `AGENTS.md` Environment Variables section must state that the override steers container and runtime operations only and leaves machine lifecycle on `podman-machine-default`, and `src-tauri/src/machine/mod.rs`'s module comment must state the same so the next reader does not read the hardcoding as an oversight.

Tradeoff: zero behavioural risk, and it matches the namespace argument — a machine is not a connection. It leaves a genuine product wart for any user who sets `PODMAN_CONNECTION` themselves: the app would health-gate and self-repair a machine it never uses, exactly the situation the E2E tier is in. Option B is only honest if `PODMAN_CONNECTION` is documented as test-only.

### Option C — a dedicated machine-name override, read only by `src-tauri/src/machine/mod.rs`

Add a `machine_name()` resolver reading a new variable (`MIO_PODMAN_MACHINE`, defaulting to `DEFAULT_MACHINE_NAME`) and use the resolver in place of the literal at all eight call sites in `src-tauri/src/machine/mod.rs`. `beforeSession` (`e2e/wdio.conf.ts:243`) then sets `MIO_PODMAN_MACHINE=mio-tests` for every session **except** the one running `podman-startup-recovery.spec.ts`, decided from the resolved `specs` argument exactly as the existing `isLightSession` check already decides `MIO_MOCK_BACKEND`. `onPrepare` drops to initialising `podman-machine-default` without starting it, reviving `03_e2e.md`'s original Environment Notes contract.

Option C is the only candidate that leaves `e2e/test/specs/standard/podman-startup-recovery.spec.ts` unmodified. That spec's session keeps today's exact topology — probe on `podman-machine-default`, sandboxes on `mio-tests` — so Steps 1-7 including the Step 6 socket assertion and the Step 7 independence assertion all still prove what they prove, and `check_other_machines_running()` still returns true because `mio-tests` remains up. The third virtual machine boots once, inside the spec whose subject is booting it, instead of in `onPrepare`. The 12 other `/projects` specs health-gate against `mio-tests`, still a real machine with a real socket, so probe coverage moves rather than disappears.

Tradeoff: a second env-steering knob in shipped code whose only consumer is the test harness, and a `standard` tier where the probe target varies per spec session. Option C is coherent only on top of Option B's premise — if machine lifecycle is about "the machine the app manages", then overriding *which* machine that is belongs in its own variable rather than being conflated with the backend target.

**Recommendation: Option B plus Option C, in that order.** Option B settles the semantics with no behavioural change and is worth landing regardless. Option C then removes the third virtual-machine boot without touching a single assertion, which is the property Option A cannot offer. Option A should be rejected: it costs the Step 7 independence assertion, and it binds machine-lifecycle commands to a value that `add_host` allows to be a remote SSH endpoint. The one decision left to the maintainer is whether a test-only `MIO_PODMAN_MACHINE` variable in shipped code is acceptable; the precedent set by `MIO_MOCK_BACKEND`, `SANDBOX_WORKSPACE_ROOT`, `MIO_CREDENTIALS_DIR` and `MIO_SETTINGS_DIR` suggests it is.

## Unrelated fragility: `after()` restarts the machine but never re-activates its socket

Independent of the options above, and worth fixing under any of them. `podman-startup-recovery.spec.ts`'s `after()` hook (line 43) runs `podman machine start podman-machine-default` only. A mid-spec failure between Step 2 and Step 5 therefore leaves the machine running with no rootless socket, because socket activation is the app's `activate_podman_socket` work that the failed run never completed. `poll_ready(1)` inside `check_podman_ready` then fails for every following session, so `project-start-stop.spec.ts`, `runtime-management.spec.ts` and `workspace-mount.spec.ts` land on the recovery screen and burn their full timeouts — turning one spec failure into four. Calling `startPodmanSocket(DEFAULT_MACHINE)` from `e2e/lib/podman-machine.ts` in the `after()` hook, after the restart, contains the blast radius. `03_e2e.md`'s Environment Notes bullet 5 anticipated a stopped machine but not a running machine with a dead socket.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `podman-startup-recovery.spec.ts` Step 2 times out: the "Start Podman" button never appears although `podman machine stop podman-machine-default` succeeded | A change pointed `machine_is_running` or `poll_ready` (`src-tauri/src/machine/mod.rs`) at `connection_name()`, so `check_podman_ready` probes `mio-tests` (running) instead of the stopped `podman-machine-default` | Revert the machine-lifecycle target to a value that names `podman-machine-default` during that spec's session; see the Option A and Option C sections above before repointing any call site in `src-tauri/src/machine/mod.rs` |
| `project-start-stop.spec.ts`, `runtime-management.spec.ts` and `workspace-mount.spec.ts` all show the "Start Podman" recovery screen and time out after `podman-startup-recovery.spec.ts` failed | `podman-startup-recovery.spec.ts`'s `after()` hook restarts `podman-machine-default` but never re-activates the rootless socket, so `poll_ready(1)` inside `check_podman_ready` (`src-tauri/src/lib.rs:620`) keeps failing for later sessions | Run `provisionMachine("podman-machine-default")` (`e2e/lib/podman-machine.ts`) manually before re-running the suite, and add a `startPodmanSocket` call to the spec's `after()` hook |
| `onPrepare` was reduced to `podman machine init podman-machine-default` and every `/projects` spec now hangs on the recovery screen | `machine_is_running` (`src-tauri/src/machine/mod.rs:72`) requires `Running == true`, and `check_podman_ready` also requires the rootless socket to answer `podman --connection podman-machine-default info` — an initialised machine satisfies neither | Restore the full `provisionMachine(DEFAULT_MACHINE)` call in `e2e/wdio.conf.ts` `onPrepare`; only `machine_needs_init` (`src-tauri/src/lib.rs:626`) is satisfied by initialisation alone |
| Setting `PODMAN_CONNECTION` to a user's own machine makes the app health-gate and self-repair a machine it never runs containers on | `connection_name()` (`src-tauri/src/backends/mod.rs:35`) steers `default_backend`/`default_runtime` only; all eight machine-lifecycle call sites in `src-tauri/src/machine/mod.rs` hardcode `DEFAULT_MACHINE_NAME` | Do not set `PODMAN_CONNECTION` outside the E2E harness until the split is resolved; the options and recommendation are in the "Shape of fix" section above |
| `podman machine start <name>` or `wsl --unregister <name>` fails or damages an unrelated host after machine lifecycle was made to follow `PODMAN_CONNECTION` | `add_host` (`src-tauri/src/backends/mod.rs:641`) registers connections for arbitrary remote SSH endpoints via `podman system connection add`, so a connection name is not guaranteed to name a local Podman machine | Keep machine-lifecycle operations on a name that is known to be a local machine (Option B/C above); never pass an arbitrary connection name to `podman machine` subcommands |
