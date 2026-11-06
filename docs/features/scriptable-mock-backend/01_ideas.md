---
type: ideas
title: "Scriptable Mock Backend Ideas"
feature: scriptable-mock-backend
---
# Scriptable Mock Backend Ideas

This document covers the ideation for making the e2e `light` tier's in-memory mocks configurable at runtime, so light specs can simulate backend conditions the mocks cannot express today — command failures, non-happy sync statuses, error-kind-specific frontend branches — without touching Podman. The goal is a project-level shift: lean on the `light` tier (seconds, no Podman) for day-to-day feedback, and run the `standard`/`heavy` tiers (minutes, real machine) on a periodic cadence rather than per change. The feature is entirely unimplemented today: every mock is a fixed-behaviour struct compiled into the binary and constructed once at startup, with no channel for a running spec to alter what any mock method returns. The relevant files are the five mock implementations (`src-tauri/src/backends/mock.rs`, `src-tauri/src/runtime/mock.rs`, `src-tauri/src/sync/mock.rs`, `src-tauri/src/machine/mock.rs`, `src-tauri/src/network/mock.rs`), their `default_*()` constructors (listed in `src-tauri/AGENTS.md`), the command inventory in `src-tauri/src/lib.rs`, and the light-tier wiring described in `e2e/AGENTS.md`.

## Current State

The backend injects five subsystem traits as `Arc<dyn Trait>` Tauri state (`SandboxBackend`, `RuntimeManager`, `SyncEngine`, `MachineManager`, `NetworkManager`), each with an in-memory mock selected once at startup when `MIO_MOCK_BACKEND=1` (`mock_backend_enabled()` in `src-tauri/src/backends/mod.rs`). The mocks fall into two shapes:

- **Stateful happy-path machines**: `MockBackend` (`src-tauri/src/backends/mock.rs`) holds a `Mutex<Vec<Sandbox>>` and mutates statuses on start/stop/destroy; `MockRuntime` (`src-tauri/src/runtime/mock.rs`) tracks installed packages per sandbox. Both always succeed (the only failure they can produce is "no such sandbox").
- **Stateless constant-responders**: `MockSync` (`src-tauri/src/sync/mock.rs`) answers every `status()` call with `SyncStatus::UpToDate` and every mutation with `Ok(())`; `MockMachineManager` (`src-tauri/src/machine/mock.rs`) reports "ready" for every method.

Neither shape lets a spec provoke a `BackendError`, a `SyncStatus` other than `UpToDate`, a machine-not-ready gate, or any error-recovery UI path. `e2e/AGENTS.md` (Decision rules) names exactly those paths — "a failure/recovery path, a race, a rare gating condition" — as the light tier's purpose, yet the mocks structurally cannot produce them. Consequently light specs today only assert rendering, routing, and happy-path round-trips (see `e2e/test/specs/light/local-llm-ui.spec.ts`).

Light specs already have a working command channel into the running app: `browser.execute()` calling `window.__TAURI_INTERNALS__.invoke(...)`, plus synthetic frontend events via the `plugin:event|emit` bridge (`e2e/test/specs/light/activity-center-ui.spec.ts`). `e2e/AGENTS.md` notes no shared helper exists yet; each spec inlines the invoke plumbing. The whole light suite runs against one spawned app session (`wdio.conf.ts` `beforeSession` sets `MIO_MOCK_BACKEND=1`), so mock state persists across specs within a run — there is no per-spec reset today, specs just avoid colliding.

The frontend branches on typed error kinds (`parseError` and `AppErrorKind` in `src/lib/errors.ts`, e.g. `"UnsyncedChanges"` triggering the force-destroy confirmation dialog), so failure-path UI logic exists and is currently only testable against real infrastructure by contriving real failures — slow and awkward, which is precisely the condition `e2e/AGENTS.md` says should send a spec to the light tier.

## Proposed Model

