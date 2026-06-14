# Rust Project Orientation

This document tells an AI agent which files to read first to understand the `src-tauri` Rust codebase. All source files live under `src-tauri/src/`.

## Recommended Reading Order

Read files in the order below. Each file builds on the previous, so reading out of order requires backtracking.

### 1. `src-tauri/src/backends/types.rs` — Public API surface and internal contracts

Start here. `src-tauri/src/backends/types.rs` declares everything other files depend on: the `Sandbox` struct (the data shape returned to the frontend), the `SandboxBackend` trait (the interface all backend implementations must satisfy), and the `PodmanBackend` trait (an internal helper shared by both Podman backends). `src-tauri/src/backends/types.rs` also documents the two environment variables that control runtime behaviour (`PODMAN_MACHINE`, `PODMAN_PIPE`). Reading this file first means all subsequent method signatures, type references, and machine/pipe resolution logic are already familiar.

### 2. `src-tauri/src/lib.rs` — Tauri command handlers and backend wiring

Read second. `src-tauri/src/lib.rs` is the crate entry point. `src-tauri/src/lib.rs` registers all Tauri commands and wires the chosen `SandboxBackend` implementation into Tauri state as `Arc<dyn SandboxBackend>`. The `default_backend()` function in `src-tauri/src/backends/mod.rs` is where backend selection happens at startup.

### 3. `src-tauri/src/backends/mod.rs` — Backend implementations

Only read this file if the user is actively going to work on this part of the code base. You should have enough info from the first 2 steps to know if you'll need to.
