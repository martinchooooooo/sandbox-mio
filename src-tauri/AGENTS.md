# Rust Project Orientation

This document tells an AI agent which files to read first to understand the `src-tauri` Rust codebase. All source files live under `src-tauri/src/`.

## Recommended Reading Order

Read files in the order below. Each file builds on the previous, so reading out of order requires backtracking.

### 1. `src-tauri/src/backends/types.rs` — Public API surface and internal contracts

Start here. `src-tauri/src/backends/types.rs` declares everything other files depend on: the `Sandbox` struct (the data shape returned to the frontend, including the host `workspace_path` bind-mounted into the container at `/root/work`), the `SandboxBackend` trait (the interface all backend implementations must satisfy), and the `PodmanCliBackend` struct (the sole implementation). The backend drives the `podman` CLI against a named **system connection**; the connection is selected by the `PODMAN_CONNECTION` environment variable (default `podman-machine-default`). The same code path serves the local machine and remote hosts — both are just connections, and `podman` performs the SSH transport. Reading this file first means all subsequent method signatures and the connection model are already familiar.

### 2. `src-tauri/src/lib.rs` — Tauri command handlers and backend wiring

Read second. `src-tauri/src/lib.rs` is the crate entry point. `src-tauri/src/lib.rs` registers all Tauri commands and wires the chosen `SandboxBackend` implementation into Tauri state as `Arc<dyn SandboxBackend>`, plus the `SyncEngine` (`Arc<dyn SyncEngine>`, see `src-tauri/src/sync/`) as separate state. Command groups: sandbox lifecycle (`create_sandbox`, `list_sandboxes`, `start_sandbox`, `stop_sandbox`, `destroy_sandbox`); host/connection management (`list_sandbox_hosts`, `add_sandbox_host`, `prepare_sandbox_host`, `provision_sandbox_host`); and remote deploy + sync (`deploy_sandbox`, `undeploy_sandbox`, `sandbox_deployments`, `sync_status`, `enable_sync`, `disable_sync`). Deploy-related commands hold both states and orchestrate across the backend (Podman) and the sync engine (Mutagen) so neither subsystem depends on the other. The `default_backend()` function in `src-tauri/src/backends/mod.rs` reads `PODMAN_CONNECTION` and constructs the backend at startup.

### 3. `src-tauri/src/backends/mod.rs` — Backend implementation

Only read this file if the user is actively going to work on this part of the code base. It implements `SandboxBackend` by shelling out to `podman` (lifecycle via `podman --connection <name> ...`, host management via `podman system connection ...`), and contains the guided remote-host provisioning flow. You should have enough info from the first 2 steps to know if you'll need to.
