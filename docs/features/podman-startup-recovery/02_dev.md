---
type: dev-plan
title: "Podman Startup Recovery Build Plan"
feature: podman-startup-recovery
---
# Podman Startup Recovery Build Plan

Build plan for graceful handling of a stopped or socket-missing Podman machine after a host restart, as described in `docs/features/podman-startup-recovery/01_ideas.md`. The ideas document's proposed model is carried forward unchanged. All design decisions from that document are resolved below. **Status: nothing in this plan is implemented; all phases A–F are Remaining.**

This feature depends on the activity center feature (`docs/features/activity-center/02_dev.md`) being built through its Phase A (Rust module with `open`/`progress`/`succeed`/`fail` helpers and `ActivityState` in Tauri state) and Phase B (frontend types in `src/lib/activity/types.ts`) before Phases E and F here can be implemented. Phases A–D are independent of the activity center and can be built first.

## Concrete Model

Two new `#[tauri::command]` functions are added to `src-tauri/src/lib.rs`:

`check_podman_ready` is a read-only probe. It calls `backends::machine_is_running()` (a new `pub(crate)` helper) to parse `podman machine list --format json` and check whether `DEFAULT_CONNECTION` has `Running: true`. If not, it returns `Err(AppError::PodmanUnavailable(...))` immediately. If the machine reports running, it additionally calls `backends::poll_ready(1)` (a single `verify_host` attempt); if that also fails (socket missing), it returns `Err(AppError::PodmanUnavailable(...))`. This command does not start anything.

`start_podman_machine` performs the full startup sequence and drives the activity center task with id `"podman-startup"`. The command calls `activity::open`, runs `backends::run_machine_start()`, applies the WSL shutdown guard via `backends::check_other_machines_running()` and `backends::run_wsl_shutdown()` if needed, calls `backends::activate_podman_socket()`, polls readiness with `backends::poll_ready(4)`, and then calls `activity::succeed` or `activity::fail` before returning. Every step calls `activity::progress` with a detail string so the activity center panel shows live status. If the command returns `Err`, it has already called `activity::fail`; the caller must not call `activity::fail` again.

`AppError::PodmanUnavailable(String)` is a new variant in `src-tauri/src/error.rs`. It carries a short reason string for display (not a machine name — the machine is always `DEFAULT_CONNECTION`). `kind()` returns `"PodmanUnavailable"`.

`TaskKind::PodmanStartup` is a new variant in `src-tauri/src/activity/types.rs`, serialised as `"podman-startup"`. The matching TypeScript union in `src/lib/activity/types.ts` and the Paraglide label key `task_podman_startup_label` in `messages/*.json` are added alongside it.

On the frontend, `src/routes/projects/+page.svelte` gains a `podmanUnavailable` state flag. `onMount` invokes `check_podman_ready` before the existing `loadProjects`/`loadDeployments`/`loadHosts` calls. If it returns `PodmanUnavailable`, the flag is set and the page renders a "Podman machine is not running" message with a "Start Podman" button in place of the sandbox list. The button invokes `start_podman_machine` via `createActionRunner`. On success, the flag is cleared and the normal load sequence runs. On failure, the activity center fires a sticky error toast and the page shows a "Retry" button.

## Resolved Decisions

**Machine name** (`01_ideas.md` Q1): hardcoded to `backends::DEFAULT_CONNECTION` (`"podman-machine-default"`) throughout. No runtime detection from `podman machine list`.

**`wsl --shutdown` guard** (`01_ideas.md` Q2): before running `wsl --shutdown`, `check_other_machines_running()` reads `podman machine list --format json` and returns `true` if any machine other than `DEFAULT_CONNECTION` has `Running: true`. If other machines are running, the shutdown step is skipped entirely and the start is retried bare. This avoids disrupting `mio-tests` and `mio-tests-remote` during E2E runs.

**Progress transport**: activity center (`activity::open`/`progress`/`succeed`/`fail` in `src-tauri/src/activity/mod.rs`). No `Channel` argument on `start_podman_machine`. The activity center's global panel and toasts carry all progress UI.

**Machine init**: out of scope. The machine is assumed to already exist. Neither command handles `podman machine init`.

**User-initiated start**: `check_podman_ready` never starts the machine. `start_podman_machine` runs only when the user clicks "Start Podman".

## Remaining: Phase A — `AppError::PodmanUnavailable`

### `src-tauri/src/error.rs`

Add one variant to the `AppError` enum after `UnsyncedChanges`:

```rust
/// Podman machine is stopped or its rootless socket is not ready.
#[error("Podman is not ready: {0}")]
PodmanUnavailable(String),
```

Add its arm to `kind()`:

