---
type: dev-plan
title: "lib.rs Decomposition — Build Plan"
feature: lib-decomposition
---
# lib.rs Decomposition — Build Plan

Build plan for the decomposition of `src-tauri/src/lib.rs` proposed in `docs/features/lib-decomposition/01_ideas.md`. The plan splits the 1,805-line file into a `commands/` layer of thirteen domain files, a `services/` layer owning cross-subsystem orchestration, and a `bootstrap.rs` holding startup wiring, then narrows which Tauri state each command can reach and covers six repeated patterns. Status: complete — Phases A through H are all implemented.

Every phase is behaviour-preserving. No command name, argument name, argument type, or return type changes, so `src/` and `e2e/` are untouched and the existing e2e tiers are valid regression evidence for each phase.

## The Concrete Model

Three layers, with a one-directional dependency rule.

`src-tauri/src/commands/` holds only `#[tauri::command]` functions across thirteen domain files. A command decodes arguments, requests Tauri state, delegates to one subsystem or one service, and converts the result to `AppError`. A command may depend on `services/` and on subsystem modules.

`src-tauri/src/services/` holds orchestration spanning more than one subsystem: sandbox routing (`services/routing.rs`), pane preparation (`services/panes.rs`), and sandbox create/teardown sequencing (`services/sandbox.rs`). The `services/` layer is the only layer permitted to compose subsystems. A service may depend on any subsystem module and must not depend on `commands/`.

Subsystem modules (`backends/`, `runtime/`, `sync/`, `network/`, `machine/`, `provider/`, `mockctl/`, `activity/`, `agents/`, `credentials/`, `inference/`, `settings/`, `ssh/`, `workspace/`, `process/`, `transport/`) keep their `types.rs` + `mod.rs` layout and their trait/real-impl/mock structure. No subsystem gains a dependency on a sibling subsystem — the rule in `src-tauri/AGENTS.md` that keeps each subsystem independently mockable for the e2e `light` tier.

`src-tauri/src/lib.rs` ends at approximately 60 lines: the `mod` declarations, `run()`, and the `devtools` feature gate.

## State Narrowing Rule

A command names the narrowest state handle that can do its job. A command driving one subsystem keeps its single `Arc<dyn Trait>` parameter. A command spanning subsystems takes one service handle rather than four to six trait handles. The rule is recorded in `src-tauri/AGENTS.md` in Phase H.

`src-tauri/src/lib.rs` today declares 87 `State<'_, …>` parameters across 77 commands and registers 14 managed state entries. Phase E applies the rule to the five densest commands using `SandboxRouter`, reducing their combined 25 state parameters to 15.

## Resolved Decisions

**Resolved Decision 1 — `SandboxRouter` does not cache routes.** Each resolution performs the same `SandboxBackend::list` and `SyncEngine::sessions` calls that `sandbox_facts` and `resolve_machine` in `src-tauri/src/lib.rs` perform today, because a cache would need invalidation on deploy, undeploy, destroy, and any external container change, and the container change has no hook. Caching is deferred to `docs/features/lib-decomposition/04_improvements.md`.

**Resolved Decision 2 — `undeploy_sandbox` and `destroy_sandbox` keep separate teardown functions sharing one smaller step.** `services/sandbox.rs` gains `close_panes_and_forwards`, holding the `workspace::close_app` then `NetworkManager::close_sandbox` pair both flows perform, plus separate `undeploy` and `destroy` functions. One parameterised function was rejected: the two flows order the sync flush, the remote container removal, and the session termination differently, and `destroy_sandbox` additionally owns the `AppError::UnsyncedChanges` guard and the no-session branch that terminates the microsandbox workspace session from `backends::msb_sync_session`.

**Resolved Decision 3 — the workspace `.mio` file helpers live in a new crate-root `src-tauri/src/mio_files.rs`.** The module is named for the `.mio` directory it owns rather than for the workspace, which avoids conflating filesystem access with the pane registry in `src-tauri/src/workspace/mod.rs`. A single crate-root file matches the existing convention set by `src-tauri/src/paths.rs` and `src-tauri/src/env_ext.rs`.

