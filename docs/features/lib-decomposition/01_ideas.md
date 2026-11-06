---
type: ideas
title: "lib.rs Decomposition — Ideas"
feature: lib-decomposition
---
# lib.rs Decomposition — Ideas

`src-tauri/src/lib.rs` has grown to 1,805 lines holding four unrelated concerns: the full inventory of 77 `#[tauri::command]` functions, the cross-subsystem orchestration those commands call, process-wide startup wiring, and file logging setup. The intended shape — stated in `src-tauri/AGENTS.md` — is that `src-tauri/src/lib.rs` is a thin command surface over trait-injected subsystems, with orchestration living in command handlers because subsystems never depend on each other. The orchestration half of that arrangement has no home of its own, so every cross-subsystem flow accumulates in `src-tauri/src/lib.rs`. This document proposes a three-layer split (`commands/`, `services/`, existing subsystem modules), a routing seam that collapses the repeated sandbox-addressing triple, and a narrowing of which Tauri state each command can reach.

Nothing in the proposal changes a command name, an argument shape, or a return type. The SvelteKit frontend under `src/` and the WebdriverIO suite under `e2e/` are untouched by design, which is what makes the existing e2e tiers a usable regression net for the refactor.

## Current State

`src-tauri/src/lib.rs` contains, by weight:

| Concern | Approximate lines | Representative items |
|---|---|---|
| 77 command functions | 1,150 | `create_sandbox`, `open_agent_web`, `start_inference`, `provision_podman` |
| Pane preparation | 185 | `prepare_terminal_webview`, `prepare_agent_web_pane`, `read_paseo_password`, `fetch_paseo_server_info` |
| Agent config writers | 114 | `provision_opencode`, `provision_pi` |
| Startup wiring in `run()` | 100 | Process-runner construction, 14 `manage` calls, exit handler |
| Sandbox addressing | 55 | `sandbox_facts`, `resolve_machine`, `ensure_forward_alias` |
| Logging setup | 52 | `init_logging` |

Two measurements quantify the state-scope problem. `src-tauri/src/lib.rs` declares 87 `State<'_, …>` parameters across its 77 commands, and `run()` registers 14 managed state entries that any command may request. The four pane commands (`open_terminal`, `open_terminal_pane`, `open_agent_web`, `open_workspace`) declare 21 of the 87 state parameters between them, because each reaches independently for `Arc<dyn SandboxBackend>`, `Arc<dyn SyncEngine>`, `Arc<dyn RuntimeManager>`, `Arc<dyn NetworkManager>`, and `Arc<ProviderRegistry>`.

The subsystem modules the refactor builds on are unchanged by the proposal and are inventoried in `src-tauri/AGENTS.md`: `backends/`, `runtime/`, `sync/`, `network/`, `machine/`, `provider/`, `mockctl/`, `activity/`, `agents/`, `credentials/`, `inference/`, `settings/`, `ssh/`, `workspace/`, `process/`, `transport/`.

## Proposed Model: Three Layers

The proposal introduces two crate-root directories alongside the existing subsystem modules.

`src-tauri/src/commands/` holds only `#[tauri::command]` functions. A command decodes its arguments, requests the narrowest Tauri state that can serve the request, delegates, and maps the result into `AppError`. A command that needs more than two subsystems is a signal that a service is missing.

`src-tauri/src/services/` holds cross-subsystem orchestration. The `services/` layer is the only layer permitted to compose subsystems, which preserves the rule in `src-tauri/AGENTS.md` that subsystem modules never depend on each other. Flows with load-bearing ordering constraints — the microsandbox branch of `create_sandbox`, the teardown sequence in `undeploy_sandbox` and `destroy_sandbox` — move here and gain a named function instead of living inline in a command body.

Subsystem modules keep their existing `types.rs` + `mod.rs` layout and their trait/real-impl/mock structure. No subsystem gains a dependency on a sibling subsystem.

`src-tauri/src/lib.rs` retains the `mod` declarations, `run()`, and the `devtools` feature gate — approximately 60 lines.

## Proposed Model: Command Module Inventory

Thirteen files under `src-tauri/src/commands/`, each named for a domain a future change would target:

