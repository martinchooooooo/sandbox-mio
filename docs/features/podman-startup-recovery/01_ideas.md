---
type: ideas
title: "Podman Startup Recovery"
feature: podman-startup-recovery
---
# Podman Startup Recovery

This document covers graceful handling of the scenario where the Podman machine (`podman-machine-default`) is stopped or its user-level socket is missing after a host system restart. Currently the app propagates a raw Podman error message to the UI and leaves the user with no recovery path. None of this feature is implemented; all relevant seams live in `src-tauri/src/backends/mod.rs` and `src/routes/projects/+page.svelte`.

## Current State

`PodmanCliBackend::verify_host` (`src-tauri/src/backends/mod.rs:192`) runs `podman --connection <name> info` to probe readiness. Any failure returns `BackendError::CommandFailed(stderr)`, which propagates to `AppError::Backend` and surfaces verbatim in the UI via `parseError().message` (`src/lib/errors.ts:25`).

The projects page (`src/routes/projects/+page.svelte:28`) calls `loadProjects`, `loadDeployments`, and `loadHosts` on mount; these catch errors the same way and store them in a page-level `error` string displayed to the user. There is no startup probe that runs before these calls, so the first failed invoke is what the user sees.

The E2E harness (`e2e/lib/podman-machine.ts`) already contains the full recovery logic: `provisionMachine` starts the machine, `startPodmanSocket` launches the rootless Podman API service via `podman machine ssh` with `setsid`, and `waitForConnection` polls until the socket is ready. This code is synchronous Node.js and cannot be called from the Rust backend directly, but it encodes the exact sequence the backend must replicate.

## Why the Socket Is Missing After Restart

`podman-machine-default` is a minimal WSL2 image. Its per-user systemd manager (`user@1000.service`) fails to start because no D-Bus session bus is available in this image (`systemctl --user` exits with "Failed to connect to user scope bus via local transport"). The rootless Podman API socket at `/run/user/1000/podman/podman.sock` is therefore never activated on boot. `podman machine start` starts the WSL kernel but does not launch the socket service, so `podman info` fails even after a successful machine start.

The socket must be launched manually by running the following inside the machine:

```
mkdir -p /run/user/1000/podman
setsid env XDG_RUNTIME_DIR=/run/user/1000 podman system service --time=0 </dev/null >/tmp/podman-svc.log 2>&1 &
```

`setsid` is required. Without it the service dies when the SSH channel closes, because SSH sends SIGHUP to its child process group on hangup and a plain background process is still in that group.

## Proposed Model

`machine init` is out of scope. The machine is assumed to already exist. The feature covers only two failure states: machine stopped, and machine running but socket missing.

### New `AppError` Variant: `PodmanUnavailable`

A new variant `PodmanUnavailable` should be added to `AppError` in `src-tauri/src/error.rs`. This separates "Podman is reachable but rejected a command" (`BackendError::CommandFailed`) from "the machine exists but is not ready." The frontend branches on `kind === "PodmanUnavailable"` to show a dedicated recovery UI instead of the generic error string.

The variant carries no payload beyond its message — the machine name is always `DEFAULT_CONNECTION` (`"podman-machine-default"`). Serialisation follows the existing `{ kind, message }` pattern; `kind()` must return `"PodmanUnavailable"`.

`AppErrorKind` in `src/lib/errors.ts` must add `"PodmanUnavailable"` to stay in sync.

### New Backend Command: `check_podman_ready`

A new `#[tauri::command] async fn check_podman_ready` in `src-tauri/src/lib.rs` runs `podman machine list --format json` (via `podman_global`) and checks whether `DEFAULT_CONNECTION`'s machine is listed and running. If the machine is stopped it returns `Err(AppError::PodmanUnavailable)` immediately — it does not start anything. If the machine appears running but `verify_host` still fails (socket missing), it also returns `Err(AppError::PodmanUnavailable)`.

This is a read-only probe. Starting the machine is a separate, explicit user action.

### New Backend Command: `start_podman_machine`

A new `#[tauri::command] async fn start_podman_machine` in `src-tauri/src/lib.rs` performs the full startup sequence. It enrolls into the activity center (see below) so progress is visible globally and does not need to be wired into the projects page UI directly. The sequence:

