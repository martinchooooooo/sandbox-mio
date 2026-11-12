# Rust Backend Orientation

The `src-tauri/src/` crate is the Tauri backend: every capability the frontend uses is a `#[tauri::command]` under `src-tauri/src/commands/` delegating to a service or a subsystem module. Read `src-tauri/src/backends/types.rs` first (core data shapes and the `SandboxBackend` trait), then `src-tauri/src/commands/mod.rs` (the command inventory) and `src-tauri/src/bootstrap.rs` (state wiring), then only the subsystem module the task touches. How the subsystems behave together at runtime is described, at a high level, in `docs/architecture/` (start with `docs/architecture/overview.md`); this file covers code structure, conventions and implementation-level gotchas.

## Three Layers

`commands/` holds only `#[tauri::command]` functions, one file per domain. A command decodes its arguments, requests Tauri state, delegates, and converts the result into `AppError`.

`services/` holds orchestration spanning more than one subsystem, and is the only layer permitted to compose subsystems. A service may depend on any subsystem and must not depend on `commands/`.

Subsystem modules never depend on each other — that independence is what keeps each one mockable on its own for the e2e `light` tier. A command needing more than two subsystems is the signal that a service is missing.

**State narrowing rule:** a command names the narrowest state handle that can do its job. One driving a single subsystem keeps its single `Arc<dyn Trait>` parameter; one spanning subsystems takes one service handle rather than four to six trait handles. `SandboxRouter` (`services/routing.rs`) is the worked example — it stands in for `SandboxBackend` + `SyncEngine` + `ProviderRegistry` across the pane and port commands. See `docs/features/lib-decomposition/02_dev.md`.

## Subsystem Pattern: Trait + Real Impl + Mock

Every subsystem that touches external processes is a trait injected into Tauri state as `Arc<dyn Trait>`, constructed once at startup by a `default_*()` function. Under `MIO_MOCK_BACKEND=1` (checked by `mock_backend_enabled()` in `src-tauri/src/backends/mod.rs`) a subsystem takes one of two paths: the older ones swap in a hand-written in-memory mock struct, while `backends`/`machine`/`sync` keep their real struct and swap the `ProcessRunner` beneath it (see `mockctl/` below):