| File | Command count | Contents |
|---|---|---|
| `commands/panes.rs` | 11 | Terminal, agent-web, and workspace pane lifecycle |
| `commands/inference.rs` | 10 | Machine-spec probing, model downloads, server lifecycle |
| `commands/machine.rs` | 8 | Podman machine readiness, provisioning, provider-neutral setup steps |
| `commands/agents.rs` | 7 | Agent install, interactive auth sessions, OpenCode and pi config writes |
| `commands/sandbox.rs` | 6 | Create, list, start, stop, destroy, container event stream |
| `commands/deploy.rs` | 6 | Deploy, undeploy, deployment listing, sync status and toggles |
| `commands/image.rs` | 6 | Base-image build and readiness, dismissed-version markers, provider capabilities |
| `commands/credentials.rs` | 6 | Auth container, credential capture and injection, bundle listing |
| `commands/packages.rs` | 4 | mise package install, uninstall, list, registry search |
| `commands/hosts.rs` | 4 | Remote sandbox host registration and provisioning |
| `commands/app.rs` | 4 | Mutagen version probe, activity snapshot, settings read and write |
| `commands/network.rs` | 3 | Forward listing, port exposure and revocation |
| `commands/mockctl.rs` | 2 | `mock_configure` and `mock_reset` |

`src-tauri/src/commands/mod.rs` declares the thirteen submodules and owns the `tauri::generate_handler!` inventory, replacing the list currently at the bottom of `run()` in `src-tauri/src/lib.rs`.

## Proposed Model: The Sandbox Routing Seam

`sandbox_facts`, `resolve_machine`, and `ensure_forward_alias` in `src-tauri/src/lib.rs` are called as a fixed three-step sequence by five commands: `prepare_terminal_webview`, `prepare_agent_web_pane`, `open_workspace`, `expose_port`, and (in its two-step form) the deploy path. Two of the five sites additionally repeat the same `if exec_capable` guard around the alias step.

The proposal folds the three steps into `src-tauri/src/services/routing.rs`. A `SandboxRoute` value carries the four facts every transport-sensitive command branches on: the `ProviderId` running the sandbox, the `MachineRef` its operations address, whether the sandbox supports the exec transport, and the SSH alias its Mutagen forwards tunnel over. A `SandboxRouter` struct holds `Arc<dyn SandboxBackend>`, `Arc<dyn SyncEngine>`, and `Arc<ProviderRegistry>`, is registered once in `run()`, and exposes two operations: one that resolves a full route including the alias write, and one that resolves provider, machine, and exec capability without touching `~/.ssh`.

The alias-writing operation must keep the contract documented on `ensure_forward_alias` in `src-tauri/src/lib.rs`: the managed `~/.ssh` block is rewritten on every call because, unlike the container, the SSH config is not guaranteed to survive between app runs.

## Proposed Model: Pane Preparation as a Service

`prepare_terminal_webview` and `prepare_agent_web_pane` in `src-tauri/src/lib.rs` are the two largest non-command bodies. Each resolves a route, prepares a service inside the sandbox through `RuntimeManager`, opens a Mutagen forward through `NetworkManager`, writes a session descriptor into the workspace `.mio` directory, and builds a webview initialization script. `prepare_agent_web_pane` additionally polls the paseo `/api/status` endpoint through the fresh forward via `fetch_paseo_server_info`.

Both move to `src-tauri/src/services/panes.rs`, together with `read_paseo_password`. The four pane commands in `src-tauri/src/commands/panes.rs` then differ only in which pane they request and where they mount the result — a standalone `tauri::WebviewWindowBuilder` window for `open_terminal`, an embedded pane through `workspace::embed_terminal` or `workspace::embed_agent_web` for the rest.

## Proposed Model: Bootstrap and Logging Extraction

`src-tauri/src/bootstrap.rs` takes the body of `run()` as three functions: process-runner construction, state registration inside the Tauri `setup` closure, and the `tauri::RunEvent::Exit` handler that stops the active inference server and shuts the sync engine down.

Two ordering constraints in `run()` survive the extraction and are the main correctness risk of the phase. `process::clear_inheritable_handles()` must remain the first statement of `run()` — any process spawned before the call inherits the launching terminal's console handles (`docs/features/podman-process-isolation/02_dev.md`, Phase A) — so the call stays in `src-tauri/src/lib.rs` rather than moving into `bootstrap`. `backends::set_podman_runner` must still precede all Tauri state wiring, or the `PODMAN_RUNNER` `OnceLock` in `src-tauri/src/backends/mod.rs` panics on the first free-function podman call.

