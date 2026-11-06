---
type: improvements
title: "Deferred improvements"
feature: parallel-worktree-e2e
---
# Deferred improvements

Work that is worthwhile but deliberately outside the parallel-worktree scope defined in
`docs/features/parallel-worktree-e2e/01_ideas.md`.

## Headless light tier for CI

Investigated first, before the feature was narrowed to parallel worktrees on one Windows machine. The
goal was running the `light` tier with no Tauri window, so it could execute in a CI process or a
container. Deferred because the target platform is a single Windows developer machine where a window
is available, and because the parallel-worktree fixes are a prerequisite for a CI runner anyway (a CI
job is one more concurrent consumer of the same host-global resources listed in `01_ideas.md`).

The design that was reached, kept here so it does not need re-deriving:

Add a second binary target to `src-tauri/` behind a non-default Cargo feature that also enables
`tauri/test`, running on `tauri::test::MockRuntime` instead of Wry. The seam is `run()` in
`src-tauri/src/lib.rs`: extract everything from the `MockDirector` construction down to
`generate_handler![]` into a `configure<R: Runtime>(builder: Builder<R>) -> Builder<R>` that both
binaries call. `src-tauri/src/workspace/mod.rs` is already generic over `R: Runtime` in every public
function, and the five Tauri plugins in use (`opener`, `shell`, `dialog`, `updater`, `process`) are
each generic over the runtime, so no subsystem blocks the extraction.

The headless binary exposes the IPC boundary over loopback HTTP: a `POST /invoke` endpoint answered
with `tauri::test::get_ipc_response()`, and a `GET /events` Server-Sent Events stream fed by a single
catch-all `AppHandle::listen_any` subscription. The `listen_any` route is required rather than
preferred — `MockRuntime`'s `eval_script` (`src/test/mock_runtime.rs` in the vendored `tauri-2.11.2`)
stores only the *last* evaluated script in a mutex, so the webview eval path that normally delivers
events is lossy under bursts such as the `activity://task-updated` stream.

The frontend is served from the `adapter-static` output in `build/` with an injected script defining
`window.__TAURI_INTERNALS__` (`invoke` over HTTP, a local listener registry fed by the SSE stream,
`transformCallback`/`Channel` payloads dispatched by callback id, and
`metadata.currentWebview.label` reported as `main-navigation-panel`). Specs then run in headless
Chromium. `e2e/lib/mock.ts` needs no change, because `invokeCmd()` already goes through
`window.__TAURI_INTERNALS__.invoke` from inside the page.

Three findings recorded on 2026-07-26 that make the approach tractable, all still true:

- No spec under `e2e/test/specs/light/` opens a Terminal or AgentWeb overlay pane, so every light spec
  lives in the single `main-navigation-panel` webview document.
- No page object under `e2e/test/pageobjects/` calls `browser.url()`; every route change is a click
  through the SPA router, so the headless surface only needs to load the entry point once.
- `src/routes/+layout.svelte` already wraps its `getCurrentWebview()` call in a `try`/`catch` whose
  comment names plain-browser dev as a supported case, and `src/routes/+layout.ts` sets `ssr = false`
  with `prerender = true`.

Two risks were identified and not cleared: whether `create_main_window()`
(`src-tauri/src/workspace/mod.rs`) succeeds under `MockRuntime` — it must, because `main_window()`
errors with `MainWindowMissing` and the frontend's `openWorkspace()` calls `hide_active_pane`
unconditionally — and whether `scale_factor`/`inner_size`, both consumed by `logical_size()`, are
among the `Runtime` methods `MockRuntime` leaves as `unimplemented!()` (`primary_monitor`,
`available_monitors`, and `set_theme` definitely are).

Rejected alternatives, with the reason each was dropped:

- **`xvfb-run` + `WebKitWebDriver` on a Linux runner** (the standard Tauri CI recipe): requires the
  crate to build for Linux, which is unproven — `src-tauri/src/inference/mod.rs` carries five
  `#[cfg(windows)]` blocks, `src-tauri/src/process_ext.rs` is Windows-only, and the machine subsystem
  models WSL. WebKitGTK also renders differently from WebView2, so a light failure would no longer be
  evidence about the shipped app.
- **Hiding the window instead of removing it** (`visible(false)`, or an off-screen position): solves
  focus stealing and nothing else, and `webview.set_focus()` in `show_webview()`
  (`src-tauri/src/workspace/mod.rs`) exists because input focus is load-bearing for pane activation,
  so an invisible window risks changing spec behaviour.
