# Activity Center Build Plan

Build plan for the global activity-and-notification subsystem described in `docs/features/activity-center/01_ideas.md`. It continues the ideas document's two load-bearing decisions unchanged — a Rust-owned `TaskRegistry` as the source of truth, and app-wide Tauri events (`app.emit`/`listen`) as the progress transport, with the existing per-invoke `Channel` streams left in place for in-context route UI — and it supersedes the ideas document's "left open" framing of persistence, cancellation, notification scope, retention, and enrollment order by resolving each below. **Status: nothing in this plan is implemented yet; all phases A–G are Remaining.** Every seam it builds on already exists: the four streamed-progress commands in `src-tauri/src/lib.rs`, the `app.emit`/`listen` precedent in `src-tauri/src/agents/mod.rs`, the route-scoped `src/routes/projects/EventLog.svelte`, the module-scoped store pattern in `src/lib/registry.svelte.ts`, and the Tauri-state-registry pattern (`ActiveServer`) managed in `lib.rs` `setup`.

## Concrete Model

A new Rust module `src-tauri/src/activity/` owns the authoritative state. It holds an `ActivityState` in Tauri state (managed in `lib.rs` `setup` alongside `ActiveServer`), containing a `Mutex<HashMap<TaskId, Task>>` plus a single process-wide monotonic version counter. A **Task** is a long-running operation with a lifecycle (`running` → `succeeded`/`failed`), an optional progress fraction, a `TaskKind`, a verbatim subject string, and a `version` stamp. Producing code (the enrolled commands) calls four helper functions — `open`, `progress`, `succeed`, `fail` — each of which mutates the map under the lock, stamps the task with the next version, and broadcasts the whole task with `app.emit("activity://task-updated", &task)`. A `list_tasks` command returns the current snapshot.

The frontend has one module-scoped store, `src/lib/activity/activity.svelte.ts`, instantiated once from `src/routes/+layout.svelte` via `ensureActivity()`. It follows the spike-proven **Protocol B**: it registers `listen("activity://task-updated", …)` *before* it calls `list_tasks`, and every fold — of both the snapshot and live events — applies a strictly-greater-`version` guard so the snapshot and the live stream are commutative and no update is lost or reordered. The store keeps the task ledger as `Record<TaskId, Task>` (folded by id) and an append-only Event ring buffer (container lifecycle from the existing `listen_events` channel, plus a synthetic line per terminal task transition). It exposes derived counts for the badge and a transition subscriber that raises a `sonner` toast when a task reaches a terminal state.

The UI surface is `src/lib/activity/ActivityCenter.svelte`, mounted once in the root layout: a bell button with a badge (running + unseen failures) and an expandable panel that lists active tasks with progress bars and the recent event scrollback. It generalizes — and replaces — `EventLog.svelte`, which is removed from `src/routes/projects/+page.svelte`. A `sonner` `<Toaster />` is mounted once in the layout.

Tasks and Events are deliberately two separate stores (the ledger ticks live; the event log is append-only scrollback) so progress bars never pollute the scrollback and log lines never masquerade as tasks.

## Resolved Decisions

**Persistence across app restart**: In-memory only for v1 — `ActivityState` is never written to disk. The `settings::save` JSON pattern is *not* extended to a `tasks.json`. Rationale: a running task cannot survive the `RunEvent::Exit` shutdown (so a persisted "running" row would resurrect as a lie), and a completed-task history across restarts is not a stated requirement. The registry is dropped on exit like `ActiveServer`.

**Hydration-vs-stream race**: Resolved by the 2026-06-29 spike with **both** measures. A single process-wide `AtomicU64` (`ACTIVITY_VERSION`, mirroring `SESSION_COUNTER` in `src-tauri/src/agents/mod.rs`) is `fetch_add(1)`-bumped on every mutation and stamped onto the task as `version`; the frontend store registers its `listen` subscription *before* awaiting `list_tasks`, and folds snapshot entries and live events through a strictly-greater-`version` guard. Neither measure alone suffices (see `01_ideas.md` Validation).