```rust
AppError::PodmanUnavailable(_) => "PodmanUnavailable",
```

No new `From` impl is needed — `PodmanUnavailable` is constructed explicitly by the two commands in `lib.rs`, not via `?` from a sub-module error.

### `src/lib/errors.ts`

Add `"PodmanUnavailable"` to the `AppErrorKind` union:

```ts
export type AppErrorKind =
  | "Backend" | "Sync" | "Runtime" | "Agent"
  | "Inference" | "Settings"
  | "NotDeployed" | "UnsyncedChanges"
  | "PodmanUnavailable";
```

## Remaining: Phase B — `TaskKind::PodmanStartup` and message keys

### `src-tauri/src/activity/types.rs`

Add `PodmanStartup` to the `TaskKind` enum:

```rust
pub enum TaskKind { ModelDownload, InferenceSetup, HostProvision, AgentInstall, PodmanStartup }
```

`serde(rename_all = "kebab-case")` already on the enum serialises it as `"podman-startup"`.

### `src/lib/activity/types.ts`

Add `"podman-startup"` to the `TaskKind` union:

```ts
export type TaskKind =
  | "model-download" | "inference-setup"
  | "host-provision" | "agent-install"
  | "podman-startup";
```

### `messages/en.json`, `messages/de.json`, `messages/ru.json`

Add `task_podman_startup_label` to each locale file following the existing `task_*_label` pattern established in the activity center (`02_dev.md` Phase B). English value: `"Starting Podman machine"`. The activity center's `src/lib/activity/activity.svelte.ts` looks up this key by `TaskKind`, so the key name must be `task_podman_startup_label` exactly.

## Remaining: Phase C — Backend helpers in `src-tauri/src/backends/mod.rs`

All five helpers are `pub(crate)` free functions, not methods on `PodmanCliBackend`, because they operate on the global Podman CLI rather than on a specific connection. They call the existing `podman_global` and `verify_host` private helpers, which is why they live in the same file.

Define one new constant at the top of `src-tauri/src/backends/mod.rs`, alongside `DEFAULT_CONNECTION`:

```rust
/// Rootless Podman API socket inside the default machine (UID 1000). Must be activated manually
/// via `podman system service` because the machine's user systemd session does not start on boot.
const PODMAN_SOCKET: &str = "/run/user/1000/podman/podman.sock";
```

The five helpers:

**`pub(crate) async fn machine_is_running() -> Result<bool, BackendError>`**

Runs `podman machine list --format json` via `podman_global`. Parses the JSON array and returns `true` if the entry whose `Name` equals `DEFAULT_CONNECTION` has `Running: true`. Returns `false` if the entry is absent or `Running` is `false`. Returns `Err(BackendError::CommandFailed(...))` only if the `podman` process itself fails to run (e.g., podman not in PATH).

**`pub(crate) async fn check_other_machines_running() -> bool`**

Runs `podman machine list --format json` via `podman_global`. Returns `true` if any entry other than `DEFAULT_CONNECTION` has `Running: true`. Returns `false` on any error (conservative: if we cannot determine the state, do not disrupt others). Never returns `Err`.

**`pub(crate) async fn run_machine_start() -> Result<(), BackendError>`**

Runs `podman machine start DEFAULT_CONNECTION` via `podman_global`. Returns `Ok(())` on exit 0, `Err(BackendError::CommandFailed(stderr))` otherwise.

**`pub(crate) async fn run_wsl_shutdown() -> Result<(), BackendError>`**

Runs `wsl --shutdown` via `std::process::Command` (synchronous, wrapped in `tokio::task::spawn_blocking`). Waits 2 seconds after shutdown before returning, to allow WSL to reset. Returns `Ok(())` regardless of the exit code — shutdown failure is non-fatal; the caller retries the machine start anyway.

**`pub(crate) async fn activate_podman_socket() -> Result<(), BackendError>`**

Runs `podman machine ssh DEFAULT_CONNECTION <script>` where `<script>` is:

```
test -S /run/user/1000/podman/podman.sock || { mkdir -p /run/user/1000/podman && setsid env XDG_RUNTIME_DIR=/run/user/1000 podman system service --time=0 </dev/null >/tmp/podman-svc.log 2>&1 & }
```

The `shell` flag must be `false` when spawning the process — on Windows, `shell: true` routes through `cmd.exe`, which interprets `||`, `&`, `>`, and `$(...)` as its own metacharacters before the script reaches the remote shell (see `e2e/lib/podman-machine.ts:87`). The command is given a 7-second timeout via `tokio::time::timeout`; on timeout it is sent SIGTERM (not SIGKILL — SIGKILL tears down the pty and kills the newly started service). Always returns `Ok(())` — the readiness gate is `poll_ready`, not the SSH exit code.

