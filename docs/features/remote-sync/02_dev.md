---
type: dev-plan
title: "Remote Sync Implementation Plan"
feature: remote-sync
---
# Remote Sync Implementation Plan

This document is the build plan for taking a sandbox that was created locally and **deploying it to a remote host**, then keeping its workspace mirrored back to the user's laptop with a bundled Mutagen sidecar. It continues the Phase 2 design in `docs/remote-workspace-sync.md` and supersedes that document's assumption that remoteness is fixed when a sandbox is created. Mutagen sidecar bundling, the sync engine wrapper, the SSH transport writer, and the connection parser (phases A–C) are implemented as a reusable Rust core; the create-reverts-to-local refactor, the deploy flow with its lifecycle wiring (phase D), and the UI (phase E) are all implemented. The remaining work is the E2E coverage for the deploy/destroy/undeploy paths. Background design rationale lives in the auto-memory `workspace-sync-design`.

## The Deploy-to-Remote Model

Every sandbox is created **locally** — a container on the local Podman machine with its workspace bind-mounted from a laptop directory — and stays sync-agnostic at create time. Remoteness is a later, per-sandbox action: the user picks a registered host and clicks **Deploy to Remote**. Not every local sandbox has a remote counterpart, so sync lifecycle hangs off the deploy action and its toggle, never off `create`.

"Remote" here means the **deploy target's filesystem**, not necessarily a different physical machine: the target can be any registered connection other than the source, including a second local Podman machine. What makes sync meaningful is that the deployed container's workspace lives in the target connection's filesystem (a VM or a remote host), separate from the laptop directory — not whether the target is physically remote. The only rejected target is the source connection itself, where a new container would name-collide with the kept-stopped local one.

Deploying migrates the workload by snapshot-and-push:

1. Stop the local container so its filesystem is quiescent.
2. `podman commit` the local container to an image. A bind-mounted volume is **not** captured by a commit, so the snapshot holds installed packages and container state but **not** the workspace files; those still live only in the laptop directory at this point.
3. Transfer the image to the chosen host with `podman image scp <local-connection>::<image> <target-connection>::` — directly between the two registered Podman connections over SSH, with no registry.
4. Create a container on the target connection from the transferred image, bind-mounting a remote workspace directory created empty over SSH (`remote_mkdir`), and write the host's OpenSSH alias (`write_alias`).
5. Start a Mutagen session named after the sandbox: `alpha` = the laptop directory (which holds the only copy of the workspace), `beta` = `<connection-alias>:<remote-workspace>`. The initial reconcile seeds the empty remote workspace from the laptop; thereafter edits propagate both ways.
6. Start the remote container.

The original local container is **kept, stopped**, not removed, so deploy is reversible without re-snapshotting. The laptop directory and the remote workspace directory are never deleted automatically; for a deployed sandbox the remote workspace becomes the live copy and the laptop directory its mirror.

## Resolved Decisions

The earlier "Open Questions" in `docs/remote-workspace-sync.md` are resolved as follows. Mutagen is bundled as a Tauri sidecar (a vendored binary), not a required external install. Remoteness is **not** fixed at create by which connection a sandbox is born on; every sandbox is created locally and deployed to a remote host later by an explicit action. The image is moved with `podman image scp` between registered connections, not pushed through a container registry. The original local container is kept stopped after a successful deploy, making deploy reversible. The "sync with remote" toggle maps to Mutagen `sync pause` and `sync resume`, not session teardown; session teardown happens only on destroy or an explicit undeploy. SSH transport configuration is supplied to Mutagen through an app-managed OpenSSH config file pulled into the user config with an `Include` directive, not by rewriting the user's own `~/.ssh/config` blocks in place.

Deployment state is **derived from the Mutagen session list**, not stored in a new persistence layer: a session named after the sandbox means the sandbox is deployed, and the session's `beta` URL records which host alias (Podman connection) it is deployed to. This keeps the source of truth in one place and survives app restarts without an extra state file.

## Already Implemented: Sidecar Bundling

