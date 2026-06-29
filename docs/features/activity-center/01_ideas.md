# Activity Center Ideas

This document covers the ideation for a global activity-and-notification subsystem in sandbox-mio: a single app-wide place that tracks the progress of long-running background work (local-LLM model downloads, inference-server setup, host provisioning, agent installs, and future jobs), surfaces that progress no matter which route the user is on, and raises a notification when a job finishes or fails. The feature is unimplemented as a unified subsystem today, but the codebase already contains three uncoordinated precursors that it will absorb: the per-invoke `Channel` progress streams used by `download_model` / `start_inference` / `install_agent` / `provision_sandbox_host`, the route-scoped container-event viewer `EventLog.svelte`, and the one app-wide-event pattern proven by agent auth (`agent-auth-output-<session_id>`). The relevant seams live in `src-tauri/src/lib.rs` (command registration and Tauri-state registries), the per-feature Rust modules under `src-tauri/src/{inference,agents,backends}/`, and the frontend stores/components under `src/lib/` and `src/routes/`. This document argues where the source of truth for in-flight work should live and how progress should reach a UI that survives navigation; the build plan and tests are out of scope here.

## Current State

Every long-running operation today streams progress over a Tauri `Channel<serde_json::Value>` whose `onmessage` handler is created inside a route component, so the progress UI is bound to that component's lifetime. The seams the feature builds on, and the exact reason each is insufficient on its own:

- `src-tauri/src/lib.rs` — the streamed-progress commands `provision_sandbox_host` (line 355), `install_agent` (line 393), `download_model` (line 487), and `start_inference` (line 515) each take an `on_progress: Channel<serde_json::Value>` argument supplied by the caller. The `Channel` is point-to-point: it delivers only to the component that constructed it, and a dropped `Channel` send is a silent no-op (see `RuntimeSetupEvent::send` and `AgentInstallEvent` emission). Nothing aggregates these streams.
- `src/routes/models/+page.svelte` — constructs a `new Channel<ModelDownloadEvent>()` inside `downloadModel` (line 72) and a `new Channel<RuntimeSetupEvent>()` inside `startModel` (line 100). Both handlers write to component-local `$state` (`downloadPct`, `setupEvents`). Navigating away from `/models` destroys the component; the Rust command keeps running but its progress lands nowhere, and returning to the route shows no in-flight download. The only rehydration is `downloaded_models` (terminal state), not live progress.
- `src/routes/projects/AgentManager.svelte` — same shape for `install_agent`: a per-call `Channel<AgentInstallEvent>` writing to a component-local `installLog` (line 77). Closing the sandbox panel loses the install progress.
- `src/routes/projects/EventLog.svelte` — the closest existing thing to the target UI: a fixed bottom-docked log pane fed by the `listen_events` command's `Channel`. It is route-scoped (mounted only inside `src/routes/projects/+page.svelte`, line 135), handles only Podman container lifecycle events, keeps history in a component-local `events: string[]` that is discarded on navigation, and has no notion of a *task* with progress or a terminal success/failure outcome.
- `src/routes/agent-auth/+page.svelte` — the lone precedent for a navigation-independent stream: it consumes a Tauri **app-wide event** via `listen("agent-auth-<session_id>", …)` from `@tauri-apps/api/event` (line 4), emitted by `agents::start_auth` with `app.emit` (`src-tauri/src/agents/mod.rs`, lines 135/145/157). App-wide events are global pub-sub and outlive any one component, which is the property the activity center needs.
- Tauri-state registries in `src-tauri/src/lib.rs` — `ActiveServer`, `ForwardPortMap`, and `AuthSessionMap` are each an `Arc<Mutex<…>>` managed via `app.manage(...)` in `setup` (lines 608–611). A task registry would be managed the same way and is the natural Rust-side home for authoritative task state.
- `src/lib/registry.svelte.ts` — the precedent for a frontend global store written as a `.svelte.ts` rune module with module-level `$state`, fetched once and shared across components. A global activity store would mirror this shape and be instantiated above the router.
- `src-tauri/src/settings/mod.rs` — `settings::load` / `settings::save` persist JSON to `<app_config>/settings.json`. The precedent if the task ledger ever needs to survive an app restart.