**Superseded note (2026-07-21)**: the sections below describe the model as first proposed — a trait-level `MockDirector` consulted inside hand-written `MockBackend`/`MockSync`/`MockMachineManager` structs, addressed by `<subsystem>.<method>` keys. After a first partial implementation, this was judged the wrong seam: those mock structs never run the real `PodmanCliBackend`/`MutagenCli`/`PodmanMachineManager` orchestration logic, so scripting them proves nothing about that logic. The control-plane *mechanism* (a runtime-gated `MockDirector` behind `mock_configure`/`mock_reset`, fail-once/fail-always/return-value/return-sequence behaviours, the reset-hook requirement) carries forward unchanged; the *addressing* moves one level down, from `<subsystem>.<method>` to `<binary>.<subcommand>`, so the real structs run against a scripted process layer instead of being replaced outright. See `docs/features/scriptable-mock-backend/02_dev.md` for the current design — the "Override addressing" choice below now reads backwards and is corrected there, not here (this document is a historical record of the option space, not the current design).

### Mock control plane: test-only Tauri commands (`mock_configure`, `mock_reset`)

Two new commands in `src-tauri/src/lib.rs`, registered in `generate_handler![]` like any other command, callable from light specs through the existing `window.__TAURI_INTERNALS__.invoke(...)` channel. `mock_configure` installs one behaviour override; `mock_reset` clears all overrides (and optionally seeded state) so specs start clean. Both commands reject with an `AppError` when `mock_backend_enabled()` is false, so a stray call against a real backend is a loud failure, not a silent no-op. No new transport, no new process, no app restart per scenario — the same session serves every scenario in the suite.

### Override registry: a shared `MockDirector` injected into every mock

A single registry struct (working name `MockDirector`) holding a map from override key (subsystem + method, e.g. `sync.status`) to a scripted behaviour, guarded by a mutex, constructed in `run()` in `src-tauri/src/lib.rs` and handed as an `Arc` to each mock's constructor inside the five `default_*()` functions. Each mock method consults the director first — if an override matches, it applies; otherwise the mock's existing default behaviour runs unchanged. Real implementations never receive a director, so the production code path is untouched. Centralising the registry means one `mock_configure` command covers all five subsystems and one `mock_reset` restores everything, instead of per-subsystem command pairs.

### Behaviour vocabulary: a small closed enum, not arbitrary responses

The scripted behaviours form a deliberately small enum: fail-once, fail-always (with an error variant name and message), return-fixed-value, return-sequence (consume one entry per call, then fall back to default). Scripted failures name a variant of the owning subsystem's real error enum (`BackendError`, `SyncError`, `RuntimeError`) so the error crosses the IPC boundary as the same `AppError` kind a real failure would produce — the frontend's `err.kind` branches get exercised for real. Scripted return values ride as JSON and deserialize into the method's real return type; a value that doesn't deserialize fails the `mock_configure` call immediately rather than corrupting a later assertion.

### State seeding as a separate concern from behaviour scripting

Some scenarios are about pre-existing state rather than method behaviour — e.g. "a sandbox already exists and is deployed" without walking the UI through creation. The stateful mocks (`MockBackend`, `MockRuntime`) can expose seeding through the same control plane (a `mock_seed` command or a seed payload on `mock_configure`) that inserts entries into their existing state maps. Seeding and scripting compose: seed a deployed sandbox, script `sync.status` to return `conflicted`, then assert the badge and recovery affordance.

### Shared e2e helper: `e2e/lib/mock.ts`

A typed TypeScript helper wrapping `mock_configure`/`mock_reset` (and the existing inline invoke plumbing `e2e/AGENTS.md` already flags as duplicated), so specs read as `mock.failOnce("backend.start", "CommandFailed", "boom")` rather than raw `browser.execute` blocks. The helper is also the natural home for a `beforeEach`/`afterEach` reset hook so one spec's overrides can never leak into the next — the single-session suite makes leakage the default failure mode without a hook.

### Tier cadence: light per-change, standard/heavy periodic

The process half of the feature: once light specs can cover failure and gating paths, the verification loop becomes `pnpm run test:light` on every change (already listed in the root `AGENTS.md` Verification Commands), with `standard` runs at merge/daily cadence and `heavy` runs weekly or before releases. `e2e/AGENTS.md`'s decision rules stay intact — `standard` remains the tier that proves real behaviour and remains the source of regression truth; what changes is the *frequency* mix, not the authority ranking. The rule that every light spec covers a path whose real counterpart exists in a `standard`/`heavy` spec is what keeps the fast loop honest, and the existing gotcha ("works in light, fails against real Podman") stays in force.

