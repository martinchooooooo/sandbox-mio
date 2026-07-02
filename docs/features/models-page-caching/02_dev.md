# Models Page Caching Dev Plan

This document is the build plan for `docs/features/models-page-caching/01_ideas.md`. It continues that document's core diagnosis (`detect_machine_specs` re-runs four PowerShell/winget subprocess spawns on every `/models` mount) and its in-memory-only caching approach, and it supersedes the ideas document's two-tier (in-memory + on-disk) proposal: the user explicitly rejected the on-disk tier during ideation, so this plan builds a single in-memory cache plus a loading indicator for the one wait that remains (the first probe per app launch). **Status: nothing in this feature is implemented yet.** All phases below are remaining work.

## The Concrete Model

A new `MachineSpecsCache` type, following the exact `Arc<Mutex<Option<T>>>` type-alias-plus-`.manage()` pattern already used by `ActiveServer` (`src-tauri/src/inference/types.rs:168`), `WorkspaceRegistry` (`src-tauri/src/workspace/types.rs:8`), and `AuthSessionMap` (`src-tauri/src/agents/types.rs:52`). It holds the "static" subset of `MachineSpecs` — the fields that do not change within an app run: `cpu_cores`, `gpu_names`, `has_npu`, `arch`, `foundry_installed`, `foundry_installable`. `detect_specs` (`src-tauri/src/inference/mod.rs:163-180`) is split so that on a cache hit it skips all four PowerShell/winget spawns and only re-reads RAM via `sysinfo`; on a cache miss (first call this app run, or after a forced re-detect) it runs the full probe and populates the cache. The cache lives for the lifetime of the Tauri app process — it is never written to disk and never expires on a timer; the only way to force a fresh probe is the new `redetect_machine_specs` command, wired to a manual "re-detect hardware" button in the UI. The frontend (`src/routes/models/+page.svelte`) needs no change to its `onMount` call shape — `detect_machine_specs` keeps the same `MachineSpecs` return type — but gains a visible spinner on the specs card while the (now usually instant, but on a cache miss still slow) probe is in flight, reusing the `HugeiconsIcon`/`LoaderPinwheelIcon`/`animate-spin` pattern already established in `src/routes/projects/StatusRow.svelte:1-17`.

## Resolved Decisions

Restating each Open Question from `01_ideas.md` with its resolution:

- **In-memory cache scope**: resolved as a single app-wide global, `.manage()`'d once in the Tauri builder alongside `ActiveServer`/`TaskRegistry`/etc. (`src-tauri/src/lib.rs:975-986`). The local-llm architecture already assumes one inference server for the whole app, so one set of cached specs for the whole app is consistent — there is no per-window or per-workspace variant to support.
- **On-disk cache invalidation signal**: moot — resolved by dropping the on-disk tier entirely per explicit product direction ("keep everything in memory, it is ok for the user to wait... not have the complexity of storing extra state"). No persisted cache, no persisted-cache staleness question.
- **Manual re-detect UX placement**: resolved as inline in the specs section of `/models` (`src/routes/models/+page.svelte:221-236`), not in `/settings`. The re-detect action is specific to the specs card the user is looking at when they notice stale data, so it stays local to that card rather than being buried in a separate route.
- **`foundry_installed` staleness window**: resolved as "cached with everything else, refreshed only by the manual re-detect button" — no narrower automatic re-check. A dedicated lightweight re-check for just this one field was considered in the ideas document but is not worth a second code path; the manual button already covers the (rare) case of installing Foundry Local mid-session.
- **Whether `downloaded_models`'s directory walk also deserves caching**: resolved as **out of scope for this feature**. It is materially cheaper than the PowerShell/winget spawns (a directory listing versus four process spawns) and is not the reported complaint. Revisit only if profiling after this feature ships shows it still contributes noticeable mount latency.
- **Concurrent vs sequential subprocess spawns in `detect_specs`**: resolved as **sequential, unchanged** for this feature. Now that the four spawns only run once per app launch instead of once per page visit, the added complexity of `tokio::join!`/parallel blocking tasks is not justified by the ideas document's own reasoning ("orthogonal to the cache... reduces the wait the loading indicator has to cover, rather than removing it"). Not built; not blocking.
- **Loading indicator granularity**: resolved as a single flat spinner over the whole specs section, not per-field progress. `detect_specs` returns one `MachineSpecs` struct at the end; streaming partial results (e.g. "checking GPU… checking NPU…") would require restructuring the probe into a `Channel`-based streamed command like `download_model`/`start_inference`, which is disproportionate to a wait that now only happens once per app launch.

## Implementation Phases

### Implemented: Phase A — `MachineSpecsCache` type and registration

- Add `pub type MachineSpecsCache = std::sync::Arc<std::sync::Mutex<Option<StaticMachineSpecs>>>;` to `src-tauri/src/inference/types.rs`, mirroring `ActiveServer` (`inference/types.rs:168`). Use `std::sync::Mutex`, not `tokio::sync::Mutex` — the lock is only ever held inside `spawn_blocking` (synchronous context), matching how `detect_specs` already runs off the async runtime (`lib.rs:754-757`).
- Add a private `StaticMachineSpecs` struct (`Clone`) to `src-tauri/src/inference/mod.rs` carrying exactly the six static fields: `cpu_cores: usize`, `gpu_names: Vec<String>`, `has_npu: bool`, `arch: String`, `foundry_installed: bool`, `foundry_installable: bool`.
- Register the cache in `src-tauri/src/lib.rs`'s `.setup(...)` block, next to `app.manage(ActiveServer::default())` (`lib.rs:985`): `app.manage(MachineSpecsCache::default())`.

