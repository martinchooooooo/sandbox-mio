# E2E Test Tiering Strategy

## Context

The e2e suite drives the real Tauri app through `tauri-driver` and, today, **every run pays the full
podman cost**: `onPrepare` in `e2e/wdio.conf.ts` unconditionally provisions both the `mio-tests` and
`mio-tests-remote` machines, builds the base image, and runs `pnpm tauri build --debug` before a
single spec executes. We have already started separating *heavy* specs (real GGUF download, model
load, mutagen forward) into `test/specs/heavy/` behind a named WDIO `heavy` suite, and a de-facto
*light* pattern exists (`*-ui.spec.ts` drive the UI via synthetic `__TAURI_INTERNALS__.invoke`
events and cheap commands) — but those light specs still wait on podman because the split is only at
the spec-glob level, not the provisioning level.

We want a **deliberate three-tier strategy** so an agent writing a new e2e test knows exactly which
tier it belongs to, and so the cheap tiers run cheaply. This deliverable is **documentation only** —
no code changes — capturing the taxonomy, the decision rules, and the design for the mocked tier so
implementation phases can follow. The chosen mock mechanism is **Rust env-gated mock trait impls**
(the backend already exposes the right seams).

## Scope of this deliverable

Write the strategy as long-lived agent guidance. **No `wdio.conf.ts`, Rust, or spec changes in this
pass.** Two files:

- **`e2e/AGENTS.md`** (new) — the canonical tier taxonomy + "which tier does my test belong to"
  decision rules + the mocked-tier design spec, so any agent authoring a spec picks correctly.
- **`e2e/README.md`** (update) — replace the single-line "real podman machines / preferred above all
  others" framing with the three-tier model and point to `AGENTS.md` for the rules.

## The three tiers

| Tier | Folder / suite | Cost | Provisioning needed | What it proves |
|------|----------------|------|---------------------|----------------|
| **light** (mocked) | `test/specs/light/**` → `light` suite | seconds, no podman | none (mock backend) | UI logic, routing, store fold/badge/gating, command round-trips against a mock |
| **standard** | `test/specs/standard/**` → default `specs` glob | minutes, real machine | `mio-tests` (+`mio-tests-remote` for deploy) | real sandbox lifecycle, mise, mutagen sync/forward, SSH config |
| **heavy** | `test/specs/heavy/**` → `heavy` suite (already exists) | many minutes, large assets | real machine + downloads | full download→load→inference, deploy-forward, end-to-end thoroughness |

Today's `*-ui.spec.ts` map to **light**; everything currently at `test/specs/*.ts` maps to
**standard**; `test/specs/heavy/**` is unchanged.

### Decision rules (to live in `e2e/AGENTS.md`)

1. **Does the assertion need real container/host state** (a real `podman exec`, a real mutagen
   session, a real SSH alias)? → **standard**.
2. **Does it download a large asset or load a real model/engine, or chain a real remote deploy**? →
   **heavy**.