## Proposed Model

### Task vs. Event: two distinct records

The subsystem models two different things and must not conflate them. A **Task** is a long-running operation with a lifecycle and progress: it has a stable id, a kind (`model-download`, `inference-setup`, `host-provision`, `agent-install`, …), a status (`running` | `succeeded` | `failed`), an optional progress fraction, a human-facing label, an optional verbatim backend detail line, and start/end timestamps. An **Event** is a discrete, point-in-time log line with no lifecycle — for example a Podman container `start`/`die` from `listen_events`, or a one-shot informational notice. The activity center keeps a list of Tasks (the ledger the UI ticks live) and an append-only Event log (the history `EventLog.svelte` already shows). A finished Task may *also* emit an Event for the history, but the two stores are separate so progress bars do not pollute the scrollback and log lines do not masquerade as tasks. The seam is a new typed `Task` struct (Rust) plus a `TaskKind` enum, paralleling the existing `RuntimeSetupEvent` / `AgentInstallEvent` / `ModelDownloadEvent` enums rather than replacing them.

### Source of truth in Rust: a managed task registry

A `TaskRegistry` (`Arc<Mutex<HashMap<TaskId, Task>>>`) is managed in Tauri state via `app.manage(...)` in `lib.rs` `setup`, alongside `ActiveServer` and the other registries. The Rust side owns the work, so it owns the authoritative task state. Each streamed-progress command opens a task in the registry when it starts, updates the task's progress as it works, and closes it (`succeeded`/`failed`) when it returns. Because the registry lives in Rust, in-flight work is observable even after a full webview reload (F5 / devtools reload), not only after a route change — a guarantee a frontend-only store cannot make. The seam is a new `src-tauri/src/activity/` module (mirroring `src-tauri/src/inference/`) exporting the registry type, the `Task`/`TaskKind`/`TaskUpdate` types in its `types.rs`, and a small helper that commands call to open/update/close a task.

### Progress transport: app-wide Tauri events, not per-invoke Channels

Each registry mutation is broadcast with `app.emit("activity://task-updated", &task)` (the `Emitter` trait already imported in `src-tauri/src/agents/mod.rs`). The frontend subscribes once, globally, via `listen("activity://task-updated", …)`, exactly as the agent-auth page already does for its session events. Existing per-invoke `Channel` arguments stay where they are for the *local, in-context* UI (the `/models` route still ticks its own `RuntimeSteps` checklist while the user is watching it); the command additionally writes to the registry and emits the app-wide event so the *global* surface stays in sync. The activity center is therefore an additive observer, not a rewrite of the per-feature progress flows — the `download_model` `Channel` and the `activity://task-updated` event carry the same facts to two different audiences. The seam is one extra emit per existing progress point inside `inference::download_gguf`, `InferenceEngine::start`, `agents::install_packages`, and `backends::*::provision_host`.

### Frontend global store (`activity.svelte.ts`)

A `.svelte.ts` rune module mirroring `src/lib/registry.svelte.ts`: module-level `$state` holding the task list and the event log, an `ensureActivity()` initializer that (a) calls a new `list_tasks` command to hydrate the current registry snapshot and (b) registers the global `listen("activity://task-updated", …)` and the `listen_events` container stream. Because the module's `$state` lives at module scope, not inside a component, it survives every route change. `ensureActivity()` is called once from the root layout so the subscription is established before any route mounts and is never torn down for the life of the webview. The store also exposes derived counts (running, unread-failed) for the badge. The seam is `src/lib/activity/` (`activity.svelte.ts` + a `types.ts` mirroring the Rust enums), instantiated from `src/routes/+layout.svelte`.

### Global UI surface in the root layout