## Technology Choices with Tradeoffs

### Control channel: runtime Tauri commands (not startup env presets, not a side-channel server)

Chosen: test-only Tauri commands invoked mid-session through the existing webview bridge.

Alternatives evaluated:

- **Startup scenario presets via env var** (e.g. `MIO_MOCK_SCENARIO=sync-conflict`, read at startup like `MIO_MOCK_BACKEND`) — rejected as the primary mechanism. WebdriverIO spawns the app once per session, so each scenario would need its own app spawn and session; the light tier's whole value is seconds-fast iteration inside one session. Presets also can't express mid-spec transitions ("succeed, then fail on the retry"). An env preset could complement the command channel later for whole-suite defaults, but it cannot replace per-spec runtime control.
- **A separate control HTTP server inside the app** (test builds listen on a local port; specs POST scenario JSON) — rejected. It duplicates a transport the app already has (the Tauri invoke bridge reaches every light spec today), adds a port to manage and firewall-prompt risk on Windows, and bypasses the `AppError` plumbing that makes misuse loud.
- **Per-scenario recompilation / cargo feature flags** — rejected outright; compile times would dwarf the runtime the tiers are trying to save, and scenario count would explode the build matrix.

### Override addressing: one generic string-keyed registry (not per-trait typed commands)

**Superseded note (2026-07-21)**: this subsection's "Chosen" line originally meant a `subsystem.method` key (`sync.status`, `backend.start`) addressing a trait method on a hand-written mock. That granularity is now rejected in favor of a `binary.subcommand` key (`podman.start`, `mutagen.sync-list`) addressing a scripted external-process call consumed by the real subsystem struct — see `docs/features/scriptable-mock-backend/02_dev.md`, "Decision: mocking granularity." The reasoning below for a single generic registry over typed per-knob commands still holds at the new granularity; only the key's meaning changed, not the choice to keep one central string-keyed registry over N typed commands.

Chosen: a single `mock_configure` command with a string key (now `binary.subcommand`, previously `subsystem.method`) and a behaviour payload, resolved against one central registry.

Alternatives evaluated:

- **Typed per-subsystem commands** (e.g. `mock_sync_set_status`, `mock_backend_fail_start`, one command per scriptable knob) — rejected. Compile-time safety per knob is real, but the cost is a new command + `generate_handler![]` entry + mock method for every scenario anyone ever needs, which recreates the current rigidity one knob at a time. The generic registry pays a stringly-typed key once; the drift risk is contained by validating keys against a known-key list at `mock_configure` time (unknown key → immediate error) rather than silently never matching.
- **Scenario objects defined in Rust** (a `scenarios.rs` catalogue of named multi-override bundles, specs request a scenario by name) — rejected as the base layer because it moves scenario authorship from the spec (TypeScript, next to the assertions) into the Rust crate, coupling every new test idea to a backend edit. Named bundles can be layered on top later in `e2e/lib/mock.ts` where they stay beside the specs that use them.
- **Trait-method-level addressing itself** (the feature's original choice, superseded 2026-07-21) — rejected on reflection because it requires a parallel hand-written mock struct per subsystem that reimplements orchestration logic rather than exercising it; see `02_dev.md`'s "Superseded: the trait-level design" for the full reasoning and what was discarded.

### Compile gating: always compiled, runtime-guarded (not `#[cfg(debug_assertions)]`)

Chosen: `mock_configure`/`mock_reset` compile in all profiles and hard-fail at runtime unless `mock_backend_enabled()` is true.

Alternatives evaluated:

- **Compile the commands out of release builds with `#[cfg(debug_assertions)]`** — viable (the e2e suite builds with `pnpm tauri build --debug`, so the commands would exist where needed) and it keeps test surface out of shipped binaries entirely. Rejected for the first iteration because conditional entries in `generate_handler![]` and conditionally-present director fields add cfg-branching to `src-tauri/src/lib.rs`'s state wiring for a command that is already inert without `MIO_MOCK_BACKEND=1` — and the mocks themselves ship in release binaries today under the same runtime gate, so the precedent is runtime gating. Worth revisiting if the app ever gains a security posture where even inert test commands are unacceptable; recorded as an open question.

### Error fidelity: scripted failures name real error-enum variants (not free-form error strings)

Chosen: a scripted failure specifies the owning subsystem's error variant, so serialisation through `AppError` produces the same `{ kind, message }` a real failure would.

Alternative evaluated: **generic "fail with message" strings** — rejected because the frontend explicitly branches on `err.kind` (`src/lib/errors.ts`, per the root `AGENTS.md` Error Handling section) and forbids substring matching; a mock failure that can't reproduce a real `kind` cannot test the branches that matter (e.g. `"UnsyncedChanges"` → force-destroy confirmation dialog).

## Validation

**Superseded note (2026-07-21)**: the spike below wired a director into `MockSync` (trait-level addressing). Its three cleared risks are about mechanism that survives the addressing change unchanged — `Arc` sharing across constructor seams, a scripted value crossing the IPC boundary to the rendered UI, and override persistence across `it` blocks all still apply verbatim to a `ProcessRunner`-scripted `MutagenCli` (`02_dev.md` Phase D), since none of the three depended on *how* the mock decided what to return, only on the `Arc<MockDirector>`-as-Tauri-state plumbing and the IPC round-trip. No re-spike judged necessary before starting Phase C/D.

Spiked 2026-07-14; all three risks cleared. The spike wired a minimal director into `MockSync` only — a `MockSyncDirector` struct holding `Mutex<Option<SyncStatus>>`, constructed in `run()` (`src-tauri/src/lib.rs`), registered via `app.manage()`, and passed as a second `Arc` argument into `default_sync_engine()` (`src-tauri/src/sync/mod.rs`) — plus a temporary `mock_sync_force_status` command and a throwaway light spec. The spike code was removed after the run; only this record remains.

Risks cleared, each by a passing assertion (3 passing, 271ms total, one shared app session):

- **`Arc` sharing compiles across the constructor seams** — `cargo check` passed with the director held simultaneously as Tauri state and as a `MockSync` field; the `default_sync_engine()` signature change was the only ripple.
- **A scripted value crosses the IPC boundary and reaches the rendered UI** — `sync_status` returned `"up-to-date"` before the override and `"conflict"` after; a fabricated `SessionInfo` from the overridden `sessions()` reached the projects page's 3-second deployments poll and painted the `conflict` status text on the sandbox row (`StatusRow` in `src/routes/projects/SandboxItem.svelte`, translated via `translateStatus()` in `src/lib/sandbox.ts`).
- **Overrides persist across `it` blocks** — a second `it` block observed the previous block's override with no re-configuration, confirming the shared-session leakage that makes the reset hook (see Proposed Model, `e2e/lib/mock.ts`) mandatory rather than optional.

Blockers hit during the spike, both environmental:

- `LINK : fatal error LNK1104: cannot open file '...\target\debug\deps\sandbox_mio.exe'` — a stale `sandbox-mio.exe` from a previous e2e session was still running and held the output file lock; killing the process cleared it. Same family as the `killStaleDevProcesses()` sweep in `e2e/wdio.conf.ts`, which kills stale podman/gvproxy/win-sshproxy but not the app binary itself.
- `element ("a[href='/projects']") still not displayed after 10000ms` — every pre-existing light spec currently fails because the nav-tabs rework (commit `d2559d5`) replaced the layout's `a[href='...']` anchors with bits-ui `Tabs.Trigger` buttons (`src/lib/components/GlobalNav.svelte`) and the page objects (`goHome()` in `e2e/test/pageobjects/app.page.ts`, `navLink` in `e2e/test/pageobjects/projects.page.ts`) were not updated. Unrelated to this feature, but it must be fixed before the light tier can be leaned on; the spike spec bypassed the page objects and drove the tab button directly.

One tooling observation for `02_dev.md`: `wdio run wdio.conf.ts --suite light --spec <file>` did not narrow the run — all seven light spec files executed despite the `--spec` flag. Single-spec iteration (a core light-tier use case) needs a working filter mechanism.

## Open Questions

- **Override key integrity**: is a runtime known-key list inside `mock_configure` enough to prevent key/method drift, or should the key enum be generated or unit-tested against the trait definitions so a renamed trait method fails the build rather than a test run?
- **Subsystem scope for v1**: do all five mocks receive the director in the first pass, or only the ones with immediately valuable failure scenarios (`SandboxBackend`, `SyncEngine`, `MachineManager` for the podman-startup-recovery paths), leaving `RuntimeManager`/`NetworkManager` until a spec needs them?
- **Reset policy**: does `mock_reset` run from a suite-wide `beforeEach` hook in `e2e/lib/mock.ts` (safe default, small per-test cost) or explicitly per spec (leakage risk, zero hidden behaviour)? The single-app-session design makes the answer load-bearing.
- **Event scripting**: should the control plane also let specs push synthetic backend events through `MockBackend::listen_events`'s currently-inert channel, or does the existing frontend-side `plugin:event|emit` bridge (used by `e2e/test/specs/light/activity-center-ui.spec.ts`) remain the sole synthetic-event mechanism? Backend-emitted events would exercise the real event plumbing; the frontend bridge is already proven.
- **Latency simulation**: is a delay behaviour (scripted slow responses, for loading-state and race assertions) in scope for v1, or deferred until a concrete spec needs it? Delays interact with spec timeouts and could make the light tier less "instant" by construction.
- **Release-build posture**: does the runtime-only gate on `mock_configure` remain acceptable long-term, or should a later pass move the control plane behind `#[cfg(debug_assertions)]` once the wiring is stable (see Technology Choices, compile gating)?
- **Cadence codification**: where does the light-per-change / standard-periodic / heavy-rare cadence get recorded so agents follow it — the Verification Commands section of the root `AGENTS.md`, `e2e/AGENTS.md`'s decision rules, a CI workflow, or all three? Today no CI configuration exists in the repository, so "periodic" currently means a human or agent habit.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `mock_configure` succeeds but the mock behaviour never changes | The override key does not match any method the director-aware mocks consult (typo or renamed trait method) | Validate keys against a known-key list inside `mock_configure` and reject unknown keys with an `AppError`; never let an unmatched key install silently |
| A light spec passes or fails depending on which specs ran before it | The light suite runs one app session end-to-end, so director overrides and seeded state persist across spec files unless reset | Call `mock_reset` from a shared hook in `e2e/lib/mock.ts` (see Open Questions, reset policy) rather than trusting each spec to clean up |
| A scripted failure renders a generic error path instead of the variant-specific UI | The scripted error did not map to a real error-enum variant, so `AppError` serialised with the wrong `kind` and the frontend `err.kind` branch never matched | Script failures by naming the owning subsystem's error variant (`BackendError`/`SyncError`/`RuntimeError`), not by free-form message |
| `mock_configure` rejects with an error in a normal (non-light) app run | Both control-plane commands hard-fail unless `MIO_MOCK_BACKEND=1`, by design | Expected behaviour, not a bug — the commands are inert outside the light tier; run under `MIO_MOCK_BACKEND=1` |
| A failure path passes in the light tier but the real backend behaves differently | The light tier proves UI/store logic against scripted behaviour, not that the real subsystem actually produces that behaviour | Keep the `e2e/AGENTS.md` pairing rule: every scripted light scenario mirrors a failure mode a `standard`/`heavy` spec (or the real implementation's error mapping) demonstrably produces |
| `LNK1104: cannot open file '...\target\debug\deps\sandbox_mio.exe'` during the e2e `tauri build --debug` step | A `sandbox-mio.exe` process from a previous e2e session is still running and holds the output file lock; `killStaleDevProcesses()` in `e2e/wdio.conf.ts` sweeps podman/gvproxy/win-sshproxy but not the app binary | Kill the stale `sandbox-mio` process running out of `src-tauri/target/debug`, or extend the `killStaleDevProcesses()` sweep to include it |
| Every light spec fails with `element ("a[href='/projects']") still not displayed` | The nav-tabs rework (commit `d2559d5`) replaced nav anchors with bits-ui `Tabs.Trigger` buttons in `src/lib/components/GlobalNav.svelte`; the page objects in `e2e/test/pageobjects/` still select `a[href='...']` | Update `goHome()` in `e2e/test/pageobjects/app.page.ts` and the `navLink` getters to drive the tab trigger buttons |