**Resolved Decision 4 — `provision_opencode` and `provision_pi` move wholesale into `src-tauri/src/agents/config.rs`.** Both are pure host-side filesystem merges with no Tauri state, so the `agents/` subsystem widens from install and interactive auth to agent configuration files, and `commands/agents.rs` keeps two delegating commands.

**Resolved Decision 5 — the `tauri::generate_handler!` inventory lives in `src-tauri/src/commands/mod.rs` behind a function pinned to `tauri::Wry`.** The Phase A spike found the generic `R: tauri::Runtime` form unusable: 30 of the 77 commands take a concrete `tauri::AppHandle` (that is, `AppHandle<Wry>`), which implements `CommandArg<'_, R>` only for `R = Wry`, so `generate_handler!` fails to expand under an arbitrary runtime. A three-command spike over state-free and runtime-agnostic commands passes and would have hidden the problem; the full inventory exposed it. Dropping the type parameter for `handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static` compiles and keeps the inventory next to the command modules, so the stated fallback of leaving the list in `run()` was not needed.

**Resolved Decision 6 — no phase changes the IPC surface.** Command renames and argument reshaping are deferred to `docs/features/lib-decomposition/04_improvements.md`, because a frozen IPC surface is what lets a `light` e2e failure identify a bad code move rather than a missed rename under `src/` or `e2e/`.

## Parallel-Worktree Constraints

The refactor introduces none of the host-global state that `e2e/AGENTS.md` and `docs/features/parallel-worktree-e2e/02_dev.md` constrain. Stated explicitly so a future phase does not add one unnoticed:

- **No new path.** `src-tauri/src/mio_files.rs` (Phase G) operates only on paths derived from the `workspace_path` argument a command already receives. `src-tauri/src/logging.rs` (Phase B) keeps `init_logging`'s existing resolution through `env_ext::env_dir("MIO_APP_DATA_DIR")` with the `app_log_dir()` fallback, unchanged.
- **No new host port.** `ephemeral_port` moves to `src-tauri/src/inference/mod.rs` in Phase B and keeps binding `127.0.0.1:0`; no constant is pinned.
- **No new host-global external resource.** `SandboxRouter` (Phase E) composes the existing podman connection, SSH alias, and Mutagen session handling from `src-tauri/src/backends/mod.rs` and `src-tauri/src/sync/mod.rs` without adding a name of its own. Every phase therefore stays verifiable on the per-worktree `light` tier.
- **No new bundled binary**, so `src-tauri/build.rs` needs no new asset group.
- **No new teardown of host-global state**, so no tier gate is needed in `e2e/wdio.conf.ts`.
- **No new environment variable**, so the Environment Variables section of the root `AGENTS.md` is unchanged.

## Implemented: Phase A — Handler Inventory Spike

`src-tauri/src/commands/mod.rs` holds `handler()`, returning `impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static`, and `run()` in `src-tauri/src/lib.rs` registers it in place of the inline `tauri::generate_handler!` list.

The phase landed the full 77-name inventory rather than the three-command spike list the plan called for. Two reasons. A three-command handler de-registers the other 74, so `./build e2e light` could not have run at all — the plan's stated verification was unachievable, and the Phase Independence rule below requires each phase to leave the crate shippable. The three chosen commands are also the ones that hide the failure: all three take runtime-agnostic arguments, so the generic form compiles for them and breaks on the first `tauri::AppHandle` command. Resolved Decision 5 records the resulting signature.

The names are still `crate::…` paths, because the command functions remain in `src-tauri/src/lib.rs` until Phase C repoints them at `commands::…`.

Verified: `cargo check` and `cargo test` in `src-tauri/`, `./build e2e light` at the repository root (12 spec files passing).

## Implemented: Phase B — Extract Standalone Non-Command Bodies

Verbatim moves of code that is not a command and has no cross-phase dependency. Comments move with their code; the ordering-constraint and invariant comments called out in the root `AGENTS.md` "Editing code that has comments" section must not be dropped or truncated.

New `src-tauri/src/services/mod.rs` declaring the `panes` and `routing` submodules.

New `src-tauri/src/services/routing.rs` receives `sandbox_facts`, `resolve_machine`, and `ensure_forward_alias` from `src-tauri/src/lib.rs`, still as free functions with their current signatures. The `SandboxRouter` struct arrives in Phase E.

