# Remote Workspace and File Sync Plan

This document is the implementation plan for running a sandbox container on a remote host while keeping its workspace files mirrored to the user's laptop. Phase 1 (local-only workspaces) is already implemented in `src-tauri/src/backends/mod.rs`; Phase 2 (remote workspaces with sync) is unimplemented and described here so a future agent can continue the work. Background on the design decisions is recorded in the auto-memory `workspace-sync-design`.

## Current State (Phase 1, Local)

Every sandbox gets a host directory bind-mounted into the container at `/root/work`. The directory path is computed by `workspace_path` in `src-tauri/src/backends/mod.rs` as `<root>/<name>`, where the root comes from `workspace_root` in the same file (`SANDBOX_WORKSPACE_ROOT` if set, otherwise `<Documents>/sandbox-mio`). `create` in `src-tauri/src/backends/mod.rs` creates the directory with `std::fs::create_dir_all` on the laptop, then passes `--volume <path>:/root/work` and `--workdir /root/work` to `podman create`. The host directory path is returned to the frontend in the `workspace_path` field of the `Sandbox` struct (`src-tauri/src/backends/types.rs`).

Phase 1 works because the local Podman machine is a WSL VM that auto-mounts the Windows user profile, so a laptop path handed to `podman --volume` resolves inside the container. Phase 1 has no sync step: the bind-mount source and the laptop directory are the same directory.

## The Workspace Resolver Seam

The `resolve_workspace` resolver in `src-tauri/src/backends/mod.rs` is the implemented seam (the Phase 1 `workspace_path` free function still computes the laptop path it builds on). `resolve_workspace` returns a `WorkspacePlan` with three values per sandbox: the bind-mount source path on the Podman host, the laptop directory path, and the sync strategy. For a local connection the two paths stay equal and the sync strategy is `SyncStrategy::Local`, a no-op, preserving Phase 1 behaviour. Keeping remote support behind `resolve_workspace` avoids changing the shape of `create` in `src-tauri/src/backends/mod.rs`.

Local versus remote is decided by the `IsMachine` field of the connection's entry in `podman system connection list`, parsed by `parse_connection_entry` in `src-tauri/src/backends/mod.rs`, not by comparing the connection name to `podman-machine-default`. A Podman machine (`IsMachine` true) auto-mounts the user profile and is local; only a non-machine connection registered via `provision_host` in `src-tauri/src/backends/mod.rs` is remote. The build plan `docs/plans/remote-sync.md` records the full rationale and the consequence for app-driven remote-path e2e tests.

## Remote Workspace Model

A Podman bind mount always resolves on the machine where the container runs, not on the machine running the Tauri app. The `PodmanCliBackend` in `src-tauri/src/backends/types.rs` drives `podman --connection <name>`, and Podman performs the SSH transport, so for a remote connection the container runs on the remote server and `/root/work` is bound to a directory on the remote server's filesystem. The remote workspace directory must therefore be created over the connection (an `mkdir -p` invoked through the same SSH identity the connection already uses) rather than with the laptop-side `std::fs::create_dir_all` that Phase 1 uses in `create`. Remote-host SSH connections are registered and verified by `add_host`, `prepare_host`, and `provision_host` in `src-tauri/src/backends/mod.rs`, which already manage the identity key Phase 2 sync can reuse.

## File Sync Engine

Mutagen is the chosen engine for live bidirectional sync between the remote workspace directory and the laptop directory. Mutagen is purpose-built for syncing a working directory to a remote host over SSH, supports real-time bidirectional sync with conflict handling and ignore rules, and can reuse the SSH transport and identity key that the Podman system connection already holds (registered through `add_host` in `src-tauri/src/backends/mod.rs`). A Mutagen sync session should be created when a remote sandbox is created and terminated when the remote sandbox is destroyed. Syncthing and rclone bisync were evaluated and rejected as the primary engine: Syncthing adds a continuously running daemon on both ends, and rclone bisync reconciles on a schedule rather than live.

## Validation With Two Podman Machines

The remote-sync model was validated end-to-end on a single laptop by treating a second Podman machine as the remote host, removing the need for a real cloud server during development. The development machine (`podman-machine-default`) plays the laptop and the test machine (`mio-tests`) plays the remote host. Because Podman reaches `mio-tests` over SSH exactly as it would a real server, the topology is faithful: a container created on the `mio-tests` connection bind-mounts a directory that resolves inside the `mio-tests` VM filesystem, not on the laptop, reproducing the remote bind-mount behaviour described above.

