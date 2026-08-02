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
`window.__TAURI_INTERNALS__.invoke(...)`; `invokeCmd()` in `e2e/lib/mock.ts` wraps this (see
`light/local-llm-ui.spec.ts`'s direct `get_settings`/`set_settings` calls for the pattern it
replaces). They run against the real Tauri commands, but every command that would otherwise touch
podman/mutagen instead resolves against the in-memory mock (see below), so results are
deterministic and instant.

Every scripting spec (one that calls `configureMock()`) must call `useMockControl()` once at
`describe` scope — it registers `before`/`beforeEach`/`after` hooks that reset the director, which is
mandatory because the light suite runs one app session per spec file and overrides otherwise leak
across `it` blocks (see Mock control plane below).

## Mock control plane: scripting the process layer (`e2e/lib/mock.ts`)

The real `PodmanCliBackend`/`PodmanMachineManager`/`MutagenCli` structs run unmodified against a
scripted `ProcessRunner` — a shared `MockDirector` (`src-tauri/src/mockctl/mod.rs`) intercepts the
external `podman` or `mutagen` call each subsystem method shells out to, so a running spec can
provoke a failure or a rare gating condition without a hand-written subsystem-level mock to keep in
sync — exactly the paths
this tier exists for (Decision rule 3). There is no separate mock struct and no separate Rust test
tier: overrides are keyed `<binary>.<subcommand>` (e.g. `podman.create`, `mutagen.sync-list`), and
every scriptable scenario is proven by a light spec instead of a `#[test]` fn.

- `configureMock(key, behaviour)` installs one override; `resetMocks()` clears every override;
  `useMockControl()` wires both into `before`/`beforeEach`/`after` hooks — call it once at `describe`
  scope in any spec that scripts mocks. The `beforeEach` reset makes each `it` block start from the
  built-in defaults, so a spec must install its overrides inside the `it` block that needs them; an
  override installed in the spec's own `before` hook is wiped before the first test runs.
- `key` is `<binary>.<subcommand>` (`podman.start`, `podman.ps`, `podman.machine-list`, ...) — see
  `KNOWN_CALLS` in `src-tauri/src/mockctl/mod.rs` for the full scriptable list. An unknown key
  rejects the `configureMock()` call immediately.
- `behaviour` payloads are the process's raw text output, not a typed value: `fail-once`/
  `fail-always` take `exit_code` (non-zero) and `stderr`, so the real subsystem's own `check()`
  function maps the failure to the same `BackendError`/`SyncError` a genuine failed process would
  produce; `return-value` takes `stdout` (and an optional `exit_code`); `return-sequence` consumes
  one `{ stdout, exit_code }` entry per call, then the default resumes.
- With no override installed, `podman.create`/`start`/`stop`/`rm` update a small in-memory
  container list inside `ScriptedPodmanRunner` and `podman.ps` reports it back — enough for a
  spec's own `createProject()` → row-appears round trip to work without scripting `podman.ps` by
  hand for the common case. A spec that needs specific `ps` output overrides `podman.ps` directly,
  which always takes precedence.
- `podman.machine-list` defaults to reporting `podman-machine-default` as already running, so
  `check_podman_ready` passes without a spec needing to script machine readiness itself (mirrors
  the previous revision's always-ready `MockMachineManager`).
- `RuntimeManager`/`NetworkManager` are not wired to the shared runner yet (no current spec needs
  them — `docs/features/scriptable-mock-backend/04_improvements.md`); scripting backend-emitted
  events is deferred there too — the frontend's `plugin:event|emit` bridge (see `emitTask()` in
  `light/activity-center-ui.spec.ts`) remains the only synthetic-event mechanism.
- `mutagen.sync-*` keys script `MutagenCli`'s own `sync` subcommands (`light/sync-status-ui.spec.ts`
  scripts `mutagen.sync-list`). Mutagen `forward`/`version`/`daemon` calls have no key and are not
  scriptable: the free functions in `src-tauri/src/sync/mod.rs` reach the sidecar through `run()`
  with an `AppHandle` rather than a `ProcessRunner`, so `mock_configure` rejects a
  `mutagen.forward-*` key — script those flows through `MockNetworkManager` instead.
- `light/sandbox-start-failure-ui.spec.ts` is the reference spec — copy its shape for a new
  scripted scenario.

## Mocked tier design: `MIO_MOCK_BACKEND`

The backend injects several traits as `Arc<dyn …>` Tauri state:

- `SandboxBackend` (`src-tauri/src/backends/types.rs`) and `MachineManager`
  (`src-tauri/src/machine/types.rs`) are always the real `PodmanCliBackend`/`PodmanMachineManager`
  structs, constructed by `default_backend()`/`default_machine_manager()`
  (`backends/mod.rs`/`machine/mod.rs`) with a `runner: Arc<dyn ProcessRunner>` — real
  (`SystemPodmanRunner`) or scripted (`ScriptedPodmanRunner`) depending on `MIO_MOCK_BACKEND=1`. See
  "Mock control plane" above; there is no separate mock struct for either subsystem.
- `RuntimeManager` — `src-tauri/src/runtime/types.rs`, real impl `PodmanMiseRuntime`, mock impl
  `MockRuntime` in `src-tauri/src/runtime/mock.rs`, both constructed in `default_runtime()`
  (`runtime/mod.rs`). Not yet wired to the shared `ProcessRunner` (`04_improvements.md`).
- `SyncEngine` — `src-tauri/src/sync/types.rs`, always the real `MutagenCli`, constructed by
  `default_sync_engine()` (`sync/mod.rs`) with its own `runner: Arc<dyn ProcessRunner>` —
  `SystemMutagenRunner` or `ScriptedMutagenRunner`, keyed `mutagen.sync-*`. Separate from the podman
  runner because the sidecar is resolved through the `AppHandle`. Only `MutagenCli`'s own `sync`
  subcommands are scriptable; the `forward`/`version` free functions in `sync/mod.rs` still call the
  sidecar directly (they are `NetworkManager`'s calls, deferred in `04_improvements.md`), and
  `MockNetworkManager` keeps them off the light tier's path.

Setting `MIO_MOCK_BACKEND=1` swaps every subsystem onto its mock/scripted path. This mirrors the
existing env-steering precedent (`PODMAN_CONNECTION`, `SANDBOX_WORKSPACE_ROOT`, `MIO_TEST_MODEL`):
read once with `std::env::var(...)`, no caching.

**The machine-lifecycle gap.** `check_podman_ready` / `start_podman_machine`
(`src-tauri/src/lib.rs`) do **not** go through `Arc<dyn SandboxBackend>` — they receive an
`Arc<dyn MachineManager>` (the global-machine trait: `machine_is_running`,
`check_other_machines_running`, `run_machine_start`, `run_wsl_shutdown`, `activate_podman_socket`,
`poll_ready`). Under `MIO_MOCK_BACKEND=1`, `podman.machine-list`'s built-in default (no override
installed) reports `podman-machine-default` as already running, so the light `projects` flow gets
past the start-machine gate without touching WSL/podman and without any spec needing to script it.
`start_podman_machine` still emits its `activity::open`/`activity::succeed` sequence so the UI's
podman-startup activity card behaves the same.

**`wdio.conf.ts` wiring:**
- `suites.light = ["./test/specs/light/**/*.ts"]`.
- `onPrepare` skips `provisionMachine`/`ensureMiseBaseImage`/the `podman machine list` check when the
  run targets only the `light` suite. The `pnpm tauri build --debug` step still runs — the binary is
  still needed.
- `beforeSession` sets `MIO_MOCK_BACKEND=1` in the spawned app's env (via `spawn(..., { env })`,
  alongside `PODMAN_CONNECTION`) whenever **every** spec in that worker lives under
  `test/specs/light/`. It decides from the resolved `specs` argument, not the CLI flags: the tier is
  a property of where a spec lives, so `--suite light`, `--spec test/specs/light/<file>.ts` and an
  ad-hoc single-file run all get the mock. Without that, a light spec runs against real podman and
  mutates the developer's host — `settings-debugging-ui.spec.ts` used to delete the podman machine's
  WSL distro this way (`docs/features/scriptable-mock-backend/04_improvements.md`).

## Dashboard specs: `workspace-dashboard.page.ts`, no window switching needed