New `src-tauri/src/services/panes.rs` receives `prepare_terminal_webview`, `prepare_agent_web_pane`, `read_paseo_password`, and `fetch_paseo_server_info` from `src-tauri/src/lib.rs`.

New `src-tauri/src/agents/config.rs` receives the bodies of `provision_opencode` and `provision_pi`; the two `#[tauri::command]` functions stay in `src-tauri/src/lib.rs` for this phase and delegate.

New `src-tauri/src/logging.rs` receives `init_logging` from `src-tauri/src/lib.rs`. The function keeps returning the `tracing_appender::non_blocking::WorkerGuard`, and `run()` keeps handing the guard to `manage`.

`ephemeral_port` moves from `src-tauri/src/lib.rs` to `src-tauri/src/inference/mod.rs`, whose `InferenceError::Sidecar` it already constructs.

The `.mio/dismissed_image_version` read and write bodies move from `get_dismissed_image_version` and `dismiss_image_update` in `src-tauri/src/lib.rs` to `src-tauri/src/backends/mod.rs`, beside the `BASE_IMAGE_VERSION` constant the markers gate on. The two commands stay in `src-tauri/src/lib.rs` and delegate.

`src-tauri/src/lib.rs` gains `mod logging;` and `mod services;` and ended the phase at 1,321 lines.

Two error-type notes the moves settled. `ephemeral_port` returns `InferenceError` rather than `AppError` now that it sits in a subsystem, and `start_inference` converts at the `?`. `dismiss_image_update` in `src-tauri/src/backends/mod.rs` likewise returns `BackendError`, and `agents/config.rs` keeps constructing `BackendError` for the same reason its commands do — Resolved Decision 6 freezes the IPC `kind` those errors serialise as. The resulting `agents/` reference to `backends::types::BackendError` is recorded in `docs/features/lib-decomposition/04_improvements.md`.

Verified: `cargo check` and `cargo test` in `src-tauri/`, `./build e2e light` at the repository root (12 spec files passing).

## Implemented: Phase C — Command Split and Inventory Move

Move all 77 `#[tauri::command]` functions from `src-tauri/src/lib.rs` into thirteen files under `src-tauri/src/commands/`, and move the `tauri::generate_handler!` inventory into `src-tauri/src/commands/mod.rs` per the Phase A outcome.

| File | Count | Commands |
|---|---|---|
| `commands/panes.rs` | 11 | `open_terminal`, `open_workspace`, `open_terminal_pane`, `open_agent_web`, `switch_app`, `switch_pane`, `hide_active_pane`, `set_panel_collapsed`, `close_workspace`, `close_pane`, `workspace_state` |
| `commands/inference.rs` | 10 | `detect_machine_specs`, `redetect_machine_specs`, `download_model`, `downloaded_models`, `runtime_setup_steps`, `start_inference`, `stop_inference`, `inference_status`, `install_foundry_local`, `test_model` |
| `commands/machine.rs` | 8 | `check_podman_ready`, `machine_needs_init`, `start_podman_machine`, `provision_podman`, `reset_stuck_podman_machine`, `machine_setup_steps`, `machine_readiness`, `prepare_machine` |
| `commands/agents.rs` | 7 | `install_agent`, `provision_opencode`, `provision_pi`, `start_agent_auth`, `send_agent_auth_input`, `cancel_agent_auth`, `get_agent_auth_status` |
| `commands/sandbox.rs` | 6 | `create_sandbox`, `list_sandboxes`, `start_sandbox`, `stop_sandbox`, `destroy_sandbox`, `listen_events` |
| `commands/deploy.rs` | 6 | `deploy_sandbox`, `undeploy_sandbox`, `sandbox_deployments`, `sync_status`, `enable_sync`, `disable_sync` |
| `commands/image.rs` | 6 | `base_image_version`, `build_base_image`, `base_image_ready`, `get_dismissed_image_version`, `dismiss_image_update`, `provider_capabilities` |
| `commands/credentials.rs` | 6 | `ensure_auth_container`, `capture_agent_credentials`, `inject_agent_credentials`, `seed_agent_onboarding`, `list_credential_bundles`, `delete_credential_bundle` |
| `commands/packages.rs` | 4 | `install_package`, `uninstall_package`, `list_packages`, `list_registry` |
| `commands/hosts.rs` | 4 | `list_sandbox_hosts`, `add_sandbox_host`, `prepare_sandbox_host`, `provision_sandbox_host` |
| `commands/app.rs` | 4 | `mutagen_version`, `list_tasks`, `get_settings`, `set_settings` |
| `commands/network.rs` | 3 | `list_open_forwards`, `expose_port`, `unexpose_port` |
| `commands/mockctl.rs` | 2 | `mock_configure`, `mock_reset` |