**OS-native notifications in v1**: In-app `sonner` toasts only. `tauri-plugin-notification` is **deferred** (Phase G) to avoid a new plugin dependency and a runtime permission-grant flow; the in-app toast is the reliable baseline. The store's transition subscriber is the single seam where an OS notification (behind a window-unfocused check) is added later.

**Which existing flows enroll in v1**: `download_model` (Phase D) and `start_inference` (Phase E) — the user's named motivating cases — in that order. `install_agent` and `provision_sandbox_host` are **deferred to Phase F**. The container `listen_events` stream feeds the Event half from Phase C. Each enrolled command keeps its existing `Channel` for in-context UI and *additionally* drives the registry.

**Cancellation**: Read-only observation in v1 — the panel cannot cancel a task. Rationale: cancellation requires each command to store an abort handle in the registry (a strictly larger contract than progress reporting). The existing `stop_inference` command remains the way to stop a running server; it is unrelated to the activity contract. Phase F notes where an abort handle would slot into `Task` if cancellation is later scoped.

**Event-log retention and scope**: The Event half is a frontend in-memory ring buffer capped at 500 lines (FIFO eviction), holding container lifecycle events (global, not filtered per sandbox — matching current `EventLog.svelte` behaviour) plus one synthetic line per terminal task. The Task ledger is bounded in the registry: on every terminal transition, terminal tasks beyond the newest 50 are evicted; `running` tasks are never evicted.

**Badge unread semantics**: The badge counts `running` tasks plus *unseen* failures. A `succeeded` task auto-dismisses its toast and adds nothing to the badge. Opening the panel calls `markSeen()`, which zeroes the unseen-failure count; per-item dismissal in the panel also clears that item.

## Remaining: Phase A — Rust activity module, registry, and commands

New module `src-tauri/src/activity/` with `types.rs` and `mod.rs`, wired into `src-tauri/src/lib.rs` via `mod activity`.

### `src-tauri/src/activity/types.rs`

```rust
pub type TaskId = String;

#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind { ModelDownload, InferenceSetup, HostProvision, AgentInstall }

#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus { Running, Succeeded, Failed }

// Broadcast verbatim over `activity://task-updated` and returned by `list_tasks`.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: TaskId,
    pub kind: TaskKind,
    pub version: u64,            // monotonic stamp; the frontend folds by strictly-greater version
    pub status: TaskStatus,
    pub progress: Option<f32>,   // 0.0..=1.0; None = indeterminate (step-based work without a fraction)
    pub target: Option<String>,  // verbatim subject shown next to the translated kind label, e.g. a model id
    pub detail: Option<String>,  // latest verbatim backend fact, e.g. "1.4 / 2.5 GB" or "Resolving model id"
    pub error: Option<String>,   // set only on Failed; the verbatim error message
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
}
```

No `prose` lives in Rust: the human-facing label is derived frontend-side from `kind` via Paraglide (see Phase B), and `target`/`detail`/`error` are verbatim backend data shown as-is, exactly as `RuntimeSetupEvent.detail` is in `src-tauri/src/inference/types.rs`.

### `src-tauri/src/activity/mod.rs`

```rust
pub mod types;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use types::{Task, TaskId, TaskKind, TaskStatus};

static ACTIVITY_VERSION: AtomicU64 = AtomicU64::new(0); // process-wide, mirrors SESSION_COUNTER

pub struct ActivityState {
    tasks: Mutex<HashMap<TaskId, Task>>,
}
pub type TaskRegistry = std::sync::Arc<ActivityState>;