`init_logging` moves to `src-tauri/src/logging.rs` unchanged. The returned `tracing_appender::non_blocking::WorkerGuard` must still be handed to `manage` — its `Drop` is the only code path that drains the non-blocking writer's queue at process exit.

## Proposed Model: State Narrowing Rule

The governing rule the refactor establishes, to be recorded in `src-tauri/AGENTS.md`: a command names the narrowest state handle that can do its job. A command driving one subsystem keeps its single `Arc<dyn Trait>` parameter. A command spanning subsystems takes one service handle instead of four to six trait handles.

Applying the rule with `SandboxRouter` alone:

| Command | State parameters today | After |
|---|---|---|
| `open_agent_web` | 6 | 4 |
| `open_terminal_pane` | 6 | 4 |
| `open_terminal` | 5 | 3 |
| `open_workspace` | 4 | 2 |
| `expose_port` | 4 | 2 |

## Proposed Model: Repeated-Pattern Coverage

Six patterns repeat across `src-tauri/src/lib.rs` frequently enough to justify a named cover.

The activity-task lifecycle appears as 28 `activity::` calls across four commands. `start_podman_machine` and `provision_podman` in `src-tauri/src/lib.rs` between them contain six copies of the "format the error, call `activity::fail`, return `AppError::PodmanUnavailable`" sequence. A scope helper in `src-tauri/src/activity/mod.rs` that opens a task, hands a progress handle to a closure, and resolves the task to succeeded or failed from the closure's `Result` removes every copy.

The activity progress bridge in `start_inference` (`src-tauri/src/lib.rs`) is a 20-line `tauri::ipc::Channel` that mirrors engine setup events to the frontend while counting `stepDone` and `stepSkipped` into an activity progress fraction. The bridge is generic over any stepped command and belongs in `src-tauri/src/activity/mod.rs` beside the scope helper.

Workspace file access repeats 14 times as `BackendError::CommandFailed` wrapping a formatted path. `provision_opencode`, `provision_pi`, `dismiss_image_update`, `get_dismissed_image_version`, `read_paseo_password`, and both pane preparers each read-or-default a JSON document under the workspace `.mio` directory, mutate one key, and write it back. A helper module gives the read, merge, and write operations one error-formatting site.

Runtime error construction repeats eight times as `RuntimeError::TerminalUnavailable` or `RuntimeError::AgentWebUnavailable` wrapping a stringified error. `prepare_agent_web_pane` already invents a local closure for the pattern. Constructors on `RuntimeError` in `src-tauri/src/runtime/types.rs` replace both.

Provider lookup repeats five times as a `provider_of` call followed by tuple index `.1`, in `install_agent`, `start_agent_auth`, `capture_agent_credentials`, `inject_agent_credentials`, and `seed_agent_onboarding`. Either a named accessor on `ProviderRegistry` (`src-tauri/src/provider/mod.rs`) or pushing registry resolution down into `credentials/` and `agents/` removes the tuple indexing from the command layer.

Thin delegation — a command whose whole body forwards to one trait method — appears roughly 25 times. The proposal deliberately leaves the pattern uncovered; see the tradeoff below.

## Technology Choice: Where the Command Inventory Lives

**Chosen:** a generic function in `src-tauri/src/commands/mod.rs` returning the `tauri::generate_handler!` closure, called from `run()`.

`tauri::generate_handler!` expands to a closure implementing `Fn(tauri::ipc::Invoke<R>) -> bool`, which a function can return behind `impl Fn(…) -> bool + Send + Sync + 'static` given an explicit `R: tauri::Runtime` parameter. Keeping the inventory next to the command modules means the "command not found" gotcha in `AGENTS.md` and `src-tauri/AGENTS.md` points at the same directory an agent is already editing.

**Rejected — keep the 77-name list in `src-tauri/src/lib.rs` with `commands::…` paths.** Viable and zero-risk, but leaves an 80-line list in the file the refactor exists to shrink, and splits the "add a command" workflow across two directories. Retained as the fallback if the generic return type fights the macro; the fallback still reaches roughly 140 lines for `src-tauri/src/lib.rs` rather than 60.

**Rejected — a `macro_rules!` that expands to the handler list.** Adds a layer of indirection between a command's name and its registration, which is precisely the lookup an agent performs when debugging a missing command.

## Technology Choice: A `services/` Layer Versus the Alternatives

**Chosen:** a new crate-root `services/` directory for cross-subsystem orchestration.

