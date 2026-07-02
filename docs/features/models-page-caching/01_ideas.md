# Models Page Caching Ideas

This document covers the ideation for making the `/models` route (`src/routes/models/+page.svelte`) feel instant on repeat visits instead of re-probing the host from scratch on every mount. The feature is entirely unimplemented today: no cache, memoization, or persisted host-spec store exists anywhere in the `inference` module or the frontend. It builds on the existing local-llm feature (`docs/features/local-llm/`), which is implemented (host inference server, model catalogue, download flow) but never addressed mount-time latency. The relevant files are `src/routes/models/+page.svelte` (the page and its `onMount`), `src-tauri/src/inference/mod.rs` (the `detect_specs` probe), `src-tauri/src/lib.rs` (the `detect_machine_specs` command and the app's `.manage()`-based state registration), and `src-tauri/AGENTS.md` (the `.manage()`-based Tauri state convention this feature follows). Caching is in-memory only, scoped to the running app process — no on-disk persistence is introduced, by explicit product decision; see Technology Choices.

## Current State

`src/routes/models/+page.svelte:184-212` runs an `onMount` that fires, in sequence, `get_settings`, `detect_machine_specs`, `test_model`, `downloaded_models`, and `inference_status` on every mount. Because this is a SvelteKit route (not a persistent SPA shell held across navigations), navigating away to `/` and back to `/models` unmounts and remounts the component, so all five `invoke()` calls re-run from zero every time the page is opened — this is the "reload" the user is seeing; it is not a literal browser/webview reload, it is `onMount` re-fetching everything with nothing cached in between.

The expensive call is `detect_machine_specs` (`src-tauri/src/lib.rs:752-759`), which runs `inference::detect_specs` (`src-tauri/src/inference/mod.rs:163-180`) inside `spawn_blocking` (so it does not freeze the Tauri event loop, but the page still awaits it before `specs` becomes non-null and the ranked model list renders). `detect_specs` does two cheap in-process reads via `sysinfo` (`sys.refresh_memory()`, `sys.refresh_cpu_all()` — CPU core count and RAM) and then, on Windows, four separate subprocess spawns, every single call:

- `gpu_names()` (`inference/mod.rs:191-196`) — spawns `powershell -Command "Get-CimInstance Win32_VideoController..."`
- `has_npu()` (`inference/mod.rs:198-207`) — spawns `powershell -Command "Get-PnpDevice..."`
- `foundry_installed()` (`inference/mod.rs:209-216`) — spawns `foundry --version`
- `foundry_installable()` (`inference/mod.rs:218-225`) — spawns `winget show Microsoft.FoundryLocal`

Each PowerShell/winget process spawn costs on the order of a few hundred milliseconds to over a second on Windows (process creation overhead dominates, not the query itself). Four of them run serially, so `detect_machine_specs` alone can take well over a second before the specs section (`data-specs-ready`) and the ranked catalogue render. `downloaded_models` (`inference/mod.rs:46-66`, referenced via `lib.rs:797`) also re-walks the model directory on disk every mount; cheaper than the PowerShell spawns but still avoidable work.

Of the six probed fields (`cpu_cores`, `total_ram_bytes`, `free_ram_bytes`, `gpu_names`, `has_npu`, `arch`, `foundry_installed`, `foundry_installable` — `src/lib/models/types.ts:1-11`), only `free_ram_bytes` (and `total_ram_bytes`, in the rare case of a hot-plugged DIMM or a Windows-reported memory-limit change) is expected to vary between mounts within a single app run. `cpu_cores`, `gpu_names`, `has_npu`, `arch`, and `foundry_installable` are effectively hardware/OS constants for the lifetime of the install. `foundry_installed` can change mid-session (the user can install Foundry Local via winget while the app is open) but not on a per-mount cadence.

Existing precedent for host-side state that survives page remounts: the app already registers several `Mutex`-guarded structs via `.manage(...)` in the Tauri builder (`src-tauri/src/lib.rs:975-986`) — `ActiveServer` (inference run state), `TaskRegistry`, `WorkspaceRegistry`, `AuthSessionMap`, `ForwardPortMap` — each injected into commands as `tauri::State`. Separately, `src-tauri/src/settings/mod.rs` persists `AppSettings` as JSON at `<app_config>/settings.json` (`load`/`save`, `src-tauri/src/settings/mod.rs:21-35`) — the only existing app-wide (not workspace-scoped) on-disk persistence in the codebase.

## Proposed Model

### Split the probe into "static" and "volatile" fields (`detect_specs`, `inference/mod.rs`)

`MachineSpecs` already separates conceptually into fields that are essentially install-lifetime constants (`cpu_cores`, `gpu_names`, `has_npu`, `arch`, `foundry_installable`) and fields that can change between mounts (`free_ram_bytes`, and less often `total_ram_bytes`, `foundry_installed`). The proposed change gives `detect_specs` two paths: a full probe (all subprocess spawns, run once) and a cheap refresh (`sysinfo` memory read only, re-using cached values for everything else). The seam is `src-tauri/src/inference/mod.rs:163-180`; the function signature grows a cache parameter (see below) but keeps the same `MachineSpecs` return shape so the frontend's `$state<MachineSpecs | null>` (`+page.svelte:26`) is untouched.

### In-process cache as `.manage()`-d Tauri state (`MachineSpecsCache`)

A new struct, e.g. `MachineSpecsCache(Mutex<Option<MachineSpecs>>)`, registered via `app.manage(MachineSpecsCache::default())` alongside `ActiveServer` and `TaskRegistry` (`lib.rs:985-986`). `detect_machine_specs` (`lib.rs:752-759`) takes an additional `State<'_, MachineSpecsCache>` parameter: on the first call in the app's lifetime it runs the full `detect_specs` probe (all four PowerShell/winget spawns) and stores the result; on every subsequent call within the same app run it re-reads `free_ram_bytes`/`total_ram_bytes` via a cheap `sysinfo` refresh and returns the cached static fields unchanged. This directly fixes the reported symptom — repeat visits to `/models` within one app session become near-instant — without touching disk or adding a persistence format. The cache is cleared (or not; app-lifetime-scoped is the simplest option) on app restart, which is acceptable because a probe that pays the one-time cost once per app launch is a different, much smaller complaint than "every page open."

Deliberately no on-disk tier: the first `/models` visit after every app launch still pays the full four-subprocess cost, and the user is fine waiting for that one-time probe rather than the app carrying a persisted-cache staleness problem (see Technology Choices below). The corollary is that the first-visit wait needs to be visibly a "probing your machine" state, not a silent blank pause — see the Loading Indicators section.

### Cache invalidation trigger for `foundry_installed` (manual refresh, not polling)

`foundry_installed` is the one field that can plausibly change *during* a running app session (user runs `winget install Microsoft.FoundryLocal` in a terminal while the app is open) without the machine restarting. Rather than re-polling it on every mount (which reintroduces a subprocess spawn per visit, defeating the cache), the proposal exposes a manual "re-detect hardware" affordance in the UI (a small button near `data-specs`, `+page.svelte:221-236`) that calls a new command variant forcing a full re-probe and overwriting the cache. This keeps the default path cache-only while giving the user an explicit escape hatch, consistent with the app's existing "instrumented action buttons" pattern (`createActionRunner`, `+page.svelte:23`).

### Frontend: no change to the `onMount` call shape

The frontend does not need new logic to benefit from the cache — `detect_machine_specs` keeps returning a `MachineSpecs` object with the same shape, so `+page.svelte:191-196` (`specs = await invoke<MachineSpecs>("detect_machine_specs")`) is unchanged. All the caching lives host-side. The only frontend addition is the optional manual re-detect button described above, which calls a new command (e.g. `redetect_machine_specs`) instead of `detect_machine_specs`.

### Loading indicators for the first-probe wait

Because the in-memory-only cache still leaves one real wait per app launch (the first `/models` visit pays the full four-subprocess cost, on the order of a second or more), the existing "detecting" state deserves to be more informative than it is today. Currently `+page.svelte:233-235` shows a single flat line, `m.models_detecting()`, with no indication of progress or that anything is actively happening beyond static text. Two independent improvements, both scoped to the existing specs section (`data-specs`, `+page.svelte:221-236`):

- **A visible busy/spinner state** on the specs card while `specs` is still `null`, using the same `loading` affordance the page already applies to buttons elsewhere (`Button` with a `loading` prop, e.g. `+page.svelte:278`, `325`) so the visual language is consistent rather than introducing a new spinner component.
- **Distinguish "first probe this session" from "cached, near-instant"**: on a cache hit the specs section can render with no loading state at all (the data is available before the section would even paint), while a cache miss shows the busy state. This is a natural side effect of awaiting `detect_machine_specs` before render either way — no extra signal from the backend is needed to tell these apart, since a cache hit simply resolves fast enough that the loading state is imperceptible.

A stretch option, left to `02_dev.md` to decide on: since the four subprocess spawns inside `detect_specs` are independent of each other, they could run concurrently (e.g. `tokio::join!` inside the `spawn_blocking` closure, or four separate blocking tasks) rather than sequentially, cutting the worst-case first-probe wait roughly to the slowest single spawn instead of the sum of all four. This is an implementation detail of the probe itself and orthogonal to the cache — it reduces the wait the loading indicator has to cover, rather than removing it.

## Technology Choices with Tradeoffs

### Cache location: Tauri-managed in-memory state (not a frontend `localStorage`/Svelte store cache)

Chosen: cache in Rust, behind `.manage()`, following the `ActiveServer`/`TaskRegistry` precedent.

Alternatives evaluated:

- **Cache in the frontend (a Svelte store or `localStorage`, read before calling `invoke`)** — rejected as the primary mechanism. The webview reloads its JS context on full page navigation in some Tauri configurations, and more importantly the probe itself (the subprocess spawns) is what's slow — caching only on the frontend still requires at least one round trip to Rust to know whether the cache is valid, and duplicates the "is this still fresh" logic that the host already owns via `sysinfo`. The host is also the only side that knows when a forced re-probe (the manual refresh button) should invalidate the cache.
- **Cache nothing frontend-side, rely purely on the host cache** — this is the chosen design; noted here because it means the frontend needs zero new state beyond what it already has.

### Persistence tier: in-memory only, no on-disk cache

Chosen: cache exclusively in the `.manage()`'d in-memory struct, scoped to the running app process. There is no on-disk persistence for machine specs.

Alternatives evaluated:

- **On-disk JSON cache (mirroring `settings::load`/`save`, `src-tauri/src/settings/mod.rs:21-35`), surviving app restarts** — rejected by explicit product decision. It would make the very first `/models` visit after an app launch instant too, but at the cost of a staleness question the in-memory cache doesn't have: stale GPU/CPU data surviving a hardware change, a VM migration, or a machine clone, plus the extra code path of reading, parsing, and validating a cache file before falling back to a fresh probe. The one-time per-launch wait is judged an acceptable, bounded cost; the user is fine waiting once per app start rather than the app carrying extra persisted state and an invalidation story for it. Revisit only if the one-time-per-launch probe cost turns out to still bother users after the in-memory cache and loading indicators ship.

### Volatile-field refresh: re-run `sysinfo` on every call rather than caching RAM too

Chosen: `free_ram_bytes` (and `total_ram_bytes`) are re-read from `sysinfo` on every `detect_machine_specs` call, never cached.

Alternatives evaluated:

- **Cache RAM too, refresh only on the manual button** — rejected. The user explicitly called out that free RAM "does" change, and it's the one field a user opening the models page is likely checking *because* it changes (deciding whether a model will fit right now). `sysinfo`'s `refresh_memory()` is an in-process read with no subprocess spawn, so there is no latency reason to cache it — only the four PowerShell/winget spawns are worth avoiding.

### Static-field probe re-trigger: explicit user action (not a TTL-based background poll)

Chosen: the cache never expires on a timer; it is invalidated only by app restart (tier 1) or an explicit user-triggered re-detect (tier 2 addition).

Alternatives evaluated:

- **TTL-based cache (e.g. re-probe every N minutes)** — rejected. CPU core count, GPU vendor/model, NPU presence, and architecture do not change on any timescale a TTL would sensibly target short of "the user physically changed hardware and rebooted," which app restart already covers. A TTL would reintroduce periodic subprocess spawns for no benefit and add a tunable with no good default.
- **Background poll on a timer while the page is open** — rejected for the same reason, plus it would spawn PowerShell processes even while the user isn't looking at the page if implemented at the app level rather than page level.

## Validation

Not yet spiked. The latency claim (four sequential PowerShell/winget subprocess spawns per `detect_machine_specs` call) is read directly from `src-tauri/src/inference/mod.rs:191-225` and is consistent with the local-llm feature's own validation notes (`docs/features/local-llm/01_ideas.md:97-114`), which used the same `Get-CimInstance`/`Get-PnpDevice`/`winget` commands during its spike but did not measure or address repeat-call cost — that feature's spike was about proving the signals exist, not about mount-time performance. Before `02_dev.md` is written, a quick timing spike is worth running: wrap each of the four subprocess calls in `detect_specs` with `std::time::Instant` logging (or time the existing `detect_machine_specs` command end-to-end from the frontend via `performance.now()` around the `invoke` call) to confirm the actual per-mount cost and which of the four spawns dominates, so the dev plan can state a concrete "before" number.

## Open Questions

- **In-memory cache scope**: should `MachineSpecsCache` be a single global (app-wide, matches the app's single inference server) or is there any future world with per-window/per-workspace specs that would make a global cache wrong? The host-sidecar architecture in `local-llm/01_ideas.md` implies one set of specs for the whole app, which argues for a global cache, but this should be confirmed against that feature's "Open Questions: Concurrency and memory pressure" item before committing.
- **Manual re-detect UX placement**: does the "re-detect hardware" affordance live inline in the specs section (`+page.svelte:221-236`) as a small icon button, or in `src/routes/settings/+page.svelte` alongside the engine picker, given settings already reads `get_settings`/`set_settings` for the inference engine? Placement affects discoverability versus clutter on the primary models flow.
- **`foundry_installed` staleness window**: is "stale until the user clicks re-detect" an acceptable UX for a user who installs Foundry Local via winget in a separate terminal while the app is open and then expects the models page to notice, or does that specific field warrant its own lightweight re-check (e.g. re-run only `foundry --version`, not all four spawns, on every mount) as a middle ground? This is the one field where "never re-poll automatically" has a real (if narrow) downside.
- **Whether `downloaded_models`'s directory walk also deserves caching**: it's much cheaper than the PowerShell spawns, but it is also work that produces the same answer between mounts unless a download just completed. Worth revisiting once the `detect_machine_specs` fix lands and it's clear whether remaining mount latency is still noticeable.
- **Concurrent vs sequential subprocess spawns in `detect_specs`**: is running the four PowerShell/winget spawns concurrently (cutting the worst-case first-probe wait to roughly the slowest single spawn) worth the added complexity for a cost that now only happens once per app launch, or is sequential simple-and-correct good enough given the loading indicator makes the wait legible? `02_dev.md` should decide with an actual measured "before" number (see Validation).
- **Loading indicator granularity**: does the busy state on the specs card need to communicate *which* of the four signals is still probing (e.g. "checking GPU… checking NPU…"), or is a single flat spinner over the whole specs section sufficient given the wait is now a one-time-per-launch event rather than a per-visit annoyance? A single spinner is the simpler default; per-field granularity would need `detect_specs` to stream partial results rather than return one `MachineSpecs` struct at the end, a bigger change than this feature's scope implies.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Cached specs go stale after the user changes hardware (new GPU, more RAM slots) without restarting the app | The in-memory cache is scoped to the running app process and never re-probes automatically | Cache invalidates naturally on app restart (tier 1); if a persisted tier ships, gate it behind the manual re-detect affordance rather than trusting old data indefinitely |
| Free RAM shown on the models page looks wrong or lags reality | Only the static fields (CPU/GPU/NPU/arch/Foundry-installable) should be cached; `free_ram_bytes`/`total_ram_bytes` must be re-read via `sysinfo` on every call | Never fold the volatile RAM fields into the cached struct's "skip re-probe" path — always re-run `sys.refresh_memory()` |
| `foundry_installed` shows "not installed" after the user installs Foundry Local mid-session | The cache treats Foundry install state as static once probed, but it can change without an app restart | Provide the manual re-detect button, and consider a cheap Foundry-only re-check on mount as a narrower fix (see Open Questions) |
| First `/models` visit after every app launch still spawns four PowerShell/winget processes and feels slow | The cache is in-memory only by deliberate choice (no on-disk tier), so it cannot help before the first probe of a session | This is expected, not a bug — cover it with a visible loading state (see Loading Indicators) rather than trying to eliminate the wait; revisit an on-disk cache only if this one-time wait proves to still bother users |
| `detect_machine_specs` command signature change breaks callers | Adding a `State<'_, MachineSpecsCache>` parameter changes the command's registration but not its IPC contract (still returns `MachineSpecs`) | Confirm no other caller depends on `detect_machine_specs` always performing a full re-probe (grep all `invoke("detect_machine_specs"` call sites) before assuming the cached fast path is safe everywhere |

