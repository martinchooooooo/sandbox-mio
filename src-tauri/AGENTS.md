# Rust Backend Orientation

The `src-tauri/src/` crate is the Tauri backend: every capability the frontend uses is a `#[tauri::command]` in `src-tauri/src/lib.rs` delegating to a subsystem module. Read `src-tauri/src/backends/types.rs` first (core data shapes and the `SandboxBackend` trait), then `src-tauri/src/lib.rs` (command inventory and state wiring), then only the subsystem module the task touches.

## Subsystem Pattern: Trait + Real Impl + Mock

Every subsystem that touches external processes is a trait injected into Tauri state as `Arc<dyn Trait>`, constructed once at startup by a `default_*()` function that returns the in-memory mock when `MIO_MOCK_BACKEND=1` (checked by `mock_backend_enabled()` in `src-tauri/src/backends/mod.rs`):

| Trait | Declared in | Real impl (drives) | Mock | Constructor |
|---|---|---|---|---|
| `SandboxBackend` | `backends/types.rs` | `PodmanCliBackend` (podman CLI over a named connection) | `backends/mock.rs` | `default_backend()` in `backends/mod.rs` |
| `RuntimeManager` | `runtime/types.rs` | `PodmanMiseRuntime` (mise + zellij inside the container) | `runtime/mock.rs` | `default_runtime()` in `runtime/mod.rs` |
| `SyncEngine` | `sync/types.rs` | `MutagenCli` (bundled Mutagen sidecar) | `sync/mock.rs` | `default_sync_engine()` in `sync/mod.rs` |
| `MachineManager` | `machine/types.rs` | podman machine / WSL lifecycle | `machine/mock.rs` | `default_machine_manager()` in `machine/mod.rs` |

Command handlers in `src-tauri/src/lib.rs` orchestrate across subsystems (e.g. deploy commands hold both `SandboxBackend` and `SyncEngine` state) so subsystems never depend on each other. When adding a new external-process capability, follow the trait + mock pattern rather than shelling out from `lib.rs` — otherwise the e2e `light` tier (see `e2e/AGENTS.md`) breaks for every flow touching the new capability.

## Module Inventory

Modules follow one layout: `types.rs` declares the public types/trait, `mod.rs` implements.

- `backends/` — sandbox (container) lifecycle via podman; owns the base image (`BASE_IMAGE_VERSION`, embedded Containerfile from `src-tauri/resources/Containerfile`) and workspace-path derivation.
- `runtime/` — in-container tooling: mise package install/uninstall, zellij web terminal preparation, agent web UI (paseo).
- `sync/` — Mutagen file-sync and port-forwarding sessions for remote deploys and embedded panes.
- `machine/` — podman machine readiness/startup on the host (WSL on Windows).
- `activity/` — `TaskRegistry`, the source of truth for long-running task state; pushes `activity://task-updated` events consumed by `src/lib/activity/activity.svelte.ts`.
- `agents/` — AI agent install/provisioning and interactive auth sessions (`AuthSessionMap`).
- `credentials/` — shared agent credential bundles (capture from one sandbox, inject into another).
- `inference/` — local LLM serving: machine-spec probing, model downloads, llama.cpp (`inference/llama.rs`) and Foundry Local (`inference/foundry.rs`) engines.
- `settings/` — persisted `AppSettings`.
- `ssh/` — SSH config manipulation for remote hosts.
- `workspace/` — embedded-pane registry (`WorkspaceRegistry`): which Terminal/Dashboard/agent-web panes are open per sandbox; emits `workspace://state`. Contains the crate's only unit tests.
- `error.rs` — `AppError`, the IPC boundary type; see the Error Handling section of the root `AGENTS.md`.

## Adding a Tauri Command

1. Implement the capability on the owning subsystem trait (all impls including the mock must compile).
2. Add the `#[tauri::command]` fn in `src-tauri/src/lib.rs`, returning `Result<T, AppError>` — never `Result<T, String>`.
3. Register the fn in `generate_handler![]` in `src-tauri/src/lib.rs`.
4. New long-running work reports progress through `activity::TaskRegistry`, not ad-hoc events.
5. Verify with `cargo check` in `src-tauri/`, then exercise via `pnpm run test:light` in `e2e/` if the mock covers the flow.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `invoke()` rejects with "command not found" for a compiled command | Command fn missing from `generate_handler![]` in `src-tauri/src/lib.rs` | Add the fn name to `generate_handler![]` in `src-tauri/src/lib.rs` |
| e2e `light` tier hangs or errors on a new command | New command shells out directly instead of going through a mocked trait, so `MIO_MOCK_BACKEND=1` has no effect on the command | Route the external call through one of the four subsystem traits and implement the mock method |
| `podman -v` rejects the workspace mount path | Podman does not expand `~` or `$HOME`, and rejects backslash Windows paths | Use `WORKSPACE_MOUNT` (`/root/work`) and `workspace_path()` in `src-tauri/src/backends/mod.rs`, which forward-slashes the host path |
| Container base-image change silently ignored | `ensure_base_image` short-circuits on `podman image exists <tag>` | Bump `BASE_IMAGE_VERSION` in `src-tauri/src/backends/mod.rs` together with any `src-tauri/resources/Containerfile` change |
| Sandbox name accepted by UI but rejected by backend | `validate_name()` in `src-tauri/src/backends/mod.rs` bars non-ASCII-alphanumeric leading chars and path-unsafe chars because the name becomes a workspace path segment | Surface the `InvalidName` error via `parseError` on the frontend; do not loosen `validate_name()` without securing `workspace_path()` |
| `check_podman_ready` / `start_podman_machine` do not hit the mock backend | Machine-lifecycle commands use `Arc<dyn MachineManager>`, not `SandboxBackend` | Extend `MockMachineManager` in `src-tauri/src/machine/mock.rs` for the mocked behaviour |