**Rejected — push orchestration down into the subsystems.** Moving `resolve_machine` into `provider/` would require `provider/` to depend on `sync/` for the deploy-session lookup, and moving pane preparation into `runtime/` would require `runtime/` to depend on `network/`. Both violate the rule in `src-tauri/AGENTS.md` that subsystems never depend on each other, which is what keeps each subsystem independently mockable for the e2e `light` tier.

**Rejected — leave orchestration in the command bodies.** The status quo. Orchestration with no home is why `src-tauri/src/lib.rs` reached 1,805 lines; splitting commands across thirteen files without extracting orchestration would distribute the same 400 lines of composition across the thirteen instead of concentrating it.

**Rejected — name the directory `orchestration/` or `app/`.** Purely nominal; `services/` was chosen for brevity and because the layer's members are nouns (`SandboxRouter`) as often as verbs.

## Technology Choice: `SandboxRouter` as Managed State

**Chosen:** a `SandboxRouter` struct holding three `Arc`s, registered once in `run()` and requested as a single `State` parameter.

**Rejected — free functions in `services/routing.rs` taking the three `Arc`s as arguments.** Preserves the current call shape exactly and is the smaller diff, but leaves every pane and network command still declaring `Arc<dyn SandboxBackend>`, `Arc<dyn SyncEngine>`, and `Arc<ProviderRegistry>` in its signature. The state-scope goal is unmet.

**Rejected — a per-domain facade for every domain (`PaneService`, `DeployService`, `AgentService`).** Reaches the lowest state-parameter count, but produces facades whose only member is a passthrough for the domains that genuinely drive one subsystem, and risks a second god object. The proposal takes `SandboxRouter` now and leaves a `PaneService` folding `RuntimeManager` and `NetworkManager` alongside the router as a deferred option in `docs/features/lib-decomposition/04_improvements.md`.

**Note on host-global state:** the refactor introduces none. `SandboxRouter` composes existing subsystems and adds no new host-global dependency — no new podman connection, SSH alias, fixed host port, or Windows service — so every phase stays verifiable on the per-worktree `light` tier (`e2e/AGENTS.md`).

## Technology Choice: Leaving Thin Delegations Uncovered

**Chosen:** the roughly 25 commands whose body is a single forwarded trait call stay written out longhand.

**Rejected — a declarative macro generating command bodies from a trait method name.** Would remove roughly 150 lines, but a command's `#[tauri::command]` attribute, its doc comment, and its argument names are the frontend contract, and each is what an agent greps for when tracing an `invoke()` call from `src/`. Hiding all three behind a macro invocation makes the contract unsearchable. The per-domain file split already reduces the cost of the longhand form to scanning a 60-line file.

## Technology Choice: Freezing the IPC Surface

**Chosen:** no command name, argument name, argument type, or return type changes in any phase.

The alternative — folding renames into the refactor while the files are open — was rejected because a frozen IPC surface is what makes `./build e2e light` and `./build e2e` valid regression evidence for a pure-move change. Any rename would require simultaneous edits under `src/` and `e2e/`, and a failing spec would no longer distinguish a bad move from a missed rename. Renames belong in a follow-up recorded in `docs/features/lib-decomposition/04_improvements.md`.

## Technology Choice: The Activity Scope Helper Shape

**Chosen:** a closure-taking helper (open, run the closure with a progress handle, resolve from the returned `Result`).

**Rejected — an RAII guard whose `Drop` marks the task failed unless explicitly succeeded.** Reads more naturally at the call site, but a `Drop` implementation cannot see the error value, so a failed task would lose the message `activity::fail` currently records — the message the frontend Activity Center displays.

**Rejected — leaving the four call sites as they are.** The Podman startup path is the densest user and also the most delicate: `start_podman_machine` in `src-tauri/src/lib.rs` deliberately treats an "already running" non-zero exit as success rather than triggering a `wsl --shutdown`. Converting the path is worthwhile but is the one conversion that must be read against `docs/features/podman-startup-recovery/02_dev.md` first.

## Validation

Not yet validated. One risk warrants a spike before the bulk move: whether `tauri::generate_handler!` expands cleanly inside a generic function returning `impl Fn(tauri::ipc::Invoke<R>) -> bool`. The spike is three commands and a `cargo check`, and its outcome selects between the chosen inventory location and the stated fallback. Spike artefacts belong in `docs/features/lib-decomposition/spike/`.