- **Stubbing the IPC layer in TypeScript** (`mockIPC` from `@tauri-apps/api/mocks`): trivial to build
  but deletes the reason the light tier exists, reintroducing one layer higher exactly the
  hand-written-mock drift that `docs/features/scriptable-mock-backend/02_dev.md` ("Superseded")
  eliminated.
- **Pure Rust integration tests via `tauri::test`, no browser:** every light spec asserts on rendered
  UI state, so a Rust-only harness would delete the coverage rather than relocate it, contradicting
  `docs/features/scriptable-mock-backend/02_dev.md` Resolved Decision 9.

A cheaper interim option was also identified: run the existing windowed light tier on a
GitHub-hosted `windows-latest` runner. Such a runner has an interactive desktop session with WebView2
preinstalled and the light tier needs no podman, so the suite may pass there with only the
`01_ideas.md` isolation fixes applied. Not headless — it still depends on a desktop session and cannot
run in a container — but it would deliver CI without the headless binary.

## Concurrent `standard` and `heavy` tier runs

`01_ideas.md` scoped concurrency to the `light` tier and deferred everything about the
real-infrastructure tiers here. `02_dev.md` has since narrowed what remains deferred: making a
`standard` run *safe* is in scope — safe beside the light runs other worktrees are driving
(the tier-gated `~/.ssh` teardown, the worktree-scoped process filter, the slug-keyed app-data and
workspace roots) and safe against a second `standard` run, which the host-scoped lock in Phase A
refuses rather than isolates. What stays deferred is making two real-infrastructure runs *concurrent*
with each other, which needs the three host-global resources below to be isolated rather than
serialised:

- **Podman connections.** `LOCAL_CONNECTION`/`REMOTE_CONNECTION` (`e2e/lib/podman-connection.ts`) name
  fixed machines (`mio-tests`, `mio-tests-remote`) that `onPrepare` provisions and `onComplete` stops
  or removes, so two runs fight over one machine's lifecycle.
- **The Mutagen daemon and its data directory.** `data_dir()` in `src-tauri/src/sync/mod.rs` exports
  `MUTAGEN_DATA_DIRECTORY` under `app_data_dir()`, and the vendored daemon is shared by session name.
  `MutagenCli::shutdown()` issues `daemon stop` through its `ProcessRunner`, so a real run's exit stops
  the daemon a concurrent real run depends on. Redirecting the data directory is already covered by
  `02_dev.md` Phase C; the daemon lifecycle is not.
- **`~/.ssh`.** The deploy flow writes `~/.ssh/sandbox-mio/config` and an `Include` line in
  `~/.ssh/config`; two concurrent real runs write and clean the same files. `02_dev.md` Phase D only
  stops *light* runs from clobbering them, and the host lock keeps two real runs apart in time rather
  than giving each its own files.

## Skipping the per-run debug rebuild

`onPrepare` in `e2e/wdio.conf.ts` deletes `src-tauri/target/debug/sandbox-mio.exe` and runs
`pnpm tauri build --debug --no-bundle` on every run, because `tauri-build` emits no
rerun-if-changed for frontend assets and a stale exe can point at `devUrl`. Two concurrent light
runs therefore saturate the machine with two full debug builds even when neither worktree changed.
An opt-in `MIO_SKIP_BUILD=1` escape hatch for a fast iteration loop was considered and deferred out
of `02_dev.md` Phase list: it reintroduces the stale-exe failure mode the existing comment
documents, and needs its own freshness check (comparing the exe's mtime against `build/` and
`src-tauri/src/`) to be safe enough to recommend.

## `tools/` and `src-tauri/binaries/` asset duplication

`mutagen-agents.tar.gz` and the Mutagen executable exist in both `tools/` and
`src-tauri/binaries/`, staged by different code paths (`setup_e2e`/`fetch_mutagen` in `build.rs`
versus `stage_mutagen_agents`, and the copy in `onPrepare`). `02_dev.md` Phase F repoints the
`onPrepare` copy at `src-tauri/binaries/` because worktree staging has to move the assets anyway; a
full audit of which consumer needs which copy, and collapsing `tools/` to the msedgedriver
executable alone, is deferred.

## Ungated destructive teardown in `onComplete`

`onComplete` in `e2e/wdio.conf.ts` performs four cleanups, of which only the remote-machine teardown
checks the tier. `01_ideas.md` gates the `~/.ssh` block as part of its scope; the workspace-root and
model-store deletions become slug-scoped there rather than tier-gated. A broader audit — every
destructive action in the launcher stating which tier it belongs to — is deferred.