1. `activity::open(&app, "podman-startup", TaskKind::PodmanStartup, None)` — opens the task before any work begins.
2. Run `podman machine start <DEFAULT_CONNECTION>`. If it fails, check `podman machine list --format json` for any other machine with `Running: true`. If none are running, attempt `wsl --shutdown` (resets a stuck WSL kernel) and retry once; if other machines are running, skip the shutdown and retry once without it. Call `activity::progress` at each step with a `detail` string (e.g. `"Starting machine…"`, `"Retrying after WSL reset…"`, `"Retrying…"`).
3. Activate the socket: run `podman machine ssh <name> '<setsid command>'` with a 7-second SIGTERM timeout, mirroring `startPodmanSocket` in `e2e/lib/podman-machine.ts:88`. Call `activity::progress` with `detail: "Activating Podman socket…"`.
4. Poll `verify_host` (existing `src-tauri/src/backends/mod.rs:192`) up to four times with 500 ms sleep between attempts.
5. On success: `activity::succeed(&app, "podman-startup", None)`. On failure: `activity::fail(&app, "podman-startup", error)`, then return `Err(AppError::PodmanUnavailable)`.

The task id `"podman-startup"` is a natural singleton — only one machine can be starting at a time — so a retry replaces the prior row in the activity ledger.

### New `TaskKind` Variant: `PodmanStartup`

`TaskKind` in `src-tauri/src/activity/types.rs` gains a `PodmanStartup` variant (serialised as `"podman-startup"`). The corresponding TypeScript union in `src/lib/activity/types.ts` gains `"podman-startup"`. A Paraglide key `task_podman_startup_label` ("Starting Podman machine") is added to all locale files in `messages/`.

### Frontend: Startup Probe and "Start Podman" Screen

The projects page (`src/routes/projects/+page.svelte`) invokes `check_podman_ready` in `onMount` before the existing `loadProjects`/`loadDeployments`/`loadHosts` calls. If it returns `PodmanUnavailable`, the page replaces its normal content with a simple "Podman machine is not running" message and a "Start Podman" button.

Clicking the button invokes `start_podman_machine`. The activity center automatically surfaces progress via its global panel and toasts — the projects page needs no progress UI of its own. The button only needs to go into a loading state (via `createActionRunner`) while the command is in flight.

On success the activity center fires a `sonner` success toast ("Starting Podman machine finished"), and the projects page calls `check_podman_ready` once more to confirm, then runs the normal load sequence.

On failure the activity center fires a sticky failure toast with the error. The projects page shows a "Retry" button that re-invokes `start_podman_machine`.

## Technology Choices

### `podman machine ssh` vs. raw `ssh` for socket activation

`podman machine ssh <name> <cmd>` is chosen over running a raw `ssh` command with the machine's connection URI. `podman machine ssh` resolves the correct key path, port, and username automatically from `podman system connection list`, so no additional parsing is needed. The alternative — calling `ssh` directly with values from `remote_connection_info` (`src-tauri/src/backends/mod.rs:225`) — is rejected because it duplicates the URI-parsing logic already in `podman` and would break if Podman changes how it stores machine credentials.

### `setsid` for socket daemonisation vs. `nohup`

`setsid` is required to run `podman system service` in a new session so it survives the SIGHUP sent by SSH on channel close. `nohup` was considered and rejected: `nohup` only redirects SIGHUP-on-terminal-close but does not move the process out of the SSH session's process group. In this WSL image, the SSH server sends SIGHUP to the entire foreground process group when the channel closes, so `nohup` is insufficient. The E2E spike confirmed `setsid` is the correct primitive (`e2e/lib/podman-machine.ts:93`).

### Detection via `podman machine list` vs. stderr message matching

The machine's running state is determined by parsing `podman machine list --format json` rather than by matching substrings in the `CommandFailed` error message (e.g., "actively refused it"). Substring matching is fragile because Podman's error messages are not a stable API and differ between Podman versions and between the machine-stopped and socket-missing states. Parsing `--format json` output is rejected for the socket-missing case specifically (the machine shows `Running: true` even when the socket is absent), which is why the `verify_host` poll is the authoritative readiness gate.

### Progress transport: activity center (not a dedicated `Channel`)