| Trait | Declared in | Real impl (drives) | Under `MIO_MOCK_BACKEND=1` | Constructor |
|---|---|---|---|---|
| `SandboxBackend` | `backends/types.rs` | `CliBackend` (podman CLI over a named connection, dispatching instance lifecycle through the provider registry) | same struct, `ScriptedPodmanRunner` | `default_backend()` in `backends/mod.rs` |
| `RuntimeManager` | `runtime/types.rs` | `PodmanMiseRuntime` (mise + zellij inside the container) | `runtime/mock.rs` | `default_runtime()` in `runtime/mod.rs` |
| `SyncEngine` | `sync/types.rs` | `MutagenCli` (bundled Mutagen sidecar) | same struct, `ScriptedMutagenRunner` | `default_sync_engine()` in `sync/mod.rs` |
| `MachineManager` | `machine/types.rs` | podman machine / WSL lifecycle | same struct, `ScriptedPodmanRunner` | `default_machine_manager()` in `machine/mod.rs` |
| `NetworkManager` | `network/types.rs` | `MutagenNetworkManager` (forward lifecycle, port exposure) | `network/mock.rs` | `default_network_manager()` in `network/mod.rs` |
| `AgentHost` | `agent_host/types.rs` | `ExecAgentHost` (pi supervisor scripts and attach streams over the engine's streamed exec) | `agent_host/mock.rs` (replays recorded pi traces) | `default_agent_host()` in `agent_host/mod.rs` |

Cross-subsystem orchestration lives in `services/` (e.g. the deploy teardown ordering in `services/sandbox.rs` drives both `SandboxBackend` and `SyncEngine`) so subsystems never depend on each other. When adding a new external-process capability, route it through one of these traits rather than shelling out from a command — otherwise the e2e `light` tier (see `e2e/AGENTS.md`) breaks for every flow touching the new capability. Prefer the runner path over a new mock struct: a hand-written mock reimplements the logic it stands in for and drifts from it (`docs/features/scriptable-mock-backend/02_dev.md`, "Superseded").

## Module Inventory

Modules follow one layout: `types.rs` declares the public types/trait, `mod.rs` implements.

- `backends/` — sandbox (container) lifecycle via podman; owns the base image (`BASE_IMAGE_VERSION`, embedded Containerfile from `src-tauri/resources/Containerfile`) and workspace-path derivation.
- `runtime/` — in-container tooling: mise package install/uninstall, zellij web terminal preparation, agent web UI (paseo).
- `sync/` — Mutagen file-sync and low-level port-forwarding primitives (`forward_create`/`forward_terminate`/`sweep_stale_forwards`) for remote deploys and embedded panes.
- `network/` — owns forward lifecycle/registry on top of `sync`'s primitives: terminal/agent-web forwards, and exposing an arbitrary user-chosen sandbox port to the host over the sandbox's SSH alias, without recreating the container.
- `machine/` — podman machine readiness/startup on the host (WSL on Windows).
- `process/` — the production process layer: the `ProcessRunner` trait, the real
  `SystemPodmanRunner`/`SystemMsbRunner`/`SystemMutagenRunner`, the detached-command constructors
  every external binary is spawned through, the startup handle sanitiser, the per-call-shape
  timeouts, and the podman half-open breaker. See
  `docs/features/podman-process-isolation/02_dev.md`.
- `mockctl/` — runtime control plane for the e2e `light` tier: the scripted half of each
  `ProcessRunner` pair (`ScriptedPodmanRunner`, `ScriptedMsbRunner`, `ScriptedMutagenRunner`),
  `MockDirector` (override registry, keyed `<binary>.<subcommand>`), the `Behaviour` enum, and the
  `KNOWN_CALLS` validator table. Exposed via the `mock_configure`/`mock_reset` commands in `commands/mockctl.rs`.
  `CliBackend`/`PodmanMachineManager` share one podman runner and `MutagenCli` holds a mutagen
  one; all three run unmodified against them, so there is no separate mock struct for
  `backends`/`machine`/`sync`. Depends on `process/`, never the reverse. See
  `docs/features/scriptable-mock-backend/02_dev.md`.
- `provider/` — the `MachineProvider` seam and `ProviderRegistry`: `PodmanProvider` (`provider/mod.rs`), `MsbProvider` (`provider/msb.rs`), and `msb_policy.rs`, which renders an egress policy into `msb create` flags. See `docs/architecture/providers.md`.
- `proxy/` — per-sandbox mitmproxy sidecar (free functions, not a trait subsystem: every external call already crosses the `ProcessRunner` seam through a provider), secret-injection rules, and the flow-log poller. See `docs/architecture/network.md`.
- `net_policy.rs` — the `.mio/net-policy.json` schema, validation, and per-sandbox applied snapshot; a plain file module like `mio_files.rs`, usable from any layer.
- `activity/` — `TaskRegistry`, the source of truth for long-running task state; pushes `activity://task-updated` events consumed by `src/lib/activity/activity.svelte.ts`.
- `agent_host/` — the native agent chat's host side: installs and drives the in-sandbox pi supervisor (`resources/agent-host/*.sh`), holds one attach stream per sandbox, frames the journal into offset-tagged records, derives pending dialogs, and lists chat session files host-side. See `docs/features/native-agent-ui/02_dev.md`.
- `agents/` — AI agent install/provisioning and interactive auth sessions (`AuthSessionMap`).
- `credentials/` — shared agent credential bundles (capture from one sandbox, inject into another).
- `inference/` — local LLM serving: machine-spec probing, model downloads, llama.cpp (`inference/llama.rs`) and Foundry Local (`inference/foundry.rs`) engines.
- `settings/` — persisted `AppSettings`.
- `ssh/` — SSH config manipulation for remote hosts.
- `workspace/` — embedded-pane registry (`WorkspaceRegistry`): which Terminal/Dashboard/agent-web panes are open per sandbox; emits `workspace://state`.
- `commands/` — the `#[tauri::command]` surface, one file per domain (`panes`, `agent_chat`, `inference`, `machine`, `agents`, `sandbox`, `deploy`, `image`, `credentials`, `packages`, `hosts`, `app`, `network`, `proxy`, `net_policy`, `mockctl`), plus the `tauri::generate_handler!` inventory in `commands/mod.rs`.
- `services/` — cross-subsystem orchestration: `routing.rs` (`SandboxRouter`, resolving a sandbox to its provider, machine, and SSH alias), `panes.rs` (terminal and agent-web pane preparation), `sandbox.rs` (create, network re-apply, and teardown sequencing), `start_env.rs` (`AppStartEnv`, the per-sandbox environment microsandbox start calls receive, including secret values).
- `bootstrap.rs` — startup wiring: process-runner construction, every `manage` call, and the exit handler.
- `logging.rs` — `init_logging`. Returns a `WorkerGuard` whose `Drop` is the only thing that drains the log file's non-blocking writer, so it must be handed to `manage`.
- `mio_files.rs` — reads and writes for the JSON documents under a workspace's `.mio` directory, formatting the path into the failure message in one place.
- `error.rs` — `AppError`, the IPC boundary type; see the Error Handling section of the root `AGENTS.md`.

## Adding a Tauri Command

1. Implement the capability on the owning subsystem trait (all impls including the mock must compile).
2. Add the `pub` `#[tauri::command]` fn to the matching domain file under `src-tauri/src/commands/`, returning `Result<T, AppError>` — never `Result<T, String>`. If it needs more than two subsystems, put the orchestration in `src-tauri/src/services/` and give the command one service handle.
3. Register its `<domain>::<name>` path in `tauri::generate_handler![]` in `src-tauri/src/commands/mod.rs`.
4. New long-running work reports progress through `activity::TaskRegistry`, not ad-hoc events.
5. Verify with `cargo check` in `src-tauri/`, then exercise via `./build e2e light` (repo root) if the mock covers the flow.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `invoke()` rejects with "command not found" for a compiled command | Command fn missing from `generate_handler![]` in `src-tauri/src/commands/mod.rs` | Add the `<domain>::<name>` path to `generate_handler![]` in `src-tauri/src/commands/mod.rs` |
| e2e `light` tier hangs or errors on a new command | New command shells out directly instead of going through a mocked trait, so `MIO_MOCK_BACKEND=1` has no effect on the command | Route the external call through one of the subsystem traits and implement the mock method |
| `podman -v` rejects the workspace mount path | Podman does not expand `~` or `$HOME`, and rejects backslash Windows paths | Use `WORKSPACE_MOUNT` (`/root/work`) and `workspace_path()` in `src-tauri/src/backends/mod.rs`, which forward-slashes the host path |
| Container base-image change silently ignored | `ensure_base_image` short-circuits on `podman image exists <tag>` | Bump `BASE_IMAGE_VERSION` in `src-tauri/src/backends/mod.rs` together with any `src-tauri/resources/Containerfile` change |
| Sandbox name accepted by UI but rejected by backend | `validate_name()` in `src-tauri/src/backends/mod.rs` bars non-ASCII-alphanumeric leading chars and path-unsafe chars because the name becomes a workspace path segment | Surface the `InvalidName` error via `parseError` on the frontend; do not loosen `validate_name()` without securing `workspace_path()` |
| `check_podman_ready` / `start_podman_machine` behave unexpectedly under `MIO_MOCK_BACKEND=1` | Machine-lifecycle commands use `Arc<dyn MachineManager>` (`PodmanMachineManager`), routed through the same shared `ProcessRunner` as `backends` | Script `podman.machine-list`/`podman.machine-start` via `mock_configure`, or rely on `ScriptedPodmanRunner`'s built-in "already running" default (`src-tauri/src/mockctl/mod.rs`) |
| `mock_configure` rejects a key that looks like a real `podman` call | The key is missing from `KNOWN_CALLS` in `src-tauri/src/mockctl/mod.rs` — only listed keys are scriptable | Add the key to `KNOWN_CALLS`; verify the derived key matches the matching strategy in `docs/features/scriptable-mock-backend/02_dev.md` |
| A `light` run's activity scrollback shows container events for sandboxes no spec created, or a leftover `podman` process holds `target/debug` files locked | A subsystem method spawns a streamed child process, which `ProcessRunner::run` (`src-tauri/src/process/types.rs`) cannot represent because it models one-shot calls only, and no `mock_backend_enabled()` gate covers the spawn | Gate the spawning method on `mock_backend_enabled()`, as `CliBackend::listen_events` (`src-tauri/src/backends/mod.rs`) already does; clear a leftover process with `killStaleDevProcesses` in `e2e/wdio.conf.ts` |
| A `light` spec restarts or reconfigures the developer's real Podman machine | A `podman` call that spawns a child rather than issuing a one-shot call bypasses the scripted `ProcessRunner` seam, so `MIO_MOCK_BACKEND=1` does not divert it | Gate the spawning method on `mock_backend_enabled()`, as `PodmanMachineManager::activate_podman_socket` and `run_host_wsl()` (`src-tauri/src/machine/mod.rs`) already do |
| `provision_podman` fails with `PodmanRebootRequired` on a `light` run on a host without WSL installed | The mock path of `PodmanMachineManager::install_wsl` (`src-tauri/src/machine/mod.rs`) returns `Ok(true)`, which means reboot-required per the `MachineManager` contract in `src-tauri/src/machine/types.rs` | Return `Ok(false)` from the mock path of `install_wsl`; `wsl_is_ready` is deliberately ungated as a read-only probe, so the mock path is reachable on any host without WSL |
| A panicking `wsl` blocking task reports success instead of an error | `run_host_wsl` (`src-tauri/src/machine/mod.rs`) maps a `tokio::task::JoinError` to the same `None` that signals the `MIO_MOCK_BACKEND=1` skip | Keep `None` reserved for the mock skip: `run_host_wsl` wraps a `JoinError` into an `std::io::Error` and returns `Some(Err(..))`, which callers map to `BackendError::CommandFailed` |
| `MockRuntime::list` returns no packages for a sandbox whose packages installed successfully | `workspace_name()` in `src-tauri/src/runtime/mock.rs` keys `list()` off the workspace path's final segment, which differs from the sandbox name when the sandbox was created from an `existing_path` argument to `resolve_create_workspace` (`src-tauri/src/backends/mod.rs`) | Assert light-tier package state only on sandboxes created without `existing_path`, as the `MockRuntime` doc comment requires; keying the map by sandbox name on both sides needs `RuntimeManager::list` (`src-tauri/src/runtime/types.rs`) widened, since it receives no name |
| A newly added spawn site hangs forever on Windows when the app was launched from a terminal | The command was built with a raw `Command::new` instead of the detached constructor, so it inherits the launching console's handles | Build every external command through `detached_command`/`detached_command_std` in `src-tauri/src/process/mod.rs`; never call `Command::new` for an external binary elsewhere |
| A dev or e2e build cannot find `llama-server`, or `msb` fails its minimum-version check at startup | Everything under `src-tauri/binaries/` (Podman client, Mutagen, `msb`, llama.cpp) is gitignored and staged by the root `build.rs` at pinned versions, so a fresh or stale checkout lacks it or holds an old release | Run `./build dev --setup` (or `./build dev --setup-worktree <path>` in a new worktree); keep `MSB_VERSION` in `build.rs` at or above `MIN_MSB_VERSION` in `src-tauri/src/machine/msb.rs` |
| The app window goes white and "(Not responding)" during a model download or another frequent-progress task, with low CPU and no error in `mio.log` | tauri's `tracing` feature makes `emit` block on the main thread while holding tauri's JS-listener lock, so a background emit racing any main-thread emit (tauri's own focus/resize events, `emit_state` in `src-tauri/src/workspace/mod.rs`) deadlocks; `tauri-plugin-devtools` turns that feature on | Keep the `devtools` feature out of `default` in `src-tauri/Cargo.toml` and enable it only with `MIO_DEVTOOLS=1`; never add a dependency that enables `tauri/tracing` (check with `cargo tree -e features -i tauri`) |
| A `BackendError` variant reaches the frontend as `kind: "Backend"` despite having its own `AppError` variant | `From<BackendError> for AppError` (`src-tauri/src/error.rs`) is hand-written and maps only the variants listed in it | Add the variant's arm to that `From` impl as well as to `kind()`, and mirror the discriminant into `AppErrorKind` (`src/lib/errors.ts`) |
