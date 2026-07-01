# E2E test tiers

Three tiers, picked by how much real infrastructure a spec's assertions need. **`standard` is the
default** — it proves real behaviour against a real machine and is what regression confidence
should be built on. `light` and `heavy` are for specific situations, not a default to reach for
first.

| Tier | Folder / suite | Cost | Provisioning needed | What it proves |
|------|----------------|------|---------------------|----------------|
| **standard** | `test/specs/standard/**` → default `specs` glob | minutes, real machine | `mio-tests` (+`mio-tests-remote` for deploy) | real sandbox lifecycle, mise, mutagen sync/forward, SSH config — the happy/common paths |
| **light** | `test/specs/light/**` → `light` suite | seconds, no podman | none (`MIO_MOCK_BACKEND=1`, mock backend) | UI logic, routing, store fold/badge/gating, command round-trips against a mock — edge cases and debugging |
| **heavy** | `test/specs/heavy/**` → `heavy` suite | many minutes, large assets | real machine + downloads | full download→load→inference, deploy-forward, end-to-end thoroughness |

## Decision rules

1. **Default to `standard`.** Write the happy/common-path assertion against the real backend first
   — a real `podman exec`, a real mutagen session, a real SSH alias. This is the tier that actually
   proves the feature works.
2. **Does it download a large asset or load a real model/engine, or chain a real remote deploy**? →
   **heavy**, regardless of how common the path is.
3. **Drop to `light` only for edge cases or debugging**, not as the first spec for a feature:
   - an edge case that's slow or awkward to provoke against real infra (a failure/recovery path, a
     race, a rare gating condition) where the assertion is really about UI/store logic once that
     state is reached;
   - fast, deterministic iteration while actively debugging a UI/store bug, before or alongside the
     `standard` spec that proves the real path.
   Written against the mock backend with no podman.
4. Where useful, a feature carries a real (`standard` or `heavy`) spec paired with a `light` one for
   exactly this reason — the real spec is the source of truth, the `light` spec covers a gating-logic
   edge case or gives a fast debug loop (e.g. `heavy/local-llm.spec.ts`'s real download→load→infer
   path paired with `light/local-llm-ui.spec.ts`'s catalogue/gating-logic edge cases).

## Writing a light spec

Only reach for this once you've confirmed the corresponding real (`standard`/`heavy`) coverage
exists or is planned — light specs are for the edge case or debug loop, not the primary proof.
Light specs drive the running webview directly via `browser.execute()` and
`window.__TAURI_INTERNALS__.invoke(...)` — no shared helper exists yet, each spec inlines this (see
`light/activity-center-ui.spec.ts`'s local `emitTask()` wrapping `plugin:event|emit`, or
`light/local-llm-ui.spec.ts`'s direct `get_settings`/`set_settings` calls). They run against the real
Tauri commands, but every command that would otherwise touch podman/mutagen instead resolves
against the in-memory mock (see below), so results are deterministic and instant.

## Mocked tier design: `MIO_MOCK_BACKEND`

The backend injects three traits as `Arc<dyn …>` Tauri state, all swappable in one place — a mock
needs no Tauri command changes:

- `SandboxBackend` — `src-tauri/src/backends/types.rs:114`, real impl `PodmanCliBackend`, mock impl
  `MockBackend` in `src-tauri/src/backends/mock.rs`, both constructed in `default_backend()`
  (`backends/mod.rs:83`).
- `RuntimeManager` — `src-tauri/src/runtime/types.rs:31`, real impl `PodmanMiseRuntime`, mock impl
  `MockRuntime` in `src-tauri/src/runtime/mock.rs`, both constructed in `default_runtime()`
  (`runtime/mod.rs:28`).
- `SyncEngine` — `src-tauri/src/sync/types.rs:52`, real impl `MutagenCli`, mock impl `MockSync` in
  `src-tauri/src/sync/mock.rs`, both constructed in `default_sync_engine()` (`sync/mod.rs:123`).

Setting `MIO_MOCK_BACKEND=1` makes all three constructors return their mock instead of the
podman/mutagen-backed real impl. This mirrors the existing env-steering precedent
(`PODMAN_CONNECTION`, `SANDBOX_WORKSPACE_ROOT`, `MIO_TEST_MODEL`): read once with
`std::env::var(...)`, no caching.

**The machine-lifecycle gap.** `check_podman_ready` / `start_podman_machine`
(`src-tauri/src/lib.rs`) do **not** go through `Arc<dyn SandboxBackend>` — they receive an
`Arc<dyn MachineManager>` (the global-machine trait: `machine_is_running`,
`check_other_machines_running`, `run_machine_start`, `run_wsl_shutdown`, `activate_podman_socket`,
`poll_ready`). `default_machine_manager()` in `src-tauri/src/machine/mod.rs` picks the implementation
once from `MIO_MOCK_BACKEND=1`: the `MockMachineManager` in `machine/mock.rs` returns a "ready"/no-op result
for every method, so the light `projects` flow gets past the start-machine gate without touching
WSL/podman. `start_podman_machine` still emits its `activity::open`/`activity::succeed` sequence so
the UI's podman-startup activity card behaves the same.

**`wdio.conf.ts` wiring:**
- `suites.light = ["./test/specs/light/**/*.ts"]`.
- `onPrepare` skips `provisionMachine`/`ensureMiseBaseImage`/the `podman machine list` check when the
  run targets only the `light` suite. The `pnpm tauri build --debug` step still runs — the binary is
  still needed.
- `beforeSession` sets `MIO_MOCK_BACKEND=1` in the spawned app's env (via `spawn(..., { env })`,
  alongside `PODMAN_CONNECTION`) when running the light suite.

## Scripts

- `pnpm test` / `test:standard` → `test/specs/standard/**` — real podman, unchanged behaviour.
- `pnpm run test:light` → `wdio run wdio.conf.ts --suite light` — no podman, mock backend.
- `pnpm run test:heavy` → unchanged.