A sandbox's Dashboard (package/agent/ports management) is a DOM region of the single
`main-navigation-panel` webview (docs/features/consolidate-app-container/02_dev.md Phase C), not a
separate WebDriver window handle — there is nothing to switch to or back from. Call
`workspaceDashboard.waitForRendered(sandbox)` (`e2e/test/pageobjects/workspace-dashboard.page.ts`)
after `row.openWorkspace()` or any action that returns the client-side view to Dashboard, then use
its other getters/actions directly. Use `captureDiagOnFailure()` (`e2e/lib/diag.ts`) for the
standard failure-dump `afterEach`, same as any other spec.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A light spec's `it` block sees the built-in mock defaults instead of the override the spec installed | `useMockControl()` (`e2e/lib/mock.ts`) resets the director at `beforeEach` as well as `before`/`after`, so an override installed in the spec's own `before` hook is cleared before the first test runs | Install every override inside the `it` block that consumes it, as all four light specs do (`test/specs/light/sandbox-start-failure-ui.spec.ts` is the reference shape); the same reset is what stops an override leaking into later tests |
| `mock_configure` rejects a `mutagen.forward-*` key | `forward_create`, `forward_terminate` and `sweep_stale_forwards` in `src-tauri/src/sync/mod.rs` call the sidecar through `run()` with an `AppHandle`, so no `ProcessRunner` key exists for Mutagen forward subcommands | Script the flow through `MockNetworkManager` (`src-tauri/src/network/mock.rs`) instead; extending the seam is deferred in `docs/features/scriptable-mock-backend/04_improvements.md` |
| A light spec's scripted `podman.ps` fixture reports an unexpected sandbox image version, or `readBaseImage()` throws "Could not parse BASE_IMAGE / BASE_IMAGE_VERSION" | The fixtures take their image tag from `MISE_BASE_IMAGE` (`e2e/lib/podman-machine.ts`), which regex-parses `BASE_IMAGE` and `BASE_IMAGE_VERSION` out of `src-tauri/src/backends/mod.rs` — a renamed or retyped declaration defeats the parse | Keep both declarations in their current `const NAME: <type> = …` form (the comment above them says so), and never reintroduce a literal image reference into a spec — import `MISE_BASE_IMAGE` |
| A new launcher-side flag read through `argValues` (`e2e/wdio.conf.ts`) sees no value, so `IS_LIGHT_RUN` is false and `onPrepare` provisions a real Podman machine for a light run | `argValues` matches an exact flag argument or a `<flag>=` prefix only — a flag spelled any other way (a short alias, a differently punctuated form) falls through both branches | Pass `--suite`/`--spec` in either the space-separated or the `--flag=value` form, both of which `argValues` handles; a genuinely new spelling needs a new branch in `argValues` |
| A light spec in a subdirectory of `test/specs/light/` never runs under `MIO_SPEC` | The filtered `LIGHT_SPECS` glob dropped the `**/` recursion the unfiltered glob has | Every tier glob in `e2e/wdio.conf.ts` is recursive (`./test/specs/light/**/*<MIO_SPEC>*.ts`, `./test/specs/standard/**/*.ts`, `./test/specs/heavy/**/*.ts`) — keep the `**/` when editing one, and keep the filtered and unfiltered light globs at the same depth |

## Wait budgets: use the named constants from `lib/timeouts.ts`

Every WebDriver wait takes its timeout from a named budget in `e2e/lib/timeouts.ts` rather than a
literal. A budget names *what is being waited on* (`DEPLOY`, `PACKAGE_INSTALL`, `SUBPROCESS`), so
two waits on the same kind of work stay equal by construction — `deployTo`/`undeploy` in
`test/pageobjects/project-row.component.ts` are the pair that motivated the file. Pick the budget
describing the slowest real step in the wait and let the number follow; add a new constant only when
no existing one describes the work, and put the rationale for its size in its doc comment rather
than at the call site.

Raising a budget only makes a failing wait slower, while lowering one can break a passing spec — so
when several literals collapse onto one name, take the largest of the group.

Two things deliberately stay outside the file: mocha's `this.timeout(...)`, which is a per-spec
ceiling over many waits rather than a wait, and the `spawnSync` budgets in `lib/podman-machine.ts`,
which bound host provisioning that runs before WebDriver exists.

## Failure diagnostics: use `captureDiagOnFailure()` from `lib/diag.ts`

Any spec that wants a page dump on failure (URL + page source + screenshot to `e2e/.diag`) should
call `captureDiagOnFailure()` from `e2e/lib/diag.ts` rather than hand-rolling the `afterEach` — that
block used to be copy-pasted verbatim across five places and is exactly the kind of drift-prone
duplication this helper removes. `useDashboardWebview()` already calls it, so dashboard specs get it
for free. For a spec that also needs a sidecar artifact (e.g. `heavy/local-llm.spec.ts` copies
`llama-server.log`), pass an `extra(diagDir, stamp)` callback — it runs after the page dump, beside it.