Mutagen v0.18.1 is bundled as a Tauri sidecar. `bundle.externalBin` in `src-tauri/tauri.conf.json` declares the `binaries/mutagen` sidecar, and Tauri resolves the platform binary `mutagen-<target-triple>` at build time. The `tauri-plugin-shell` plugin is registered in `run` in `src-tauri/src/lib.rs` and scoped in `src-tauri/capabilities/default.json` with a `shell:allow-execute` entry naming the `binaries/mutagen` sidecar. The `command` function in `src-tauri/src/sync/mod.rs` builds a sidecar invocation with `MUTAGEN_DATA_DIRECTORY` pinned to an app-private directory under the Tauri app data directory, isolating the app's Mutagen daemon and sessions from a Mutagen the user installed separately. The `version` function in `src-tauri/src/sync/mod.rs` and the `mutagen_version` command in `src-tauri/src/lib.rs` are a runtime smoke test that the sidecar resolves and runs. The binary is gitignored (`/src-tauri/binaries/mutagen-*` in `.gitignore`) and staged reproducibly by `fetch_mutagen` in the root task runner `build.rs`, run as part of `build dev --setup`, which downloads the pinned Mutagen release and copies it to the triple-suffixed sidecar name. `fetch_mutagen` is Windows-only for now and uses `todo!()` for other platforms, matching `setup_e2e` in `build.rs`.

The runtime smoke test (invoking `mutagen_version` from the running application) is not yet performed and is the first validation step of the remaining work.

## Already Implemented: Reusable Rust Core (Phases A–C)

The sync engine wrapper, SSH transport writer, and Podman connection parser are implemented and reused unchanged by the deploy flow, though the create-time remote branch they were first wired into is removed (see "Refactor" below):

- The `SyncEngine` trait, `MutagenCli` implementation, and `SyncStatus` enum in `src-tauri/src/sync/types.rs`, with the implementation and the `status_from_session` status collapser in `src-tauri/src/sync/mod.rs`; the `default_sync_engine` constructor registered as Tauri managed state in `run` in `src-tauri/src/lib.rs`; the `sync_status`, `enable_sync`, and `disable_sync` commands; and a best-effort Mutagen `daemon stop` on `RunEvent::Exit`.
- The OpenSSH config writer (`write_alias`, `remove_alias`, `ensure_include`) in `src-tauri/src/ssh/mod.rs` with `SshAlias` in `src-tauri/src/ssh/types.rs`.
- The connection parser `parse_connection_entry` and live lookup `remote_connection_info` in `src-tauri/src/backends/mod.rs`, returning a `ConnectionInfo` (host, port, user, identity) for any SSH-reachable connection — **including a Podman machine** — and `None` only for a connection with no `ssh://` endpoint (e.g. a local unix socket); plus `remote_mkdir`, and the `destroy` method on the `SandboxBackend` trait with its `destroy_sandbox` command.

These helpers carried interim `#[allow(dead_code)]` markers in phases A–C; phase D supplied their callers (deploy, destroy, undeploy), so the markers are gone. `remove_alias` keeps its marker — no host-removal flow is wired yet.

## Implemented: `create` Reverted to Local-Only (Refactor)

Phases A–C wired the remote branch (`remote_mkdir` + `write_alias` + a remote bind-mount source) into `create` in `src-tauri/src/backends/mod.rs`, on the now-superseded assumption that a sandbox could be born on a remote connection. Under the deploy model `create` is always local, so its `SyncStrategy::Remote` branch was removed and `create` now takes the Phase 1 local `workspace_path` + `std::fs::create_dir_all` path with no `resolve_workspace` call. `resolve_workspace` was changed to take an explicit `connection` argument (was `self.connection`) so the deploy flow can resolve the remote bind-mount source against the chosen target; `remote_mkdir`, `remote_connection_info`, `write_alias`, `SshAlias`, `WorkspacePlan`, and `SyncStrategy` are now reached through deploy, so the interim `#[allow(dead_code)]` markers on them are gone. The `is_remote` field was removed from `Sandbox` entirely (every sandbox is created on the local machine); deployment state is reported separately from the Mutagen session list (see "Deployment State"). `list` no longer derives remoteness from the connection.

## Implemented: Phase D (Deploy Flow and Lifecycle)

Phase D added the deploy flow and wired the sync session into the deploy/destroy lifecycle. Because deploy spans both subsystems — Podman operations (commit, `image scp`, remote create) and the Mutagen engine (session create) — and is independent of `create`, the orchestration lives in **Tauri commands in `src-tauri/src/lib.rs`** that hold both managed states, not in the backend reaching into the engine. This keeps `SandboxBackend` free of the sync engine and `SyncEngine` free of Podman, matching the existing separation where `Sandbox` reports no sync state and sync is read through separate commands.

The backend gained the Podman side of deploy plus a remote-removal helper, both running against an arbitrary connection via the new `podman_on(connection, args)` helper (`--connection <target>`, not `self.connection`):