Mutagen bridged the gap in the validation. A two-way-resolved Mutagen session was created between a laptop directory under the Windows user profile and the remote workspace directory inside the `mio-tests` VM. The session's initial pull copied a container-written file down to the laptop, and subsequent edits propagated in both directions within a few seconds: a file written inside the container appeared on the laptop, and a file written on the laptop appeared inside the container with intact contents. This confirms the engine choice and confirms that Mutagen can reuse the SSH identity Podman already holds.

Mutagen uses the system OpenSSH client as its transport and honours the user's SSH configuration, so the validation pointed Mutagen at the Podman machine through a host alias that named the machine's SSH port and the Podman-generated identity key. The identity key is the same one the `mio-tests` system connection already records, the key Phase 2 sync is expected to reuse. The Podman machine SSH port is assigned dynamically at machine boot and changes on every machine restart, so a host alias or Mutagen session that hard-codes the port breaks after a `podman machine` restart. A real remote host has a stable port, making the dynamic port a validation-only artifact; the Phase 2 implication is that the backend must read the port and identity from the live system-connection list when it creates a sync session rather than caching them.

## Git Checkpoints

Git checkpoints are a planned feature layered on top of Mutagen live sync, not a replacement for Mutagen. A checkpoint is an explicit user action that commits the current workspace state so non-technical users have a "versions of my work" model. Git checkpoints must not sit in the live edit path; Mutagen handles continuous sync and Git records discrete, user-initiated snapshots. Git-based sync as the primary transport was rejected because continuous editing, build artefacts, and binary files make commit-per-change impractical.

## Destroy Guard and Lifecycle

Destroying a remote sandbox must check the Mutagen sync status before removing the container. If the remote workspace has unsynced changes that have not reached the laptop, destruction must surface an explicit warning to the user rather than proceeding silently, because the remote workspace directory holds the only copy of unsynced work. The remote workspace directory persists across the container lifecycle: stopping or destroying the container does not delete the remote workspace directory, following the same principle as the destroy guard.

## Open Questions

The build plan in `docs/plans/remote-sync.md` resolves three earlier open questions: Mutagen is bundled as a vendored Tauri sidecar (not a required external install), the "sync with remote" toggle maps to Mutagen `sync pause` and `sync resume` (not session teardown), and SSH transport configuration is supplied through an app-managed OpenSSH config file pulled in with an `Include` directive (not by rewriting the user's own config blocks). Two decisions remain unresolved and are not yet covered by the plan. When a remote sandbox is stopped (not destroyed), whether Mutagen performs a final flush to the laptop before the container halts is undecided. Whether the remote workspace directory is ever torn down automatically, or only ever removed by an explicit user action, is undecided.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Remote sandbox workspace files do not appear on the laptop | A Podman bind mount resolves on the remote Podman host, and no sync engine copies the remote workspace directory back to the laptop | Create a Mutagen sync session between the remote workspace directory and the laptop directory when the remote sandbox is created |
| `create` fails creating the workspace directory for a remote connection | `create` in `src-tauri/src/backends/mod.rs` uses laptop-side `std::fs::create_dir_all`, which cannot create a directory on the remote server | For remote connections, `create` in `src-tauri/src/backends/mod.rs` calls `remote_mkdir`, which runs `mkdir -p` over the connection's SSH identity instead of `std::fs::create_dir_all` |
| A Podman machine connection is misclassified as remote, or a real server as local | Local-versus-remote was decided by connection name instead of the `IsMachine` field of `podman system connection list` | Decide remoteness from `IsMachine` in `parse_connection_entry` (`src-tauri/src/backends/mod.rs`); Podman machines are local, only non-machine connections are remote |
| Remote workspace work lost after destroying a sandbox | The remote sandbox was destroyed while the remote workspace directory held unsynced changes | Check Mutagen sync status before destroy and warn the user when unsynced changes exist; never delete the remote workspace directory on container stop or destroy |
| Phase 2 changes break local sandbox creation | Remote logic was added directly into `create` instead of behind the resolver | Route bind-mount source path, laptop path, and sync strategy through `workspace_path` in `src-tauri/src/backends/mod.rs`, returning equal paths and a no-op strategy for the local connection |
| Git checkpoints conflict with live edits | Git was used as the live sync transport rather than as a discrete snapshot layer | Use Mutagen for continuous sync and reserve Git checkpoints for explicit user-initiated snapshots |
| Mutagen sync stops working after a Podman machine restart | The Podman machine SSH port is assigned dynamically at boot, so a host alias or Mutagen session that hard-codes the port points at the wrong port after a restart | Read the port and identity from the live system-connection list when creating a sync session rather than caching them; this affects the two-machine validation setup, not real remote hosts with stable ports |
