---
type: architecture
title: "Remote Deploy and Sync"
description: "Moving a Podman sandbox to a remote Podman host while its workspace stays editable on the laptop: hosts, deploy, deployment state, undeploy and destroy."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-05T23:55:00+11:00
stale_after: 2027-02-05T00:00:00+11:00
code:
  - src-tauri/src/commands/deploy.rs
  - src-tauri/src/commands/hosts.rs
  - src-tauri/src/backends/
  - src-tauri/src/services/sandbox.rs
  - src-tauri/src/runtime/
  - src/routes/projects/RemoteManager.svelte
sources:
  - resource: docs/features/remote-deploys/01_ideas.md
  - resource: docs/features/remote-sync/02_dev.md
  - resource: docs/features/remote-terminal/02_dev.md
  - resource: docs/features/sandbox-runtime-management/02_dev.md
---
# Remote Deploy and Sync

A Podman sandbox can move from the laptop to a remote Podman host while its workspace stays editable on the laptop: the remote container does the work, and a Mutagen sync session mirrors the workspace in both directions. microsandbox sandboxes cannot be deployed ([Providers](providers.md)).

## Remote Hosts

- A remote host is a Podman system connection; the app stores nothing else about it.
- The app can register a connection, prepare a host (enable the rootless Podman socket and user lingering over SSH), or run guided setup for an external Linux server. The server must already accept the user's SSH key.

## Deploy

1. **Move the container.** Resolve the remote endpoint before touching anything local, then stop (but keep) the local container, create the remote workspace folder, build the base image on the remote if it is missing (the image is never pulled from a registry), and create the remote container. The sandbox's alias is repointed at the remote container ([Transport](transport.md)).
2. **Rebuild the software.** Installed tools are replayed on the remote from the workspace's `mise.toml`. Software moves as a manifest, never as a committed image.
3. **Start syncing.** A Mutagen sync session named after the sandbox mirrors the laptop workspace and the remote one.

A sandbox with a proxy sidecar is refused, because the remote side has no proxy ([Network](network.md)).

## Deployment State

- A sync session named exactly after a sandbox is the only record that the sandbox is deployed. Routing sends a deployed sandbox's terminal and port operations to its remote connection.
- Sync status (syncing, up to date, conflict, error) is shown per sandbox. Sync can be paused and resumed without undeploying.
- The terminal and agent web UI work unchanged on a deployed sandbox, because their forwards tunnel through the sandbox's alias, which now points at the remote container.

## Undeploy and Destroy

The two flows order their steps differently on purpose ([lib-decomposition plan](../features/lib-decomposition/02_dev.md), Resolved Decision 2); do not unify them.

- **Undeploy:** close panes and forwards, flush the session to pull remote edits, remove the remote container, end the session, start the local container. The remote container goes first because ending the session drops Mutagen's SSH connections, which can briefly leave the machine unable to remove the container.
- **Destroy of a deployed sandbox:** flush, then refuse unless the session is up to date or the user confirms losing unsynced remote work; then close panes and forwards, end the session, remove the remote container, and remove the local container.
- Neither flow deletes the laptop workspace.

## Invariants

- Only tools installed through mise reach the remote host.
- Remote connection details, notably the SSH port that changes on every machine boot, are re-read on every use and never cached.
- Only a session named exactly after a sandbox means deployed; a microsandbox workspace session uses a different name.