The activity UI moves out of `src/routes/projects/+page.svelte` and into `src/routes/+layout.svelte` (currently only a theme/locale toolbar plus `<slot />`), so it renders on every route. The surface is two parts: a persistent affordance (a bell/activity button with a badge count of running + unseen-failed tasks) and an expandable panel that lists active tasks with progress and the recent event/history scrollback. `EventLog.svelte` is generalized into this panel rather than duplicated; its container-lifecycle handling becomes the Event half of the store. The seam is a new `src/lib/activity/ActivityCenter.svelte` mounted once in the layout, consuming the global store.

### Notifications on terminal transitions

When a task transitions to `succeeded` or `failed`, the store raises a transient notification (a toast) so a user on an unrelated route learns that the background model download finished or the provision failed, without watching the panel. Failures are sticky/dismissible and increment the badge's unseen count; successes auto-dismiss. Optionally, when the app window is unfocused or minimized, the same transition raises an OS-level notification via `tauri-plugin-notification` so the user is reached outside the app. The seam is a subscriber inside `activity.svelte.ts` that reacts to status transitions; the toast renderer is a UI component in the layout (see the toast technology choice below).

### Task identity and idempotency

A task id is assigned by the opening command and is stable for the operation's lifetime so repeated `task-updated` emits fold onto one ledger row rather than appending duplicates (the same fold-by-id discipline `RuntimeSteps.svelte` already applies to setup events by step id). For operations that are naturally singletons — one inference setup at a time (the `ActiveServer` one-model-at-a-time rule), one provision per host — the id derives from the operation's natural key (engine+model, host name) so a re-run replaces the prior row. For inherently repeatable operations the id is freshly generated. Deriving the id is the opening command's responsibility; the registry only stores what it is told.

## Technology Choices with Tradeoffs

### Source of truth: Rust task registry (not a frontend-only store)

Chosen: hold authoritative task state in a Rust `TaskRegistry` in Tauri state, with the frontend store as a cache hydrated from `list_tasks` and kept live by events.

Alternatives evaluated:

- **Frontend-only global store** (a `.svelte.ts` module owns the tasks, started from the layout) — rejected as the source of truth because it cannot survive a webview reload, and because the `Channel` `onmessage` handler that feeds it would have to be created from module scope rather than a component to outlive navigation, which is awkward and still loses everything on reload. It is retained as the *cache* layer, not the authority.
- **Persisted ledger on disk** (extend `settings::save` to write tasks) — rejected for v1. A download or provision in flight when the app quits is cancelled anyway (the `RunEvent::Exit` hook stops the inference server), so a persisted "running" task would resurrect as a lie. Persistence buys only a history of completed jobs across restarts, which is not a stated requirement; it is left as an open question.

### Progress transport: app-wide events (keep Channels for in-context UI)

Chosen: broadcast task updates with `app.emit` to a single app-wide event the global store subscribes to, while leaving the existing per-invoke `Channel` streams in place for the route that initiated the work.

Alternatives evaluated:

- **Replace every `Channel` with app-wide events** — rejected. The per-invoke `Channel` is the right tool for point-to-point, in-context progress (the `/models` `RuntimeSteps` checklist the user is actively watching); ripping it out to route everything through global events would force the local UI to filter a global stream for its own task and is a larger, riskier change than adding one emit. The two coexist: `Channel` for the foreground watcher, app-wide event for the global observer.
- **Poll a `list_tasks` command on an interval** (as `src/routes/projects/+page.svelte` already polls `sandbox_deployments` every 3 s, line 85) — rejected as the primary transport because polling adds latency to progress bars and load proportional to poll rate; the agent-auth app-wide-event precedent shows push works. Polling is retained only as the one-shot hydration call (`list_tasks`) on store init and as a possible reconciliation safety net.
- **A single shared `Channel` opened once from the layout** — rejected because a Tauri `Channel` is created by the JS caller and passed into one command invocation; there is no idiomatic way to multiplex unrelated commands' progress onto one long-lived `Channel`, whereas `app.emit`/`listen` is purpose-built for exactly that fan-out.