The `INFERENCE_FORWARD` constant moves from `src-tauri/src/lib.rs` to `src-tauri/src/inference/mod.rs`, since it names the Mutagen forward carrying the inference server and is read by both `start_inference` and `stop_inference`.

Two commands that appear misfiled by name are placed deliberately. `test_model` sits in `commands/inference.rs` because it reads `MIO_TEST_MODEL`, the inference e2e override documented in the root `AGENTS.md`. `provision_opencode` and `provision_pi` sit in `commands/agents.rs` per Resolved Decision 4.

After Phase C, adding a command to `tauri::generate_handler!` in `src-tauri/src/lib.rs` is no longer possible — the inventory is in `src-tauri/src/commands/mod.rs`. The gotcha rows in the root `AGENTS.md` and `src-tauri/AGENTS.md` are corrected in Phase H.

Every command function became `pub` in the move, so `tauri::generate_handler!` in `src-tauri/src/commands/mod.rs` can name it. `src-tauri/src/lib.rs` ended the phase at 163 lines.

Verified: `cargo check` and `cargo test` in `src-tauri/`, `./build e2e light` at the repository root (12 spec files passing). A "command not found" rejection in the `light` run means a command was moved but not listed in `src-tauri/src/commands/mod.rs`.

## Implemented: Phase D — Bootstrap Extraction

`src-tauri/src/bootstrap.rs` received the body of `run()` as three items: `build()` for process-runner construction (the `MockDirector`, the podman and msb `ProcessRunner` pair, the port transport, and the machine manager), `Bootstrap::register_state` called from the Tauri `setup` closure, and `on_run_event` holding the `tauri::RunEvent::Exit` handler that stops the active inference server and calls `SyncEngine::shutdown`.

`build()` returns a `Bootstrap` struct carrying the constructed runners, which `register_state` consumes. The three `manage` calls that sat on the builder itself (the machine manager, `MachineRunners`, and the `MockDirector`) moved into `register_state` and kept their position ahead of the rest, so `run()` holds no state wiring at all.

Two ordering constraints survive the extraction and carry the phase's correctness risk.

`process::clear_inheritable_handles()` stays the first statement of `run()` in `src-tauri/src/lib.rs` and does not move into `src-tauri/src/bootstrap.rs`. Any process spawned before the call inherits the launching terminal's console handles, and both `PodmanCliBackend::listen_events` and the Mutagen daemon spawn early (`docs/features/podman-process-isolation/02_dev.md`, Phase A).

`backends::set_podman_runner` stays ahead of every `manage` call. The `PODMAN_RUNNER` `OnceLock` in `src-tauri/src/backends/mod.rs` panics on the first free-function podman call if the setter has not run.

The `devtools` plugin block stays in `run()` in `src-tauri/src/lib.rs` behind its `#[cfg(all(debug_assertions, feature = "devtools"))]` gate, because the plugin claims the global tracing subscriber and its interaction with `init_logging` in `src-tauri/src/logging.rs` is the reason the `MIO_DEVTOOLS` opt-in exists.

`src-tauri/src/lib.rs` ended the phase at 67 lines: the `mod` declarations, `run()`, and the `devtools` gate.

Verified: `cargo check` and `cargo test` in `src-tauri/`, `./build e2e light` (12 spec files passing). The log check passed too — the run's `MIO_APP_DATA_DIR` holds a 127 KB `mio.log.<date>` written through to the run's end, so the `WorkerGuard` is still managed. Note the file carries `tracing_appender`'s daily date suffix rather than the bare `mio.log` name this document originally used.

