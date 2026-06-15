# Remote Workspace and File Sync Plan

This document is the implementation plan for running a sandbox container on a remote host while keeping its workspace files mirrored to the user's laptop. Phase 1 (local-only workspaces) is already implemented in `src-tauri/src/backends/mod.rs`; Phase 2 (remote workspaces with sync) is unimplemented and described here so a future agent can continue the work. Background on the design decisions is recorded in the auto-memory `workspace-sync-design`.

## Current State (Phase 1, Local)

Every sandbox gets a host directory bind-mounted into the container at `/root/work`. The directory path is computed by `workspace_path` in `src-tauri/src/backends/mod.rs` as `<root>/<name>`, where the root comes from `workspace_root` in the same file (`SANDBOX_WORKSPACE_ROOT` if set, otherwise `<Documents>/sandbox-mio`). `create` in `src-tauri/src/backends/mod.rs` creates the directory with `std::fs::create_dir_all` on the laptop, then passes `--volume <path>:/root/work` and `--workdir /root/work` to `podman create`. The host directory path is returned to the frontend in the `workspace_path` field of the `Sandbox` struct (`src-tauri/src/backends/types.rs`).

Phase 1 works because the local Podman machine is a WSL VM that auto-mounts the Windows user profile, so a laptop path handed to `podman --volume` resolves inside the container. Phase 1 has no sync step: the bind-mount source and the laptop directory are the same directory.

## The Workspace Resolver Seam

`workspace_path` in `src-tauri/src/backends/mod.rs` is the single function Phase 2 must extend. Phase 1 returns one forward-slash path that serves as both the laptop directory and the bind-mount source. Phase 2 should make `workspace_path` (or a replacement resolver) return three things per sandbox: the bind-mount source path on the Podman host, the laptop directory path, and the sync strategy. For the local connection the two paths stay equal and the sync strategy is a no-op, preserving Phase 1 behaviour. Keeping remote support behind `workspace_path` avoids changing the shape of `create` in `src-tauri/src/backends/mod.rs`.

## Remote Workspace Model

A Podman bind mount always resolves on the machine where the container runs, not on the machine running the Tauri app. The `PodmanCliBackend` in `src-tauri/src/backends/types.rs` drives `podman --connection <name>`, and Podman performs the SSH transport, so for a remote connection the container runs on the remote server and `/root/work` is bound to a directory on the remote server's filesystem. The remote workspace directory must therefore be created over the connection (an `mkdir -p` invoked through the same SSH identity the connection already uses) rather than with the laptop-side `std::fs::create_dir_all` that Phase 1 uses in `create`. Remote-host SSH connections are registered and verified by `add_host`, `prepare_host`, and `provision_host` in `src-tauri/src/backends/mod.rs`, which already manage the identity key Phase 2 sync can reuse.

## File Sync Engine

Mutagen is the chosen engine for live bidirectional sync between the remote workspace directory and the laptop directory. Mutagen is purpose-built for syncing a working directory to a remote host over SSH, supports real-time bidirectional sync with conflict handling and ignore rules, and can reuse the SSH transport and identity key that the Podman system connection already holds (registered through `add_host` in `src-tauri/src/backends/mod.rs`). A Mutagen sync session should be created when a remote sandbox is created and terminated when the remote sandbox is destroyed. Syncthing and rclone bisync were evaluated and rejected as the primary engine: Syncthing adds a continuously running daemon on both ends, and rclone bisync reconciles on a schedule rather than live.

## Git Checkpoints

Git checkpoints are a planned feature layered on top of Mutagen live sync, not a replacement for Mutagen. A checkpoint is an explicit user action that commits the current workspace state so non-technical users have a "versions of my work" model. Git checkpoints must not sit in the live edit path; Mutagen handles continuous sync and Git records discrete, user-initiated snapshots. Git-based sync as the primary transport was rejected because continuous editing, build artefacts, and binary files make commit-per-change impractical.

## Destroy Guard and Lifecycle

Destroying a remote sandbox must check the Mutagen sync status before removing the container. If the remote workspace has unsynced changes that have not reached the laptop, destruction must surface an explicit warning to the user rather than proceeding silently, because the remote workspace directory holds the only copy of unsynced work. The remote workspace directory persists across the container lifecycle: stopping or destroying the container does not delete the remote workspace directory, following the same principle as the destroy guard.

## Open Questions

The following decisions are unresolved and block a complete Phase 2 implementation. When a remote sandbox is stopped (not destroyed), whether Mutagen performs a final flush to the laptop before the container halts is undecided. Whether the remote workspace directory is ever torn down automatically, or only ever removed by an explicit user action, is undecided. How a Mutagen sync session is bundled and invoked from the Rust backend (vendored binary versus required external install) is undecided.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Remote sandbox workspace files do not appear on the laptop | A Podman bind mount resolves on the remote Podman host, and no sync engine copies the remote workspace directory back to the laptop | Create a Mutagen sync session between the remote workspace directory and the laptop directory when the remote sandbox is created |
| `create` fails creating the workspace directory for a remote connection | `create` in `src-tauri/src/backends/mod.rs` uses laptop-side `std::fs::create_dir_all`, which cannot create a directory on the remote server | For remote connections, create the workspace directory over the connection with `mkdir -p` through the registered SSH identity instead of `std::fs::create_dir_all` |
| Remote workspace work lost after destroying a sandbox | The remote sandbox was destroyed while the remote workspace directory held unsynced changes | Check Mutagen sync status before destroy and warn the user when unsynced changes exist; never delete the remote workspace directory on container stop or destroy |
| Phase 2 changes break local sandbox creation | Remote logic was added directly into `create` instead of behind the resolver | Route bind-mount source path, laptop path, and sync strategy through `workspace_path` in `src-tauri/src/backends/mod.rs`, returning equal paths and a no-op strategy for the local connection |
| Git checkpoints conflict with live edits | Git was used as the live sync transport rather than as a discrete snapshot layer | Use Mutagen for continuous sync and reserve Git checkpoints for explicit user-initiated snapshots |
