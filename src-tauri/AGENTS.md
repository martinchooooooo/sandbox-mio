# Rust Project Orientation

This document tells an AI agent which files to read first to understand the `src-tauri` Rust codebase. All source files live under `src-tauri/src/`.

## Recommended Reading Order

Read files in the order below. Each file builds on the previous, so reading out of order requires backtracking.

### 1. `src-tauri/src/backends/types.rs` — Public API surface

Start here. `src-tauri/src/backends/types.rs` declares the two types every other file depends on: the `Sandbox` struct (the data shape returned to the frontend) and the `SandboxBackend` trait (the interface all backend implementations must satisfy). Reading `src-tauri/src/backends/types.rs` first means all subsequent method signatures and type references are already familiar.

### 2. `src-tauri/src/backends/internals.rs` — Internal implementation contract

Read second. `src-tauri/src/backends/internals.rs` declares the `PodmanBackend` trait, an internal helper shared by both Podman backend implementations. `src-tauri/src/backends/internals.rs` also documents the two environment variables that control runtime behaviour. Reading `src-tauri/src/backends/internals.rs` before the implementations avoids confusion about where machine and pipe resolution logic originates.

### 3. `src-tauri/src/lib.rs` — Tauri command handlers and backend wiring

Read third. `src-tauri/src/lib.rs` is the crate entry point. `src-tauri/src/lib.rs` registers all Tauri commands and wires the chosen `SandboxBackend` implementation into Tauri state as `Arc<dyn SandboxBackend>`. The `run()` function in `src-tauri/src/lib.rs` is where backend selection happens at startup.

### 4. `src-tauri/src/backends/mod.rs` — Backend implementations

Only read these files if the user is actively going to work on this part of the code base. You should have enough info from the first 3 steps to know if you'll need to.