### Notifications: in-app toasts via shadcn-svelte `sonner`, OS notifications optional

Chosen: render in-app toasts with the shadcn-svelte `sonner` component, added via `pnpm dlx shadcn-svelte@latest add sonner` per the `AGENTS.md` "Adding new components" instruction. Treat OS-native notifications (`tauri-plugin-notification`) as an optional add for the unfocused/minimized case.

Alternatives evaluated:

- **Hand-rolled toast component** — rejected because `AGENTS.md` directs reaching for a shadcn-svelte primitive before writing custom markup, and `sonner` already handles stacking, auto-dismiss, and sticky/dismissible variants that the success-vs-failure distinction needs.
- **OS notifications only, no in-app toasts** (`tauri-plugin-notification`) — rejected as the sole mechanism. OS notifications are suppressed or coalesced by the platform when the app is focused, require a runtime permission grant, and are easy to miss; the in-app toast is the reliable baseline. The OS notification is the complement for when the app is not in front, not a replacement.
- **No toasts, badge + panel only** — rejected because the stated goal is that a user navigating elsewhere *learns* about background progress; a badge the user must look at does not actively surface a completed download, whereas a toast does.

### UI placement: root layout (not a dedicated route, not per-route duplication)

Chosen: mount the activity surface once in `src/routes/+layout.svelte` so it is present on every route and backed by the always-live global store.

Alternatives evaluated:

- **A dedicated `/activity` route** — rejected because a route the user must navigate *to* defeats the purpose: the value is ambient visibility while doing something else. A route can still exist later as a "full history" view, but it cannot be the only surface.
- **Per-route embedding** (the current `EventLog.svelte` approach, mounted in `projects/+page.svelte`) — rejected as the model to extend: it is exactly the limitation being removed, since the log vanishes off-route and each route would have to mount and reconcile its own copy.

### Task model: typed `Task`/`TaskKind` enum (not opaque JSON blobs)

Chosen: a typed Rust `Task` struct and `TaskKind` enum serialized to the frontend, mirroring the existing `RuntimeSetupEvent` / `ModelDownloadEvent` / `AgentInstallEvent` enums and the `serde(tag = "kind")` discriminated-union convention already in `src-tauri/src/inference/types.rs`.

Alternatives evaluated:

- **Opaque `serde_json::Value` per task** (as the raw container events arrive in `EventLog.svelte`, which gropes for `ev.Type ?? ev.type ?? …`) — rejected because the defensive key-probing in `EventLog.svelte` is precisely the fragility a typed model removes; the activity center owns both ends and should use a stable discriminant like the rest of the codebase.
- **Reuse one of the existing event enums for everything** — rejected because `RuntimeSetupEvent` is per-step (no terminal success/fail outcome for the whole job), `ModelDownloadEvent` is download-specific, and none carries a cross-kind label/status the global ledger needs; the `Task` type is a new, deliberately kind-agnostic envelope that *references* a kind rather than being one.

### Copy and i18n: Paraglide keys for labels, verbatim backend detail passed through

Chosen: follow the established split — translatable task labels live in `messages/*.json` keyed by `TaskKind`, exactly as `RuntimeSteps.svelte` maps step ids to `m.runtime_step_*` keys; verbatim backend facts (a model id, a port, an error string) ride in a `detail` field shown as-is, never translated, matching the `detail`/`error` fields on `RuntimeSetupEvent`.

Alternative evaluated: **labels constructed in Rust** — rejected for the same reason `src-tauri/src/inference/types.rs` keeps prose out of Rust ("No prose lives in Rust"): all human-facing copy must be translatable through Paraglide, so Rust emits only stable ids and opaque data.

## Validation

Spikes run on 2026-06-29 on the developer's Windows 11 machine.

### Transport API surface is present in the installed toolchain (cleared)