## Implemented: Phase E — SandboxRouter and State Narrowing

`src-tauri/src/services/routing.rs` holds a `SandboxRoute` struct and a `SandboxRouter` struct owning `Arc<dyn SandboxBackend>`, `Arc<dyn SyncEngine>`, and `Arc<ProviderRegistry>`.

`SandboxRoute` carries three fields, not the four the plan listed. The `ProviderId` is `machine.provider`: `resolve_machine` passes the sandbox's provider through both of its branches, so a separate field would have been a second copy of the same value, and the compiler flagged it as never read. The remaining fields are the `MachineRef`, the exec-transport flag, and the alias name.

`SandboxRouter` exposes three operations rather than two, because folding the `if exec_capable` guard into the alias-writing resolution would have changed behaviour at one of its two call-site families. `resolve` always writes the alias, as the pane preparers have always done — a publish-relay Podman sandbox reaches its forwards over the shared connection alias, which must exist. `resolve_for_ports` writes it only for an exec-capable sandbox, which is what `open_workspace` and `expose_port` did inline; the guard is now stated once in the router instead of twice at the call sites. `locate` writes nothing to `~/.ssh`.

Naming the alias and writing it are separate steps now: `alias_name` derives the alias from the machine and the exec flag (it is deterministic), so every `SandboxRoute` carries a non-optional alias regardless of which operation produced it, while only two of the three touch `~/.ssh`. The two must agree on their provider branch — a comment on each says so.

The alias-writing operations keep the contract documented on the old `ensure_forward_alias`: the managed `~/.ssh` block is rewritten on every call because, unlike the container, the SSH config is not guaranteed to survive between app runs.

`src-tauri/src/bootstrap.rs` registers one `SandboxRouter` in the Tauri `setup` closure, after the `SandboxBackend`, `SyncEngine`, and `ProviderRegistry` registrations it borrows from.

Signatures narrowed in this phase:

| Command | File | State parameters before | After |
|---|---|---|---|
| `open_agent_web` | `commands/panes.rs` | 6 | 4 (`SandboxRouter`, `RuntimeManager`, `NetworkManager`, `WorkspaceRegistry`) |
| `open_terminal_pane` | `commands/panes.rs` | 6 | 4 (same set) |
| `open_terminal` | `commands/panes.rs` | 5 | 3 (`SandboxRouter`, `RuntimeManager`, `NetworkManager`) |
| `open_workspace` | `commands/panes.rs` | 4 | 2 (`SandboxRouter`, `NetworkManager`) |
| `expose_port` | `commands/network.rs` | 4 | 2 (`SandboxRouter`, `NetworkManager`) |

`prepare_terminal_webview` and `prepare_agent_web_pane` in `src-tauri/src/services/panes.rs` take a `SandboxRouter` reference in place of their `SandboxBackend`, `SyncEngine`, and `ProviderRegistry` references, dropping both from eight parameters to six and losing their `#[allow(clippy::too_many_arguments)]` attributes — `cargo clippy` confirms neither trips the lint now.

`src-tauri/src/bootstrap.rs` registers one `SandboxRouter` in `register_state`, reading the three handles back out of Tauri state immediately after registering them.

Verified: `cargo check`, `cargo clippy`, and `cargo test` in `src-tauri/`, then `./build e2e light` (12 spec files passing) and `./build e2e` (`standard`). The `standard` tier is warranted because Phase E is the first phase to change how state reaches a handler, and the `light` tier cannot prove the real SSH alias write.

## Implemented: Phase F — Sandbox Create and Teardown Service

`src-tauri/src/services/sandbox.rs` holds the orchestration that was inline in three commands, as `create`, `undeploy`, and `destroy`. `create_sandbox` keeps the `settings::resolve_default_provider` fallback in the command, since resolving a defaulted argument is argument decoding rather than orchestration; the service takes the provider already resolved and needs no `AppHandle`.