**`pub(crate) async fn poll_ready(attempts: usize) -> Result<(), BackendError>`**

Calls `PodmanCliBackend::verify_host(DEFAULT_CONNECTION)` up to `attempts` times, sleeping 500 ms between each. Returns `Ok(())` on the first success. Returns `Err(BackendError::CommandFailed(...))` after all attempts are exhausted.

## Remaining: Phase D — `check_podman_ready` command

Add to `src-tauri/src/lib.rs`:

```rust
#[tauri::command]
async fn check_podman_ready() -> Result<(), AppError> {
    if !backends::machine_is_running().await
        .map_err(|e| AppError::PodmanUnavailable(e.to_string()))?
    {
        return Err(AppError::PodmanUnavailable("Podman machine is not running".into()));
    }
    backends::poll_ready(1).await
        .map_err(|_| AppError::PodmanUnavailable("Podman socket is not ready".into()))
}
```

Register in the `invoke_handler!` list in `run()`.

This command takes no state — it operates on the global Podman CLI. It does not call into the `SandboxBackend` trait.

## Remaining: Phase E — `start_podman_machine` command

This phase requires the activity center feature to be built through its Phase A (`src-tauri/src/activity/mod.rs` with `open`/`progress`/`succeed`/`fail`, and `ActivityState` managed in `setup`).

Add to `src-tauri/src/lib.rs`:

```rust
#[tauri::command]
async fn start_podman_machine(app: tauri::AppHandle) -> Result<(), AppError> {
    use crate::activity::types::TaskKind;

    activity::open(&app, "podman-startup", TaskKind::PodmanStartup, None);

    activity::progress(&app, "podman-startup", None, Some("Starting machine…".into()));
    if backends::run_machine_start().await.is_err() {
        let detail = if backends::check_other_machines_running().await {
            activity::progress(&app, "podman-startup", None, Some("Retrying…".into()));
            "Retrying…"
        } else {
            activity::progress(&app, "podman-startup", None, Some("Retrying after WSL reset…".into()));
            backends::run_wsl_shutdown().await;
            "Retrying after WSL reset…"
        };
        if backends::run_machine_start().await.is_err() {
            let err = "Failed to start Podman machine".to_string();
            activity::fail(&app, "podman-startup", err.clone());
            return Err(AppError::PodmanUnavailable(err));
        }
    }

    activity::progress(&app, "podman-startup", None, Some("Activating Podman socket…".into()));
    backends::activate_podman_socket().await;

    activity::progress(&app, "podman-startup", None, Some("Waiting for socket…".into()));
    if let Err(e) = backends::poll_ready(4).await {
        let err = format!("Podman socket did not become ready: {e}");
        activity::fail(&app, "podman-startup", err.clone());
        return Err(AppError::PodmanUnavailable(err));
    }

    activity::succeed(&app, "podman-startup", None);
    Ok(())
}
```

Register in the `invoke_handler!` list. Add `use crate::activity` to the imports at the top of `lib.rs`. `ActivityState` is already managed in `setup` by the time this command is added (activity center Phase A prerequisite).

## Remaining: Phase F — Frontend startup gate

This phase requires the activity center feature to be built through its Phase B (`src/lib/activity/types.ts` must include `"podman-startup"` in `TaskKind`, and `task_podman_startup_label` must exist in the Paraglide message files). Phase B of this plan covers the additions; the activity center's Phase B must also be complete for the store to exist.

### `src/routes/projects/+page.svelte`

Add `podmanUnavailable` state and a second `createActionRunner` for the start action. Update `onMount` to probe first:

```ts
let podmanUnavailable = $state(false);
const startAction = createActionRunner();

onMount(async () => {
    try {
        await invoke("check_podman_ready");
    } catch (e) {
        if (parseError(e).kind === "PodmanUnavailable") {
            podmanUnavailable = true;
            return;          // skip the normal load sequence
        }
        error = parseError(e).message;
        return;
    }
    loadAll();
    pollId = setInterval(loadDeployments, 3000);
});
```

Extract the three existing load calls into a `loadAll()` helper to avoid repetition. `loadAll` also starts the deployment poll.

Add a `startPodman` handler that uses `startAction`:

```ts
async function startPodman() {
    const ok = await startAction.run(
        "start",
        () => invoke("start_podman_machine"),
        { message: m.podman_starting() }
    );
    if (ok) {
        podmanUnavailable = false;
        loadAll();
        pollId = setInterval(loadDeployments, 3000);
    }
}
```

In the template, gate the existing sandbox list on `!podmanUnavailable`. When `podmanUnavailable` is true, render instead:

```html
{#if podmanUnavailable}
    <div class="mt-8 text-center space-y-4">
        <p class="text-muted-foreground">{m.podman_not_running()}</p>
        <Button
            loading={startAction.busy === "start"}
            disabled={startAction.busy !== null}
            onclick={startPodman}
        >{m.podman_start_button()}</Button>
        {#if startAction.error}
            <p class="text-destructive text-sm">{startAction.error}</p>
            <Button variant="outline" onclick={startPodman}>{m.retry()}</Button>
        {/if}
    </div>
{:else}
    <!-- existing sandbox list markup -->
{/if}
```

### `messages/en.json`, `messages/de.json`, `messages/ru.json`

Add three new keys following the `AGENTS.md` locale-file convention (add to `en.json` first, then mirror):

- `podman_not_running`: `"Podman machine is not running"` — shown as the explanation text.
- `podman_start_button`: `"Start Podman"` — the button label.
- `podman_starting`: `"Starting Podman machine…"` — the `StatusRow` message while the action is in flight.

A `retry` key may already exist; if not, add `"retry": "Retry"`.

## Lifecycle / Cross-Cutting Concerns

**Activity center dependency boundary**: Phases A–D can be built and compiled without the activity center. In Phase E, `activity::open` and friends are called directly; if the activity center is not yet wired into `setup`, those calls will panic at runtime. Build Phase E only after the activity center's Phase A is merged.

**`activity::fail` must precede every `Err` return in `start_podman_machine`**: the registry has no visibility into the command's `Result`. If the command returns `Err` without calling `activity::fail`, the `"podman-startup"` task is stuck `running` in the activity panel until the app restarts. The pseudocode above already does this; any future edits must preserve the invariant.

**`activate_podman_socket` is fire-and-forget**: the SSH command returns after the `setsid` script starts the background service; the socket may not yet be bound when the command returns. `poll_ready` is the actual readiness gate. Never interpret a zero exit from `activate_podman_socket` as "socket is ready".

**`shell: false` on `podman machine ssh`**: on Windows, spawning with `shell: true` routes through `cmd.exe`, which shreds the `||`, `&`, `>`, and `<` metacharacters in the activation script. Pass the script as a single `OsStr` argument directly to `podman`. See `e2e/lib/podman-machine.ts:87` for the confirmed working approach.

**SIGTERM, not SIGKILL, on the SSH timeout**: `tokio::time::timeout` cancels the future but does not kill the OS process. Use `tokio::process::Command` (not `std::process::Command`) so the child process can be sent SIGTERM via `child.kill()` on timeout. SIGKILL tears down the pty abruptly and can kill the newly backgrounded socket service alongside it.

**`onMount` guard**: if `check_podman_ready` returns any error other than `PodmanUnavailable` (e.g., `Backend` — podman binary not found), the projects page should show the generic `error` string, not the "Start Podman" screen. The `kind === "PodmanUnavailable"` branch in `onMount` is the discriminant; other kinds fall through to the existing `error` string.

**Poll interval guard**: `setInterval(loadDeployments, 3000)` must not be started until `podmanUnavailable` is false. Starting the poll while Podman is down floods the backend with failing `sandbox_deployments` invocations. The `onMount` and `startPodman` handlers both start the interval only after the `check_podman_ready` probe succeeds.

## Common Gotchas

| Gotcha | Detail |
|---|---|
| Machine "Running" ≠ socket ready | `podman machine list` shows `Running: true` even when the socket is absent. `machine_is_running()` returning `true` does not mean the connection works; `poll_ready` is the authoritative gate. |
| `activate_podman_socket` exit code is not the readiness signal | The SSH command returns as soon as the background `setsid` script starts, not when the socket is bound. Always follow with `poll_ready`. |
| `shell: true` shreds the activation script on Windows | `cmd.exe` interprets `||`, `&`, `>`, and `<` before they reach SSH. Always spawn `podman machine ssh` with `shell: false`. |
| SIGKILL kills the socket service | If the SSH timeout fires and the process is killed with SIGKILL, the `setsid`-backgrounded service may die with it. Use SIGTERM. |
| `activity::fail` missing before `Err` return | Without it, the activity panel shows `"podman-startup"` as running forever. Every early-return error path in `start_podman_machine` must call `activity::fail` first. |
| Deployment poll starts while Podman is down | Starting `setInterval(loadDeployments, 3000)` before `check_podman_ready` resolves floods the backend. Start the interval only after a successful probe. |
| `wsl --shutdown` during E2E run | `run_wsl_shutdown` terminates every WSL instance, including `mio-tests`. `check_other_machines_running()` guards against this, but it reads a fresh `podman machine list` at call time — a machine that starts after the check is not protected. |