`start_podman_machine` reports progress through the activity center (`activity::open`/`progress`/`succeed`/`fail` in `src-tauri/src/activity/mod.rs`) rather than via a per-invoke `Channel` argument supplied by the frontend. A `Channel` would require the projects page to wire up an `onmessage` handler and display its own progress UI, which is exactly the per-route progress duplication the activity center was built to eliminate. The activity center's global panel and toast surface the startup progress app-wide without any bespoke progress UI on the projects page. This feature therefore depends on the activity center feature being built first.

### Startup probe vs. per-command retry

Recovering on first command failure (wrapping every `podman()` call) is rejected because it would add a retry loop inside functions like `list`, `create`, and `deploy` that are not designed to be re-entrant after partial execution. A single upfront probe keeps all existing command handlers free of recovery logic.

### User-initiated start vs. auto-start

Auto-starting the machine on app launch was considered and rejected. Starting `podman machine start` takes 5–15 seconds and the user may not want it running (e.g., they are working on something unrelated). Showing the "not running" state and requiring a button click keeps the user in control of when the machine starts.

## Validation (Spike — 2026-06-29)

The full failure sequence was reproduced by restarting the host laptop. Steps and outcomes:

| Risk | Command | Outcome |
|---|---|---|
| Machine stopped after restart | `podman machine list` | Confirmed: `podman-machine-default` listed with no "Currently running" marker |
| `podman info` fails when stopped | `podman info` | Confirmed: "unable to connect to Podman socket: ... connectex: No connection could be made" |
| Machine start succeeds | `podman machine start` | Confirmed: exit 0, machine Running |
| Socket still missing after start | `ls /run/user/1000/podman/` | Confirmed: directory absent |
| User systemd not available | `wsl -d podman-machine-default -- systemctl --user status podman.socket` | Confirmed: "Failed to connect to user scope bus via local transport" |
| System socket active but wrong path | `wsl -d podman-machine-default -- systemctl status podman.socket` | Confirmed: rootful socket at `/run/podman/podman.sock` active; user socket not started |
| Manual socket launch works | `wsl -d podman-machine-default -- bash -c "mkdir -p /run/user/1000/podman && XDG_RUNTIME_DIR=/run/user/1000 podman system service --time=0 ..."` | Confirmed: socket appears at `/run/user/1000/podman/podman.sock` |
| `podman info` succeeds after manual socket launch | `podman info` | Confirmed: returns version info, connection healthy |

## Resolved Decisions

**Machine name**: hardcoded to `DEFAULT_CONNECTION` (`"podman-machine-default"`). No detection from `podman machine list`.

**`wsl --shutdown` guard**: before running `wsl --shutdown`, check `podman machine list --format json` for any other machine with `Running: true`. If any are found, skip the shutdown step and proceed directly to the retry — losing the WSL reset is acceptable because the shutdown would disrupt those machines. If no other machines are running, proceed with `wsl --shutdown` as normal.

## Common Gotchas

| Gotcha | Detail |
|---|---|
| Machine "Running" ≠ socket ready | `podman machine list` shows `Running: true` even when `/run/user/1000/podman/podman.sock` does not exist. Always use `verify_host` (i.e., `podman info`) as the authoritative readiness check. |
| `systemctl --user` is not available | The `podman-machine-default` WSL image has no D-Bus session bus. Commands like `systemctl --user enable podman.socket` fail silently or return a bus error. Use `podman system service` directly. |
| `setsid` is mandatory | Without `setsid`, `podman system service` is in the SSH session's process group and is killed by SIGHUP when the SSH channel closes. Plain `&` or `nohup` are insufficient on this WSL image. |
| SSH port changes on each machine boot | The SSH port assigned to `podman-machine-default` changes every time the machine starts. Never cache the port; always read it fresh from `podman system connection list --format json`. |
| `wsl --shutdown` affects all machines | Running `wsl --shutdown` terminates every WSL instance on the host, not just `podman-machine-default`. Avoid it during E2E runs or check that no other machine is in use first. |
| SIGKILL breaks pty teardown | When using `podman machine ssh` with a timeout, use SIGTERM not SIGKILL. SIGKILL tears down the pty abruptly and can kill the newly started service with it. |