The refactor's ongoing validation is the existing verification chain from `AGENTS.md`, run per phase: `cargo check` and `cargo test` in `src-tauri/`, then `./build e2e light` at the repository root. Because the IPC surface is frozen, a `light` failure indicates a bad move rather than an intended behaviour change. `./build e2e` (`standard`) is warranted after the state-narrowing phase and after the pattern-coverage phase, the only two phases that change how state or control flow reaches a handler.

## Open Questions

**Does `SandboxRouter` cache routes?** `sandbox_facts` in `src-tauri/src/lib.rs` performs a full `SandboxBackend::list` call per invocation, and `resolve_machine` performs a `SyncEngine::sessions` call, so a single `open_agent_web` costs two list calls before any work begins. Caching would cut the cost but introduces an invalidation problem on deploy, undeploy, and destroy. The proposal assumes no caching; `02_dev.md` should confirm or resolve otherwise.

**Do `undeploy_sandbox` and `destroy_sandbox` share one teardown function?** Both close panes, close forwards, flush the sync session, remove the remote container, and terminate the session, but in different orders and with different guards — `destroy_sandbox` additionally enforces the `AppError::UnsyncedChanges` check and handles the no-session case. Whether one parameterised `services/sandbox.rs` function serves both, or two functions sharing smaller steps, is undecided.

**Which module owns the workspace `.mio` file helpers?** Candidates are a new crate-root helper module, an addition to `src-tauri/src/backends/mod.rs` (which already owns `workspace_path` derivation), or `src-tauri/src/workspace/mod.rs` (which currently owns pane registry state, not filesystem access). The third option risks conflating two meanings of "workspace".

**Does `commands/agents.rs` keep `provision_opencode` and `provision_pi`, or do the config writers move wholesale into `agents/config.rs`?** Both functions are pure host-side filesystem merges with no Tauri state, so the subsystem could own the logic entirely with a two-line command remaining. The question is whether `agents/` — currently scoped to install and interactive auth — should widen to agent configuration files.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `invoke()` rejects a compiled command with "command not found" after the decomposition | The command function was added to a file under `src-tauri/src/commands/` but not to the `tauri::generate_handler!` inventory, which moves from `run()` in `src-tauri/src/lib.rs` to `src-tauri/src/commands/mod.rs` | Add the command path to `tauri::generate_handler!` in `src-tauri/src/commands/mod.rs` |
| `podman_global_runner` panics with "set_podman_runner must run before any free-function podman call" after bootstrap extraction | The `PODMAN_RUNNER` `OnceLock` in `src-tauri/src/backends/mod.rs` is unset because moving `run()`'s body into `src-tauri/src/bootstrap.rs` reordered `backends::set_podman_runner` after Tauri state wiring | Keep `backends::set_podman_runner` ahead of every `manage` call in `src-tauri/src/bootstrap.rs` |
| A child process inherits the launching terminal's console handles after bootstrap extraction | `process::clear_inheritable_handles()` was moved out of the first statement position of `run()` in `src-tauri/src/lib.rs`, and `listen_events` or the Mutagen daemon spawned before the call ran | Leave `process::clear_inheritable_handles()` as the first statement of `run()` in `src-tauri/src/lib.rs`; `src-tauri/src/bootstrap.rs` starts after the call (`docs/features/podman-process-isolation/02_dev.md`, Phase A) |
| `mio.log` is empty for a run after the logging extraction | The `tracing_appender::non_blocking::WorkerGuard` returned by `init_logging` in `src-tauri/src/logging.rs` was dropped or leaked instead of being passed to `manage`, so the non-blocking writer's queue never drained at exit | Pass the `WorkerGuard` to `manage` inside the Tauri `setup` closure, as `run()` in `src-tauri/src/lib.rs` does today |
| A Mutagen forward fails with an SSH transport error after the routing extraction | The alias-writing path of `SandboxRouter` was skipped for a call that previously went through `ensure_forward_alias`, and the managed `~/.ssh` block does not survive between app runs the way the container does | Call the route resolution that writes the alias, not the read-only variant, before opening any forward in `src-tauri/src/services/panes.rs` |
| A `light` e2e spec fails after a phase that only moved code | The move changed a command name, argument name, or return type, breaking the frozen IPC contract the `light` tier asserts against | Revert the signature change; renames are deferred to `docs/features/lib-decomposition/04_improvements.md` and require simultaneous edits under `src/` and `e2e/` |