The design's two transports were confirmed against the actually-installed `@tauri-apps/api@2` type declarations rather than from memory. `grep` over `node_modules/@tauri-apps/api/event.d.ts` confirmed `listen<T>(event, handler, options?): Promise<UnlistenFn>` (line 87) and `emit<T>(event, payload?): Promise<void>` (line 129), and over `core.d.ts` confirmed `class Channel<T>` with an `onmessage` setter (lines 61/67) and `invoke<T>` (line 127). The app-wide-event half (`emit`/`listen`) and the per-invoke `Channel` half therefore both exist as the Proposed Model assumes. The Rust emit side needs no new spike: `agents::start_auth` already calls `app.emit(&name, &AgentAuthEvent::…)` through the `tauri::Emitter` trait in compiling production code (`src-tauri/src/agents/mod.rs`, lines 135/145/157), and the managed-registry pattern is already proven by `ActiveServer` / `ForwardPortMap` (`app.manage` in `lib.rs` `setup`).

### Lifetime independence of app-wide events (cleared by existing production behaviour)

The load-bearing assumption — that an app-wide event delivers progress to a listener independent of any route component's lifetime — is already demonstrated in production by the agent-auth flow: `agents::start_auth` emits `agent-auth-output-<session_id>` via `app.emit` (`src-tauri/src/agents/mod.rs`, lines 135/145/157) and `src/routes/agent-auth/+page.svelte` consumes it via `listen(...)` (line 4) in a *separate webview window*, the strongest possible form of lifetime independence. The fold-by-id rehydration discipline is likewise already proven by `RuntimeSteps.svelte`, which folds a `RuntimeSetupEvent` stream into a per-id state map, and module-scoped `$state` surviving component unmount is proven by `src/lib/registry.svelte.ts` (its `fetched` flag persists across mounts). No fresh spike adds confidence beyond the running code.

### Hydration-vs-stream race resolution (cleared by a runnable model)

The one genuinely new and unproven risk — that a `task-updated` emit can be missed or applied out of order relative to the `list_tasks` snapshot — was cleared by a standalone Rust model that brute-forces every interleaving. The model walks one task through five monotonically version-stamped updates and compares two consumer protocols against the producer's final truth. Command: `rustc -O race_spike.rs -o race_spike.exe && ./race_spike.exe`.

Result:

- **Protocol A — snapshot-then-subscribe, no version fold** (the naive ordering "await `list_tasks`, then `listen`"): of 21 racy cuts, **5 lost the terminal update** — confirmed bug. Any emit landing in the gap between the snapshot read and the subscription is delivered to neither and is lost forever.
- **Protocol B — subscribe-then-snapshot, fold by version** (register `listen` first, then read the snapshot, and apply snapshot *and* buffered live events through a strictly-greater-version guard): of **873** combinations spanning every cut **and every delivery order** of the buffered live events, **all 873 converged** to the producer's final state — confirmed safe and order-independent.

The spike resolves the open question concretely: the consumer must register the subscription *before* reading the snapshot, and every fold (snapshot entries included) must be guarded by a monotonic per-task version. Subscribe-first alone is insufficient — a late-applied snapshot can still clobber a newer live event — so the version guard is what makes the two sources commutative. This conclusion is carried into the `02_dev.md` registry contract.

### Not spiked (deferred, recorded honestly)

Whether a `#[tauri::command]` future keeps running after the SPA route that initiated it is destroyed was not exercised under a live GUI. It is not load-bearing for the chosen design — because each command updates the Rust registry directly rather than relying on the caller's `Channel`, the registry stays authoritative regardless of whether the caller's promise is still held — but a GUI confirmation (start a download on `/models`, navigate to `/projects`, observe the registry still advancing) belongs in the `03_e2e.md` plan.

## Open Questions