- `deploy(name, target_connection) -> Result<RemoteDeployment>` (`src-tauri/src/backends/mod.rs`): rejects only a target equal to the source connection (which would name-collide with the kept-stopped local container) — any other connection, including a different local Podman machine, is a valid target — then resolves the remote endpoint, stops the local container, `podman commit`s it to `sandbox-mio/<name>:deployed`, `podman image scp`s the image to `target_connection` (connection-qualified names, no registry), `remote_mkdir`s the empty remote workspace, `write_alias`es the target's OpenSSH alias keyed to the connection name, then creates and starts the remote container. It returns `RemoteDeployment { alpha, beta }` — `alpha` the laptop dir, `beta` the `<alias>:<remote-workspace>` scp-style URL.
- `remove_remote(name, connection)` removes the container on the deploy target; the remote workspace directory is left intact.

The commands in `src-tauri/src/lib.rs`:

- `deploy_sandbox(name, host)` calls `backend.deploy` then `SyncEngine::create(name, alpha, beta)` to seed and start the session.
- `destroy_sandbox(name, force)` is the guard: it looks the sandbox up in `SyncEngine::sessions` (absent ⇒ purely local ⇒ direct local removal, no guard); for a deployed sandbox it best-effort `flush`es, and unless `force` is set it returns an error when `SyncEngine::status` is not `UpToDate` (the frontend confirms and retries with `force`), then `terminate`s the session, `remove_remote`s the remote container, and removes the local container.
- `undeploy_sandbox(name)` (the reversibility payoff of keeping the local container) flushes remote→laptop, terminates the session, removes the remote container, and restarts the stopped local container so the laptop bind-mount resumes.

## Implemented: Deployment State

The frontend needs, per sandbox, whether it is deployed and to which host, plus live sync status. Rather than derive this in the backend (which would couple `list` to the engine), `SyncEngine::sessions` reports the set of active sessions as `Vec<SessionInfo>` — `name` (= sandbox name), `host` (the alias parsed from the `beta` URL by `parse_session` in `src-tauri/src/sync/mod.rs`), and collapsed `SyncStatus` — exposed through the `sandbox_deployments` command. The same lookup gives `destroy_sandbox`/`undeploy_sandbox` the target connection. The frontend (phase E) overlays this onto the local sandbox list from `list_sandboxes`: a sandbox with a matching session renders as deployed (to the parsed host) with its status dot; one without renders as local-only with a **Deploy to Remote** control.

## Testing Strategy

Per project preference, the deploy/destroy/undeploy and parsing logic is validated by **E2E tests, not Rust unit tests** — the unit tests that previously covered `parse_connection_entry`, `status_from_session`, `parse_session`, and the SSH config writer were intentionally removed. The pure parsers (`parse_connection_entry`, `status_from_session`, `parse_session`) are still the natural seams to assert against from an E2E harness. The deploy target can be any SSH-reachable connection other than the source, so a **second local Podman machine** is a convenient e2e target — no external host required; the deploy creates the container in that machine's filesystem and Mutagen syncs it back to the laptop directory.

## Implemented: Phase E (UI)

The frontend in `src/routes/projects/+page.svelte` overlays deployment state onto the sandbox list. It loads `hosts` (`list_sandbox_hosts`) and `deployments` (the `sandbox_deployments` command, a `Vec<SessionInfo>` of `{ name, host, status }` keyed by name) on mount, and polls `sandbox_deployments` every 3s because sync status drifts outside the container event stream (the interval is cleared in `onDestroy`). A non-deployed item shows a **deploy** control that opens an inline host picker (a native `<select>` of registered hosts, with an empty-state message) and calls `deploy_sandbox` with the chosen host. A deployed item shows the host (`→ <host>`), a sync-status dot via the `syncColor` helper (green up-to-date, blue syncing, yellow conflict, red error — the coloured-dot idiom reused from container state), a "sync with remote" toggle (the shadcn-svelte `switch` component) wired to `enable_sync`/`disable_sync`, and an **undeploy** control wired to `undeploy_sandbox`. The destroy control calls `destroy_sandbox` with `force = false`; on an unsynced-changes error it `confirm()`s and retries with `force = true`.

The `Project` interface in `+page.svelte` has no `is_remote` field (the Rust struct dropped it); deployment is determined by the presence of a matching entry in `deployments`. The sync toggle's on/off state is tracked client-side in a `syncOn` map (optimistic, reverting on error) because Mutagen does not surface a paused state through `SyncStatus` — see the Common Gotchas row.

## Daemon Lifecycle