The create flow takes the body of `create_sandbox` from `src-tauri/src/commands/sandbox.rs`: the staged `tracing::info!` markers on the `create` target, the `ensure_base_image` then `create` then `RuntimeManager::init` sequence, and the microsandbox branch. The microsandbox branch keeps its full ordering and its failure handling — starting the instance before creating the Mutagen session because `msb ssh serve` must answer the alias `ProxyCommand`, logging the flattened Mutagen beta error, calling `ssh::probe_alias` diagnostically while the sandbox is still up, and rolling back with `SandboxBackend::destroy` so a retry under the same name does not hit `NameTaken` (`docs/features/microsandbox-provider/02_dev.md`, Phase H).

`close_panes_and_forwards` holds the `workspace::close_app` then `NetworkManager::close_sandbox` pair shared by the undeploy and destroy flows, per Resolved Decision 2.

The undeploy flow takes the body of `undeploy_sandbox` from `src-tauri/src/commands/deploy.rs`, preserving the load-bearing order recorded in its comment: close panes and forwards before the SSH channel to avoid orphans, flush pending remote writes, remove the remote container, then terminate the session, because terminating drops Mutagen's SSH connections and can briefly block a following container removal.

The destroy flow takes the body of `destroy_sandbox` from `src-tauri/src/commands/sandbox.rs`, preserving the `AppError::UnsyncedChanges` guard, the `force` bypass, and the no-session branch that best-effort terminates the microsandbox workspace session named by `backends::msb_sync_session`.

A comment on the destroy flow's `terminate`-before-`remove_remote` step now names Resolved Decision 2, so the difference from `undeploy`'s order reads as deliberate rather than as a bug to unify.

Verified: `cargo check` and `cargo test` in `src-tauri/`, then `./build e2e light` (12 spec files passing) and `./build e2e` (`standard`) — see the Standard Tier Baseline note below.

## Implemented: Phase G — Repeated-Pattern Coverage

Six patterns gain a named cover. The activity conversion is sequenced last within the phase because it changes control flow rather than only call shape.

`src-tauri/src/mio_files.rs` (Resolved Decision 3) provides read-or-default, merge, and write operations for JSON documents under a workspace `.mio` directory, formatting the `BackendError::CommandFailed` path message in one place. Call sites converted: `provision_opencode` and `provision_pi` in `src-tauri/src/agents/config.rs`, the dismissed-version helpers in `src-tauri/src/backends/mod.rs`, and `read_paseo_password` plus the `zellij.json` and `paseo.json` writes in `src-tauri/src/services/panes.rs`. The pattern appears 14 times today.

`src-tauri/src/runtime/types.rs` gained `RuntimeError::terminal_unavailable` and `RuntimeError::agent_web_unavailable`, taking any `impl Display`. Being plain functions rather than closures, they pass to `map_err` by name, so most call sites lost their closure entirely. The ad-hoc local `unavailable` closure in `prepare_agent_web_pane` is gone.

`ProviderRegistry::engine_of` in `src-tauri/src/provider/mod.rs` returns only the `Arc<dyn MachineProvider>` half of `provider_of`, removing the `.1` tuple index from five call sites in `src-tauri/src/commands/agents.rs` and `src-tauri/src/commands/credentials.rs`.

`activity::step_progress_channel` in `src-tauri/src/activity/mod.rs` mirrors engine setup events to a downstream `tauri::ipc::Channel` while counting `stepDone` and `stepSkipped` into a progress fraction, replacing the 20-line inline closure in `start_inference`.

`activity::scope` opens a task, hands a `TaskScope` handle to a closure, and resolves the task from the closure's returned `Result`. An RAII guard was rejected in `docs/features/lib-decomposition/01_ideas.md` because a `Drop` implementation cannot see the error value and would lose the message the Activity Center displays.

Preserving that message needed one addition the plan did not anticipate. The four call sites record a failure message that is not the propagated error's `Display`: `start_podman_machine` records `"Failed to start Podman machine: …"` while returning `AppError::PodmanUnavailable`, whose own `Display` prefixes `"Podman is not ready: "`. A scope resolving the task from `error.to_string()` would therefore have changed every Activity Center row. `activity::TaskFailure` carries the two separately — `TaskFailure::new(message, error)` where they differ, `TaskFailure::same(error)` where they do not. `TaskScope::set_success_detail` covers the mirror case, `start_inference` succeeding its task with the server's base URL.