- **Persistence across app restart**: Should the task ledger persist (extend the `settings::save` JSON-on-disk pattern to a `tasks.json`) or stay purely in-memory in the `TaskRegistry`? In-memory is correct for *running* tasks (they cannot survive the `RunEvent::Exit` shutdown), so the question is specifically whether a *history* of completed tasks should outlive a restart, and if so where the retention cutoff is. `02_dev.md` should resolve whether v1 ships any persistence at all.
- **Hydration-vs-stream race** *(resolved by the 2026-06-29 spike — see Validation; recorded here as the origin of the decision)*: Between the `list_tasks` snapshot read and the `listen("activity://task-updated", …)` registration, an emit could be missed or double-counted. The spike showed the answer is **both** measures together, not either alone: register the subscription before reading the snapshot, *and* assign a monotonic per-task version that every fold (snapshot entries included) uses to discard stale updates. `02_dev.md` must encode both in the registry's update contract.
- **OS-native notifications in v1**: Ship in-app `sonner` toasts only, or also wire `tauri-plugin-notification` for the unfocused/minimized case (which adds a permission-grant flow and a new plugin dependency)? Decide the v1 scope and, if included, when an OS notification fires versus an in-app toast.
- **Which existing flows enroll in v1**: All four streamed-progress commands (`download_model`, `start_inference`, `install_agent`, `provision_sandbox_host`) are candidates, but enrolling all four at once widens the change. `02_dev.md` should pick the v1 set (model download + inference setup are the user's named motivating cases) and the order the rest follow.
- **Cancellation**: Should the activity panel be able to *cancel* a running task (abort a download, stop a provision), or is it read-only observation in v1? Cancellation requires each command to hold an abort handle in the registry (a larger contract than progress reporting) and intersects with the existing `cancel_agent_auth` precedent; the scope should be decided explicitly rather than implied by the UI.
- **Event-log retention and scope**: The append-only Event half (container lifecycle from `listen_events`, plus task-completion notices) currently grows unbounded in `EventLog.svelte`'s array. Decide the in-memory cap / ring-buffer size and whether container events are global or filtered per visible sandbox.
- **Badge unread semantics**: What counts toward the badge — only failures, or also completed successes until the panel is opened — and what action clears it (opening the panel, or per-item dismissal)?

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Background progress disappears when the user changes route | Progress is delivered over a per-invoke `Channel` whose `onmessage` lives in a route component; the component is destroyed on navigation and `Channel` sends become silent no-ops | Broadcast task updates as an app-wide event (`app.emit`) and subscribe once from a module-scoped store started in the root layout, mirroring the agent-auth `listen(...)` pattern |
| In-flight tasks vanish after a webview reload (F5/devtools) | A frontend-only store holds the only copy of task state and is wiped by the reload | Keep the authoritative state in the Rust `TaskRegistry` (Tauri state) and rehydrate the store via a `list_tasks` command on init |
| Duplicate ledger rows for one operation | Each `task-updated` emit is appended instead of folded onto the existing task | Assign a stable task id at open time and fold updates by id, exactly as `RuntimeSteps.svelte` folds setup events by step id |
| A progress update is missed during startup | An emit can fire between the `list_tasks` snapshot and the `listen(...)` registration | Resolve the hydration-vs-stream race (versioned tasks, or subscribe-then-snapshot) before freezing the update contract — see Open Questions |
| Task labels are not translated / appear blank in non-English locales | Human-facing copy was put in Rust, or a `TaskKind` has no Paraglide key | Keep prose out of Rust; emit stable kind ids and map them to `messages/*.json` keys as `RuntimeSteps.svelte` does, passing only verbatim backend facts in a `detail` field |
| OS notification never appears even though a job finished | The platform suppresses native notifications while the app is focused, or permission was never granted | Treat the in-app `sonner` toast as the reliable baseline; use `tauri-plugin-notification` only as the unfocused-window complement, and request permission before relying on it |
| "Running" task shown as active forever after an app restart | A persisted ledger resurrected a task that was actually killed by the `RunEvent::Exit` shutdown | Do not persist running tasks; if any persistence ships, store only terminal (`succeeded`/`failed`) history and never rehydrate a `running` status |
| Container log lines and task progress bars bleed into each other | Events (point-in-time log lines) and Tasks (lifecycle + progress) were stored in one list | Keep the Task ledger and the append-only Event log as two separate stores, as in the Proposed Model |
