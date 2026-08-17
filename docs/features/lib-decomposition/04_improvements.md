# lib.rs Decomposition — Deferred Improvements

Work deliberately left out of `docs/features/lib-decomposition/02_dev.md`, recorded so a future agent does not treat any entry as an oversight.

## Route Caching in SandboxRouter

`SandboxRouter` in `src-tauri/src/services/routing.rs` resolves every route from scratch, costing one `SandboxBackend::list` call and one `SyncEngine::sessions` call per resolution (`docs/features/lib-decomposition/02_dev.md`, Resolved Decision 1). A single `open_agent_web` therefore issues two list calls before any pane work begins.

Deferred because invalidation has no clean trigger. Deploy, undeploy, and destroy are all reachable from `src-tauri/src/commands/`, but a container stopped or removed outside the app has no hook, and a stale cached route would produce a forward against a machine the sandbox no longer runs on. A correct version would invalidate off the `PodmanCliBackend::listen_events` stream in `src-tauri/src/backends/mod.rs`, which is gated on `mock_backend_enabled()` and therefore absent under the e2e `light` tier — meaning the cache would be untested on the tier that runs most often.

## PaneService Facade

Phase E of `docs/features/lib-decomposition/02_dev.md` narrows the four pane commands in `src-tauri/src/commands/panes.rs` to three or four state parameters each using `SandboxRouter` alone. A `PaneService` folding `Arc<dyn RuntimeManager>` and `Arc<dyn NetworkManager>` alongside the router would take them to one or two.

Deferred because `SandboxRouter` captures most of the reduction (25 state parameters to 15 across the five densest commands) and a second facade risks the per-domain-facade outcome rejected in `docs/features/lib-decomposition/01_ideas.md`: passthrough wrappers for domains that genuinely drive one subsystem. Worth revisiting only if a later feature adds a fifth pane command.

## IPC Surface Renames — Done

Several command names read inconsistently across domains — `open_workspace` starts a port-file watch rather than opening anything visible, and `runtime_setup_steps` in `src-tauri/src/commands/inference.rs` referred to an inference engine rather than to the `runtime/` subsystem.

Was deferred by Resolved Decision 6 in `docs/features/lib-decomposition/02_dev.md` until the decomposition landed and its `standard` tier run was green (Phase H: 13 of 15 spec files passing, the other 2 the pre-existing machine flakiness recorded above). Implemented as a follow-up: `open_workspace` renamed to `watch_workspace_ports` (`src-tauri/src/commands/panes.rs`), `runtime_setup_steps` renamed to `inference_setup_steps` (`src-tauri/src/commands/inference.rs`), both renames threaded through `src-tauri/src/commands/mod.rs`'s `generate_handler!` list and every `invoke()` call site and comment under `src/` and `e2e/`. `machine_setup_steps` in `src-tauri/src/commands/machine.rs` needed no change — the confusion was the naming collision with `runtime_setup_steps`, which the rename resolves.

## Thin-Delegation Boilerplate

Roughly 25 commands have a body that forwards to exactly one trait method, spread across `src-tauri/src/commands/packages.rs`, `src-tauri/src/commands/hosts.rs`, `src-tauri/src/commands/deploy.rs`, and `src-tauri/src/commands/image.rs`.

Deferred permanently in the current design and recorded here so the option is not rediscovered as new. A macro generating command bodies from a trait method name would remove roughly 150 lines but would hide the `#[tauri::command]` attribute, the doc comment, and the argument names — all three of which are the frontend contract an agent greps for when tracing an `invoke()` call from `src/`. Revisit only if the count grows past the point where the per-domain files stop being scannable.

## Standard Specs Fail Intermittently on This Machine, Not by the Refactor — Fixed

`e2e/test/specs/standard/expose-port.spec.ts` (5 of 6 tests) and `e2e/test/specs/standard/workspace-mount.spec.ts` (1 of 1) failed on this machine during Phases F/G's verification, and failed identically with the whole decomposition stashed at commit `74fef4a`. Both failed the same way: the sandbox's `li[data-sandbox=…]` never appears within the 10-second wait after create, and the follow-on assertions then hit `can only create exec sessions on running containers`. The backend log shows `create_sandbox` reaching its `initializing runtime` marker and `runtime.init` not returning inside the wait.

The Phase H verification run (docs-only changes, no `src-tauri/` or `src/` edits) hit the same symptom on a different pair: `workspace-mount.spec.ts` passed this time, but `package-autocomplete.spec.ts` failed 5 of 6 tests, its first failure the identical `li[data-sandbox=…] still not displayed after 10000ms`. A docs-only phase cannot change runtime behaviour, so the shifting set of failing specs across runs confirms this is machine-level flakiness in sandbox-creation timing rather than a code-path the refactor touches — 13 of 15 standard spec files passed both times.

Recorded rather than fixed at the time because it looked out of scope for this feature. A later triage traced it further: `runtime.init` was never the slow step (temporary `tracing::info!` timing spans around each of `create_sandbox`'s three steps showed `runtime.init` returning in 0ms — it is host-side `std::fs::create_dir_all`/`write` with no subprocess call at all). `store.create` was the real cost, measuring **4.3s** on a completely idle machine with no concurrent test load, for `podman create` plus SSH identity generation and key install; `ensure_base_image` added another 1.2s. That is 5.5s of `ProjectsPage.createProject()`'s 10-second (`COMMAND`) wait spent before `runtime.init` even starts, with zero margin left for any real-world variance — not a bug in `create_sandbox`, just a page-object wait budgeted for "one Tauri command round-trip behind a click" (`COMMAND`'s own doc comment in `e2e/lib/timeouts.ts`) applied to a call that actually chains multiple real podman lifecycle subprocess calls, which is what `SUBPROCESS` (30s) exists for. Fixed by changing `createProject`'s default from `COMMAND` to `SUBPROCESS` in `e2e/test/pageobjects/projects.page.ts`.

## `agents/config.rs` Constructs a `backends` Error Type

`provision_opencode` and `provision_pi` in `src-tauri/src/agents/config.rs` return `BackendError` from `src-tauri/src/backends/types.rs` — the only reference from the `agents/` subsystem to a sibling's type, against the independence rule in `src-tauri/AGENTS.md`.

Deferred because the alternative changes the IPC surface, which Resolved Decision 6 of `docs/features/lib-decomposition/02_dev.md` freezes for the whole decomposition. Both functions serialise today as `kind: "Backend"`; switching them to `AgentError` would emit `kind: "Agent"` and require a matching change under `src/`. The dependency is on an error enum only — no trait, no behaviour — so neither subsystem's mockability is affected. Revisit alongside the deferred renames, where a `kind` change is already on the table.

## Unit Tests for the Services Layer

`src-tauri/src/workspace/mod.rs` holds the crate's only unit tests. The `services/` layer introduced by `docs/features/lib-decomposition/02_dev.md` contains the pure decision logic most worth testing in isolation — the provider and exec-capability branching in `SandboxRouter`, and the teardown ordering in `src-tauri/src/services/sandbox.rs`.

Deferred because the decomposition is behaviour-preserving and adding tests inside it would obscure which failures came from the move. Covering the services layer is a natural follow-up once the phases land, and belongs in `docs/features/lib-decomposition/03_e2e.md`'s companion rather than in the refactor itself.