Call sites converted: `download_model` and `start_inference` in `src-tauri/src/commands/inference.rs`, then `start_podman_machine` and `provision_podman` in `src-tauri/src/commands/machine.rs`.

The two Podman commands were converted last, against `docs/features/podman-startup-recovery/02_dev.md`. Both branches survive intact: `start_podman_machine` still treats an already-running machine's non-zero `podman machine start` exit as success rather than triggering a `wsl --shutdown`, and the retry branch still distinguishes other running machines from a WSL reset.

That document also records that a failing `start_podman_machine` has already called `activity::fail` and its caller must not call it again — which `provision_podman` satisfied by tail-calling the command and letting it resolve the shared `"podman-startup"` task. Two nested scopes would have resolved that one task twice. The shared steps moved instead into a private `start_machine` in `src-tauri/src/commands/machine.rs` that reports progress on an already-open task and resolves nothing, and both commands now open exactly one scope around it. `start_machine` is not a `#[tauri::command]`; it stays beside its two callers rather than moving to `services/`, matching `inference/mod.rs`, which already treats `activity` as a cross-cutting facility rather than a peer subsystem.

Verified: `cargo check`, `cargo clippy` (no new warnings — the eight reported are all pre-existing and in files this feature does not touch), and `cargo test` in `src-tauri/`, then `./build e2e light` (12 spec files passing) and `./build e2e` (`standard`) — see the Standard Tier Baseline note below.

## Implemented: Phase H — Documentation

The root `AGENTS.md` gotcha row for "New Tauri command exists in Rust but `invoke()` rejects with command not found" changes its cause and fix to name `tauri::generate_handler!` in `src-tauri/src/commands/mod.rs` instead of `src-tauri/src/lib.rs`.

`src-tauri/AGENTS.md` changes in four places. The orientation paragraph's reading order replaces `src-tauri/src/lib.rs` with `src-tauri/src/commands/mod.rs` for the command inventory and `src-tauri/src/bootstrap.rs` for state wiring. The Module Inventory section gains `commands/`, `services/`, `bootstrap.rs`, `logging.rs`, and `mio_files.rs`, and gains the state narrowing rule and the layer dependency rule. The "Adding a Tauri Command" numbered steps change: step 2 places the function in the matching `src-tauri/src/commands/` file, and step 3 registers it in `src-tauri/src/commands/mod.rs`. The `podman_global_runner` gotcha row's fix re-words "before any Tauri state wiring in `run()`" to name `src-tauri/src/bootstrap.rs`.

The approximately 50 references to `src-tauri/src/lib.rs` across `docs/features/*/02_dev.md` and `docs/features/*/01_ideas.md` are left unchanged. Those documents are dated records of decisions taken when the code lived in `src-tauri/src/lib.rs`, not live pointers, and rewriting them would destroy the record without adding accuracy.

Three stale `src-tauri/src/lib.rs` pointers surfaced outside the plan's enumerated edits, in orientation docs that are live pointers rather than dated records, and are corrected alongside the planned edits: the `MIO_APP_DATA_DIR` row and `src/AGENTS.md`'s opening paragraph in the root tree, and the machine-lifecycle-gap paragraph in `e2e/AGENTS.md`. A fourth, the `mockctl` row's `lib.rs` mention in `src-tauri/AGENTS.md`'s Module Inventory, is corrected for the same reason though it sits inside one of the plan's four named places.

Verified: `cargo check` and `cargo test` in `src-tauri/` (20 unit tests passing), `./build e2e light` (12 spec files passing), then `./build e2e` (`standard`) — 13 of 15 spec files passing, `package-autocomplete.spec.ts` and one other failing on the pre-existing machine-level flakiness described in `docs/features/lib-decomposition/04_improvements.md`, "Standard Specs Fail Intermittently on This Machine, Not by the Refactor" — unrelated to Phase H's docs-only diff.

## Cross-Cutting: Comment Preservation