### Implemented: Phase B — split `detect_specs` into probe + cached-refresh paths

- In `src-tauri/src/inference/mod.rs`, extract the four subprocess-calling functions (`gpu_names`, `has_npu`, `foundry_installed`, `foundry_installable`, all already defined at `inference/mod.rs:191-225`) plus `cpu_cores`/`arch` into a new private `probe_static() -> StaticMachineSpecs` function. This is a pure refactor of the existing bodies — no behavior change to any individual probe.
- Rewrite `detect_specs` (`inference/mod.rs:163-180`) to take the cache: `pub fn detect_specs(cache: &MachineSpecsCache) -> MachineSpecs`. Logic: lock the cache; if `Some(static_specs)`, clone it; if `None`, call `probe_static()`, store a clone in the cache, then drop the lock before doing anything else (never hold the mutex across the RAM read, which is independent). Always re-read `total_ram_bytes`/`free_ram_bytes` fresh via `sysinfo` (`sys.refresh_memory()`, unchanged from today) regardless of cache hit or miss, per the ideas document's resolved position that RAM is the one field the user is actively checking.
- Add `pub fn redetect_specs(cache: &MachineSpecsCache) -> MachineSpecs`, which clears the cache (`*cache.lock().unwrap() = None`) and then delegates to `detect_specs(cache)` to force a full re-probe.
- Drop the now-unused `_app: &AppHandle` parameter from `detect_specs` if nothing in `probe_static`/the RAM read actually needs the `AppHandle` (confirm by checking current usage — the existing signature takes `_app` with a leading underscore, implying it was already unused).

### Implemented: Phase C — wire the `detect_machine_specs` command to the cache

- In `src-tauri/src/lib.rs`, change `detect_machine_specs` (`lib.rs:752-759`) to take an additional `cache: State<'_, MachineSpecsCache>` parameter. Clone the `Arc` out of state (`let cache = cache.inner().clone();`) before moving it into the `spawn_blocking` closure, exactly the ownership pattern `Arc<Mutex<...>>` state types require when crossing into a blocking task.
- Call `inference::detect_specs(&cache)` inside the `spawn_blocking` closure in place of the current `inference::detect_specs(&app)` call.

### Implemented: Phase D — `redetect_machine_specs` command

- Add a new `#[tauri::command] async fn redetect_machine_specs(cache: State<'_, MachineSpecsCache>) -> Result<MachineSpecs, AppError>` in `src-tauri/src/lib.rs`, next to `detect_machine_specs`, following the same `spawn_blocking` wrapping and calling `inference::redetect_specs(&cache)`.
- Register it in the `invoke_handler` list (`lib.rs:1034` area, alongside `detect_machine_specs`).

### Implemented: Phase E — frontend: cache-transparent `onMount`, spinner, and re-detect button

- No change to the `onMount` call shape in `src/routes/models/+page.svelte` — `invoke<MachineSpecs>("detect_machine_specs")` keeps working identically; the speed-up is entirely host-side.
- Replaced the flat "detecting" text with a spinner-plus-label row using the `HugeiconsIcon`/`LoaderPinwheelIcon`/`animate-spin` pattern from `src/routes/projects/StatusRow.svelte` (`role="status" aria-live="polite"`), shown only while `specs === null`. Reused `m.models_detecting()`, which already existed in `messages/en.json` — no new key needed there.
- Added a `size="icon-xs"` ghost `Button` with `data-redetect` inside the specs section header that calls `redetectSpecs()`, which invokes `redetect_machine_specs` and re-derives `ranked` via `recommendModels`. Runs through the existing `action.run("redetect", ...)` runner so it shares busy/error UI with `download`/`start`/`stop`.
- The button's `title`/`aria-label` use the `models_redetect` Paraglide key, added to `messages/en.json`, `messages/de.json`, and `messages/ru.json` in the post-sign-off i18n pass.

## Lifecycle / Cross-Cutting Concerns

- **Cache lifetime**: exactly the Tauri app process lifetime. No explicit teardown is needed — the `Arc<Mutex<Option<StaticMachineSpecs>>>` is dropped naturally on process exit, same as every other `.manage()`'d state in `lib.rs`. There is no `RunEvent::Exit` hook to add for this feature (contrast the Mutagen daemon / inference sidecar shutdown hooks, which manage external processes this feature does not spawn).
- **Thread safety**: `std::sync::Mutex` is correct (not `tokio::sync::Mutex`) because every access happens inside `spawn_blocking` closures, which run on a blocking thread pool, not the async executor — holding a `std::sync::Mutex` lock briefly there never blocks the Tokio reactor. This matches how `detect_specs` already isolates its blocking work today (`lib.rs:754-757`, comment: "`detect_specs` shells out... run it off the async runtime").
- **No new IPC error cases**: both `detect_machine_specs` and `redetect_machine_specs` keep returning `Result<MachineSpecs, AppError>`, unchanged from today's single command. No new `AppError` variant is needed — a lock-poisoning `.unwrap()` panic is the same risk profile every other `Arc<Mutex<...>>` state type in this codebase already accepts (none of `ActiveServer`, `WorkspaceRegistry`, `AuthSessionMap` handle poisoning specially either).
- **Test-model interaction**: `test_model`/`testModelFromEnv` (`+page.svelte:198-205`) is unrelated to this cache — it prepends an e2e-only catalogue entry and does not touch `MachineSpecs`. No interaction to account for.
- **i18n**: adding `models_redetect` (and any other new key) requires updating `messages/en.json` plus every other locale file in `messages/` in the same change, per the root `AGENTS.md` translation convention — do not land the English key alone.