const MAX_TERMINAL_TASKS: usize = 50;
const EVENT_NAME: &str = "activity://task-updated";
```

Helper functions (all `&AppHandle`-driven, best-effort emit like the agent-auth handlers — a dropped emit never fails the underlying work):

- `pub fn open(app: &AppHandle, id: &str, kind: TaskKind, target: Option<String>)` — inserts (or replaces) a `Running` task with a fresh version and `started_at_ms`, then emits. Replacing an existing id by natural key is intentional for singletons (see "Task identity" below).
- `pub fn progress(app: &AppHandle, id: &str, fraction: Option<f32>, detail: Option<String>)` — updates `progress`/`detail`, bumps version, emits. No-op if the id is absent.
- `pub fn succeed(app: &AppHandle, id: &str, detail: Option<String>)` — sets `Succeeded`, `ended_at_ms`, bumps version, emits, then evicts terminal tasks beyond `MAX_TERMINAL_TASKS`.
- `pub fn fail(app: &AppHandle, id: &str, error: String)` — sets `Failed`, `error`, `ended_at_ms`, bumps version, emits, then evicts.

Each helper takes the `Mutex` lock, calls `ACTIVITY_VERSION.fetch_add(1, Ordering::Relaxed)`, writes the stamp, clones the `Task` out of the lock, drops the lock, and only then calls `app.emit(EVENT_NAME, &task)` (emit outside the lock so a slow IPC never holds the registry). A private `evict_terminal(map)` keeps the newest 50 terminal tasks by `ended_at_ms` and all `running` tasks.

A small `now_ms()` helper uses `SystemTime::now().duration_since(UNIX_EPOCH)`, same as `new_session_id` in `src-tauri/src/agents/mod.rs`.

### `list_tasks` command in `src-tauri/src/lib.rs`

```rust
#[tauri::command]
fn list_tasks(activity: State<'_, TaskRegistry>) -> Vec<Task>; // infallible snapshot, sorted by started_at_ms
```

Returns a plain `Vec<Task>` (not `Result`) because a snapshot read cannot meaningfully fail — the precedent is `test_model` returning `Option<String>` without `Result` (`src-tauri/src/lib.rs`). No new `AppError` variant is needed in Phase A; the helpers are infallible best-effort. `ActivityState` is managed in `setup`: `app.manage(std::sync::Arc::new(activity::ActivityState::default()))` alongside `app.manage(ActiveServer::default())` (`src-tauri/src/lib.rs` lines 608–611). `list_tasks` is added to the `invoke_handler!` list.

**Task identity** (the opening command's responsibility, per `01_ideas.md`): singletons derive the id from a natural key so a re-run replaces the prior row — `inference-setup` (one app-wide server, the `ActiveServer` rule), `model-download:<model_id>`, `host-provision:<host>`, `agent-install:<sandbox>:<agent>`. Inherently repeatable work would mint a fresh id via a timestamp+counter like `new_session_id`; none of the v1 enrollments need that.

## Remaining: Phase B — Frontend store and types

### `src/lib/activity/types.ts`

Mirrors the Rust enums and `Task` struct (camelCase, matching `serde(rename_all = "camelCase")`), the way `src/lib/models/types.ts` mirrors the inference types:

```ts
export type TaskKind = "model-download" | "inference-setup" | "host-provision" | "agent-install";
export type TaskStatus = "running" | "succeeded" | "failed";
export interface Task {
  id: string;
  kind: TaskKind;
  version: number;
  status: TaskStatus;
  progress: number | null;
  target: string | null;
  detail: string | null;
  error: string | null;
  startedAtMs: number;
  endedAtMs: number | null;
}
export interface ActivityEvent { id: number; ts: number; text: string; level: "info" | "error"; }
```

### `src/lib/activity/activity.svelte.ts`

A `.svelte.ts` rune module mirroring `src/lib/registry.svelte.ts` (module-level `$state`, a `started` guard):

- `let tasksById = $state<Record<string, Task>>({});` and `let events = $state<ActivityEvent[]>([]);` and `let unseenFailures = $state(0);`.
- `export function tasks(): Task[]` — values sorted by `startedAtMs`, running first.
- `export function badgeCount(): number` — running count + `unseenFailures`.
- `export function markSeen(): void` — sets `unseenFailures = 0`.
- `export async function ensureActivity(): Promise<void>` — guarded by `started`; **Protocol B order**:
  1. `await listen<Task>("activity://task-updated", (e) => fold(e.payload));` (subscribe first).
  2. `const snapshot = await invoke<Task[]>("list_tasks"); for (const t of snapshot) fold(t);`
  3. start the container Event stream: `const ch = new Channel(); ch.onmessage = pushContainerEvent; await invoke("listen_events", { onEvent: ch });` (lifted from `EventLog.svelte`).
- `function fold(t: Task)`: `const cur = tasksById[t.id]; if (!cur || t.version > cur.version) { tasksById[t.id] = t; onTransition(cur, t); }` — the strictly-greater-version guard is the race fix.
- `function onTransition(prev, next)`: if `next.status` is terminal and `prev?.status !== next.status`, push a `sonner` toast (success auto-dismiss; failure sticky), append a synthetic `ActivityEvent`, and if failed increment `unseenFailures`.
- The Event ring buffer caps at 500: `events = [...events, ev].slice(-500)`.

`ensureActivity()` is called once from `src/routes/+layout.svelte`'s `<script>` (top level, not inside a component lifecycle that a route owns), so the subscription is established before any route mounts and is never torn down.

### Paraglide keys (`messages/en.json`, mirrored to `de.json`/`ru.json`)

Per `AGENTS.md`, add to `messages/en.json` first then mirror. Labels keyed by `TaskKind` (kebab→snake), following the `runtime_step_<id>_*` precedent in `src/lib/models/RuntimeSteps.svelte`:

- `task_model_download_label` ("Downloading model"), `task_inference_setup_label` ("Starting model"), `task_host_provision_label` ("Provisioning host"), `task_agent_install_label` ("Installing agent").
- `activity_toast_succeeded` ("{label} finished"), `activity_toast_failed` ("{label} failed"), `activity_title` ("Activity"), `activity_empty` ("No background activity"), `activity_running` ("Running"), `activity_badge_tooltip`.

The store holds an explicit `Record<TaskKind, () => string>` map to Paraglide's `m` functions (Paraglide generates one function per key, so dynamic indexing of `m` is not possible — same constraint `RuntimeSteps.svelte` documents).

## Remaining: Phase C — Global UI surface and EventLog supersede

### `src/lib/activity/ActivityCenter.svelte`

Mounted once in `src/routes/+layout.svelte`. A fixed bottom-right bell `Button` (existing `src/lib/components/ui/button`) showing `badgeCount()`; clicking toggles an expandable panel. The panel renders two sections: **active/recent tasks** (each row: translated kind label + `target`, a progress bar from `progress` or an indeterminate spinner when `null`, and `detail`), and the **event scrollback** (the ring buffer, reusing the markup currently in `EventLog.svelte`). Opening the panel calls `markSeen()`. Status icons/colours reuse the `RuntimeSteps.svelte` convention (`✓`/`✗`/`◐`).

### `sonner` toasts

Add the component once via `pnpm dlx shadcn-svelte@latest add sonner` (per `AGENTS.md` "Adding new components"; this also adds it to the `src/lib/components/ui/` inventory documented there). Mount a single `<Toaster />` in `src/routes/+layout.svelte`. The store calls `toast.success(...)` / `toast.error(...)` from the transition subscriber.

### Supersede: remove route-scoped `EventLog.svelte`

`src/routes/projects/EventLog.svelte` and its mount in `src/routes/projects/+page.svelte` (line 135, `<EventLog onStatusChange={applyStatus} />`) are **removed**. The container-event handling and `onStatusChange` callback move: the `listen_events` channel is now owned by the store (Phase B), and the projects page subscribes to the store for the `applyStatus` status-mapping instead of mounting its own log. Record for future agents: do **not** reintroduce a per-route `EventLog`; the activity center is the single global surface. The `ACTION_STATUS` map (container action → sandbox status) moves into the store so the projects page reads derived per-name status from it.

## Remaining: Phase D — Enroll `download_model`

`download_model` (`src-tauri/src/lib.rs` line 487) delegates to `inference::download_gguf` (`src-tauri/src/inference/mod.rs` line 71), which already streams `ModelDownloadEvent` over its `Channel`. Enrollment wraps that flow without removing the `Channel`:

- At the top of the command: `activity::open(&app, &format!("model-download:{model_id}"), TaskKind::ModelDownload, Some(model_id.clone()))`.
- Inside `download_gguf`'s progress loop (where `ModelDownloadEvent::Progress { downloaded_bytes, total_bytes }` is sent, line ~85+), additionally call `activity::progress(&app, &id, Some(downloaded/total), Some("<gb> / <gb>"))`. The `Channel` send stays for the `/models` in-context progress bar.
- On `Ok`: `activity::succeed(&app, &id, None)`. On `Err(e)`: `activity::fail(&app, &id, e.to_string())` before propagating — wrap the body so an early `?` return still records the failure (a `match` on the result, or a small guard struct, since `download_gguf` returns `Result`).

The task id `model-download:<model_id>` is a per-model singleton, so re-downloading replaces the prior row. The `download_gguf` early-return for an already-present file (line 83) opens-then-immediately-succeeds the task, which is correct (a no-op download shows as a completed task).

## Remaining: Phase E — Enroll `start_inference`

`start_inference` (`src-tauri/src/lib.rs` line 515) streams `RuntimeSetupEvent` over its `Channel` from `InferenceEngine::start`. Enrollment, with task id `inference-setup` (app-wide singleton, matching the `ActiveServer` one-model-at-a-time rule):

- `activity::open(&app, "inference-setup", TaskKind::InferenceSetup, Some(model_id.clone()))` at the top of the command.
- As each `RuntimeSetupEvent` is produced, map it to a registry update. Because setup is step-based, `progress` is `completed_steps / setup_steps().len()` and `detail` is the current step's verbatim `detail` (or the step id). `StepFailed { error }` → carry the error into the eventual `fail`.
- On the trait returning `Ok(handle)` (terminal `Ready`): `activity::succeed(&app, "inference-setup", Some(handle.base_url.clone()))`. On `Err(e)`: `activity::fail(&app, "inference-setup", e.to_string())`.

The mapping lives in `start_inference` itself (which already owns the `on_progress` channel and the `Result`), not inside the engine trait, so `InferenceEngine` implementations stay unaware of the activity subsystem. Because `start_inference` already stops any prior server first, re-running naturally replaces the single `inference-setup` row.

## Remaining: Phase F — Enroll `install_agent` and `provision_sandbox_host` (deferred)

Same wrapping pattern, deferred to keep the v1 change narrow:

- `install_agent` (`src-tauri/src/lib.rs` line 393) → `open` with id `agent-install:<name>:<agent>`, `TaskKind::AgentInstall`, mapping `AgentInstallEvent` (from `agents::install_packages`, `src-tauri/src/agents/mod.rs` line 32) to `progress`, `succeed` on the final `Done`.
- `provision_sandbox_host` (`src-tauri/src/lib.rs` line 355) → `open` with id `host-provision:<name>`, `TaskKind::HostProvision`, mapping the backend's provision progress to `progress`.

**Cancellation hook (not built):** if cancellation is later scoped, `Task` gains a non-serialized abort handle (e.g. a `tokio::sync::Notify` or `AbortHandle`) held registry-side and a `cancel_task(id)` command, paralleling `cancel_agent_auth` (`src-tauri/src/lib.rs` line 434). v1 ships without it.

## Remaining: Phase G — OS-native notifications (deferred, optional)

Add `tauri-plugin-notification`, request permission once, and in the store's `onTransition` subscriber fire an OS notification *only* when the main window is unfocused/minimized (so it complements rather than duplicates the in-app toast). Gated behind the v1 decision to ship in-app toasts first; the transition subscriber is the only seam that changes.

## Lifecycle / Cross-Cutting Concerns

**Registry lifetime**: `ActivityState` is in-memory in Tauri state, created in `setup` and dropped on exit. No `RunEvent::Exit` cleanup is needed (unlike `ActiveServer`, which holds an OS process) — a task still `running` at exit simply vanishes, consistent with the no-persistence decision.

**Emit outside the lock**: every helper clones the `Task` out of the `Mutex` and drops the guard before `app.emit`, so an IPC stall never blocks a concurrent producer. The `ACTIVITY_VERSION` `AtomicU64` is bumped under the lock so version order matches insert order.

**Race contract is split across two files**: the monotonic `version` is produced in `src-tauri/src/activity/mod.rs`; the strictly-greater-version fold and the subscribe-before-snapshot order are enforced in `src/lib/activity/activity.svelte.ts`. Both halves are required (spike, `01_ideas.md` Validation) — a future agent must not "simplify" either side away.

**Failure mapping is the command's job**: the registry has no visibility into a command's `Result`. Each enrolled command must call `activity::fail` on its error path before propagating, or a failed operation leaves a task stuck `running`. Wrap the command body so an early `?` still records the failure.

**Event name charset**: Tauri 2 event names allow `[a-zA-Z0-9-/:_]`, so `activity://task-updated` is valid; keep producers and the `listen` subscriber on the exact same constant.

**Single Toaster**: `<Toaster />` is mounted exactly once in `src/routes/+layout.svelte`; mounting it per-route would stack duplicate toast containers.

**Derived sandbox status**: the projects page's container-status overlay (`applyStatus`/`ACTION_STATUS`, `src/routes/projects/+page.svelte`) now reads from the store's container-event handling rather than a locally-mounted `EventLog`; the store is the single owner of the `listen_events` channel after Phase C.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A progress update is lost at startup | The store awaited `list_tasks` before registering `listen`, leaving a gap | Register `listen("activity://task-updated", …)` *first*, then await `list_tasks` (Protocol B order in `ensureActivity`) |
| A stale snapshot clobbers a newer live event | Folds applied last-writer-wins without a version guard | Fold both snapshot and live events with a strictly-greater-`version` guard; the `ACTIVITY_VERSION` stamp makes the two sources commutative |
| Duplicate ledger rows for one operation | Updates appended instead of folded by id | Derive a stable natural-key id at `open` time (`inference-setup`, `model-download:<id>`) and fold by id |
| A failed operation shows as `running` forever | The command returned `Err` without calling `activity::fail` | Wrap each enrolled command body so the error path calls `activity::fail` before propagating |
| Registry blocks under load | `app.emit` was called while holding the `Mutex` | Clone the task out and drop the lock before emitting |
| Task labels blank in non-English locales | Copy was put in Rust, or a `TaskKind` has no Paraglide key | Keep prose out of Rust; map `kind` → `task_<kind>_label` in the store's explicit `m` lookup, pass only verbatim `target`/`detail` from Rust |
| "Running" task resurrected as a lie after restart | A persisted ledger rehydrated a task the exit shutdown actually killed | Do not persist the registry; it is in-memory only in v1 |
| Container log and task progress bleed together | Events and Tasks stored in one list | Keep the Task ledger (`tasksById`) and the Event ring buffer (`events`) as two separate `$state` stores |
| Two `EventLog`/`Toaster` surfaces appear | A per-route `EventLog` or `<Toaster />` was reintroduced after Phase C removed it | The activity center is the single global surface mounted once in `+layout.svelte`; never re-add a per-route copy |
| `listen`/`emit` never connect | Producer and consumer used slightly different event-name strings | Both sides reference the one `EVENT_NAME` constant; do not inline the literal in two places |