The refactor moves several comments that state ordering constraints and hard-won invariants. The root `AGENTS.md` rule applies in full: a comment stating an ordering constraint or invariant is never deleted unless the constraint itself is removed, and is compressed rather than dropped if it is too long. The load-bearing comments the phases carry are the teardown ordering in `undeploy_sandbox` (Phase F), the microsandbox start-before-sync rationale in `create_sandbox` (Phase F), the alias-rewrite-every-call contract on `ensure_forward_alias` (Phases B and E), the already-running exit-code rationale in `start_podman_machine` (Phase G), the `WorkerGuard` drop contract on `init_logging` (Phase B), the `clear_inheritable_handles` first-statement requirement in `run()` (Phase D), and the reverse-forward comment in `start_inference` explaining that the inference server is reached from inside the Podman VM regardless of provider (Phase C).

## Cross-Cutting: Phase Independence

Each phase compiles, passes `cargo test`, and passes `./build e2e light` on its own, so the work can stop at any phase boundary and leave the crate in a shippable state. Phases A through D are pure moves. Phases E through G change internal call shapes and, in Phase G, control flow. The `standard` tier is run at the end of Phases E, F, G, and H, and skipped for A through D, where the `light` tier's proof that every command still dispatches is sufficient for a move with no signature change.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `invoke()` rejects a compiled command with "command not found" after Phase C | The command function exists in a file under `src-tauri/src/commands/` but is absent from the `tauri::generate_handler!` inventory, which Phase C moves out of `run()` in `src-tauri/src/lib.rs` | Add the command path to `tauri::generate_handler!` in `src-tauri/src/commands/mod.rs` |
| `cargo check` fails in Phase A with a type error on the `handler` function's return type | `tauri::generate_handler!` expands to a closure over `tauri::ipc::Invoke<R>` and needs an explicit `R: tauri::Runtime` parameter on the enclosing function | Add the `R: tauri::Runtime` parameter, or take the Resolved Decision 5 fallback and keep the inventory in `run()` in `src-tauri/src/lib.rs` with `commands::…` paths |
| `podman_global_runner` panics with "set_podman_runner must run before any free-function podman call" after Phase D | Moving `run()`'s body into `src-tauri/src/bootstrap.rs` reordered `backends::set_podman_runner` after a `manage` call, leaving the `PODMAN_RUNNER` `OnceLock` in `src-tauri/src/backends/mod.rs` unset | Keep `backends::set_podman_runner` ahead of every `manage` call in `src-tauri/src/bootstrap.rs` |
| A spawned child process holds the launching terminal's console handles after Phase D | `process::clear_inheritable_handles()` was moved out of the first statement position of `run()`, and `PodmanCliBackend::listen_events` or the Mutagen daemon spawned before the call ran | Leave `process::clear_inheritable_handles()` as the first statement of `run()` in `src-tauri/src/lib.rs` (`docs/features/podman-process-isolation/02_dev.md`, Phase A) |
| `mio.log` is 0 bytes for a run after Phase B or Phase D | The `tracing_appender::non_blocking::WorkerGuard` returned by `init_logging` in `src-tauri/src/logging.rs` was dropped instead of passed to `manage`, so the non-blocking writer never drained at exit | Pass the `WorkerGuard` to `manage` inside the Tauri `setup` closure in `src-tauri/src/bootstrap.rs` |
| A Mutagen forward fails with an SSH transport error after Phase E | A call site was pointed at `SandboxRouter`'s read-only resolution, which does not rewrite the managed `~/.ssh` block, and the SSH config does not survive between app runs the way the container does | Call the alias-writing resolution before opening any forward in `src-tauri/src/services/panes.rs` and `src-tauri/src/commands/network.rs` |
| `provision_podman` triggers an unexpected `wsl --shutdown` after Phase G | The activity scope-helper conversion collapsed the branch in `start_podman_machine` that treats an already-running machine's non-zero `podman machine start` exit as success | Restore the machine-is-running check ahead of the retry branch; see `docs/features/podman-startup-recovery/02_dev.md` |
| A `light` e2e spec fails after a phase that only moved code | The move altered a command name, argument name, or return type, breaking the IPC contract frozen by Resolved Decision 6 | Revert the signature change; renames are deferred to `docs/features/lib-decomposition/04_improvements.md` and require simultaneous edits under `src/` and `e2e/` |