The Mutagen daemon auto-starts on the first sync session command, so no explicit daemon start is wired. Because `MUTAGEN_DATA_DIRECTORY` isolates the app's daemon, a best-effort `daemon stop` runs on `RunEvent::Exit` in `run` in `src-tauri/src/lib.rs`, so the app does not orphan its private daemon. Stopping the app-private daemon never affects a Mutagen the user installed separately.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Tauri build fails because the Mutagen sidecar binary is absent | The Mutagen binary is gitignored and not committed | Run `build dev --setup` once after clone and in CI before `pnpm tauri build`; `fetch_mutagen` in `build.rs` stages the sidecar |
| Mutagen sidecar fetch hits `todo!()` on a non-Windows platform | `fetch_mutagen` in `build.rs` is implemented for Windows only | Add the non-Windows download and target-triple mapping to `fetch_mutagen` in `build.rs` |
| App's Mutagen sessions collide with a user-installed Mutagen | A shared default Mutagen data directory means one daemon serves both | Keep `MUTAGEN_DATA_DIRECTORY` pinned to the app-private directory set by `command` in `src-tauri/src/sync/mod.rs` |
| Deployed workspace is empty on the remote even though the laptop has files | A bind-mounted volume is not captured by `podman commit`, so the snapshot carries no workspace files and the remote bind-mount source is created empty | Create the Mutagen session with `alpha` = laptop dir (the only copy at deploy time) and `beta` = the empty remote workspace; the initial reconcile seeds the remote from the laptop |
| Deploy creates the remote container on the wrong host | Deploy reused the backend's pinned `self.connection` instead of the user-chosen target | Run deploy's Podman operations against the target connection (`--connection <target>`, connection-qualified `image scp` names), not `self.connection` |
| `podman commit` misses recent in-container changes | The container was committed while running and mid-write | Stop the local container before committing so its filesystem is quiescent |
| Mutagen sync session cannot connect to a non-standard SSH port | A Mutagen scp-style `alias:path` URL cannot carry a port or identity file | Write an OpenSSH `Host` alias to `~/.ssh/sandbox-mio/config` and pull it in with a one-time `Include sandbox-mio/config` line in `~/.ssh/config` |
| Mutagen sync stops working after a Podman machine restart | A Podman machine's SSH port is assigned dynamically at boot, so a cached port points at the wrong port after a restart | Read host, port, user, and identity live from `podman system connection list` when deploying, never cached |
| User's `~/.ssh/config` is corrupted by sync setup | Concurrent or non-atomic rewrites of the shared user config file | Touch `~/.ssh/config` only for the one-time `Include` insertion, guarded by an in-process mutex and written with an atomic temp-file rename; keep churny writes in the app-owned `~/.ssh/sandbox-mio/config` |
| Sync toggle loses conflict state and re-scans on every flip | The toggle terminates and recreates the Mutagen session instead of pausing it | Map the toggle to Mutagen `sync pause` and `sync resume`; reserve `sync terminate` for destroy and undeploy |
| Remote workspace work lost after destroying a sandbox | The deployed sandbox was destroyed while the remote workspace held unsynced changes | `destroy_sandbox` flushes and checks status, then errors unless `force` is set; the frontend confirms and retries with `force = true`. Never delete the remote workspace directory |
| A local-only sandbox triggers a spurious destroy warning or a failed terminate | The destroy guard assumed every sandbox has a session | Treat an absent Mutagen session named after the sandbox as "purely local": skip the flush/status guard and the terminate |
| The stopped local container is shown as just "stopped" after deploy | `list_sandboxes` reports the kept-stopped local container with no deployment context | Overlay the deployment-state command (Mutagen session list) onto the sandbox list so a deployed sandbox renders as deployed-to-`<host>`, not stopped-locally |
| Adding a custom `switch` element instead of the shared component | shadcn-svelte components belong in `src/lib/components/ui/`, not hand-rolled | The `switch` component is now installed (`src/lib/components/ui/switch`); use it. Its CLI add (`pnpm dlx shadcn-svelte@latest add <name>`) needs an interactive TTY, so run it in a real terminal, not a non-interactive shell |
| Sync toggle shows the wrong on/off position after a restart | Mutagen does not expose a paused state through the collapsed `SyncStatus`, so a paused session still reports `syncing`/`up-to-date` | The UI tracks toggle state client-side in `syncOn` (optimistic), defaulting on. To make it authoritative, surface the session's paused flag through `SessionInfo` and drive the toggle from it |
| App leaves a Mutagen daemon running after exit | The app-private daemon auto-started on first session command and was never stopped | A best-effort Mutagen `daemon stop` runs on `RunEvent::Exit` in `run` in `src-tauri/src/lib.rs` |
| Deploy rejected as "onto its own connection" | The chosen target is the backend's pinned source connection (`PODMAN_CONNECTION`, default `podman-machine-default`); a new container there would name-collide with the kept-stopped local one | Pick any other connection as the target. A Podman machine is a valid target (the parser no longer treats machines as local), so a second local machine works for an app-driven e2e without an external host |