3. **Otherwise** (it asserts on UI rendering, navigation, gating logic, store behaviour, or a cheap
   command's shape) → **light**, written against the mock backend with no podman.
4. Prefer the **lowest** tier that can prove the behaviour. A light spec is the default; promote only
   when an assertion genuinely requires real infrastructure. Where useful, a feature carries a
   light/standard pair (the existing `local-llm-ui` + `heavy/local-llm` precedent).

## Mocked tier design (Rust env-gated mocks) — spec to document, not yet build

The backend already injects three traits as `Arc<dyn …>` Tauri state, all swappable in one place:

- `SandboxBackend` — `src-tauri/src/backends/types.rs:113`, impl `PodmanCliBackend`, constructed in
  `default_backend()` (`backends/mod.rs:83`).
- `RuntimeManager` — `src-tauri/src/runtime/types.rs:31`, impl `PodmanMiseRuntime`,
  `default_runtime()`.
- `SyncEngine` — `src-tauri/src/sync/types.rs:52`, impl `MutagenCli`.

All Tauri command bodies (`lib.rs:217-446`) delegate to these, so a mock needs no command changes.

**Design to capture in the doc:**

1. Add `Mock{Backend,Runtime,Sync}` impls (in-memory state machine — e.g. `create`→appends a
   `Sandbox`, `start`/`stop` flip status, `list` returns the vec, `listen_events` replays synthetic
   events) gated by a new env var **`MIO_MOCK_BACKEND=1`** read inside the three `default_*()`
   functions. When set, return the mock; otherwise the real podman-backed impl. This mirrors the
   existing env-steering precedent (`PODMAN_CONNECTION`, `SANDBOX_WORKSPACE_ROOT`, `MIO_TEST_MODEL`).
2. **Close the machine-lifecycle gap.** `check_podman_ready` / `start_podman_machine` (`lib.rs:336`)
   call free functions in `backends/mod.rs:599-692` (`machine_is_running`, `run_machine_start`,
   `activate_podman_socket`, …) that bypass the trait. Document the chosen seam: have these two
   commands short-circuit to a "ready" result when `MIO_MOCK_BACKEND=1` (simplest), so the light
   `projects` flow gets past the start-machine gate without touching WSL/podman.
3. **Wire the light suite in `wdio.conf.ts`:** add `suites.light = ["./test/specs/light/**/*.ts"]`;
   in `onPrepare`, **skip `provisionMachine`/`ensureMiseBaseImage`** when the run targets only the
   light suite, and set `MIO_MOCK_BACKEND=1` in the app's env (passed through `beforeSession`'s
   `spawn(..., { env })`, alongside `PODMAN_CONNECTION`). The `tauri build --debug` step stays (the
   binary is still needed); only the podman/machine work is skipped.
4. Light specs keep using the established `__TAURI_INTERNALS__.invoke` / synthetic-event style for
   event-driven state (as `activity-center-ui.spec.ts` already does), now backed by the mock so the
   real `projects` lifecycle commands also resolve deterministically.

This item (3 phases: mock impls → machine-gate seam → conditional provisioning + light suite) is the
"larger plan" and is **documented here as the roadmap**, to be implemented in a follow-up once the
taxonomy doc lands.

## `e2e/package.json` scripts (documented target, implemented later)

- `test` → default `specs` (standard) — unchanged behaviour.
- `test:light` → `wdio run wdio.conf.ts --suite light` (no podman).
- `test:standard` → explicit alias of the default suite.
- `test:heavy` → unchanged.

## Critical files

- **Create**: `e2e/AGENTS.md` — taxonomy table, decision rules, tier-folder layout, the
  `MIO_MOCK_BACKEND` mocked-tier design + roadmap, script map.
- **Update**: `e2e/README.md` — reframe intro to three tiers, link to `AGENTS.md`, keep the page-object
  and test-machine sections.
- **Reference only** (named in the doc as the future implementation surface, not edited now):
  `e2e/wdio.conf.ts` (`specs`/`suites`/`onPrepare`/`beforeSession`),
  `src-tauri/src/backends/mod.rs` (`default_backend`, machine free functions),
  `src-tauri/src/runtime/mod.rs` (`default_runtime`), `src-tauri/src/sync/mod.rs`,
  `src-tauri/src/lib.rs` (`check_podman_ready`/`start_podman_machine`).

## Verification

Since this pass is documentation-only:

1. Re-read `e2e/AGENTS.md` and confirm the decision rules unambiguously classify every existing
   spec: each `*-ui.spec.ts` → light, each current `test/specs/*.ts` → standard, each
   `test/specs/heavy/**` → heavy.
2. Confirm the mocked-tier section names the exact seams an implementer touches
   (`default_backend`/`default_runtime`/`default_sync_engine`, the two machine-gate commands, the
   `onPrepare` skip, and the `beforeSession` env injection) so the follow-up needs no re-exploration.
3. No tests run in this pass. The follow-up implementation phase verifies with
   `cd e2e && pnpm run test:light` completing with **zero `podman` invocations** (no machine
   start/stop in the log) while still exercising the projects lifecycle UI.
