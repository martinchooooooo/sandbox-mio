---
type: architecture
title: "Transport: SSH Aliases, Mutagen and Ports"
description: "How the app reaches services inside a sandbox: per-sandbox SSH aliases tunnelled through the engine's exec, Mutagen sync sessions and forwards, and port exposure."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-10T18:00:00+11:00
stale_after: 2027-02-05T00:00:00+11:00
code:
  - src-tauri/src/ssh/
  - src-tauri/src/sync/
  - src-tauri/src/network/
  - src-tauri/src/services/routing.rs
sources:
  - resource: docs/features/podman-exec-transport/02_dev.md
  - resource: docs/features/mutagen-transport/02_dev.md
  - resource: docs/features/expose-ports/04_improvements.md
  - resource: docs/features/local-llm-reachability/02_dev.md
  - resource: docs/features/techdebt-sweep/02_dev.md
---
# Transport: SSH Aliases, Mutagen and Ports

Sandboxes publish no ports. The app reaches every in-sandbox service (the web terminal, the agent web UI, the user's own dev servers) through Mutagen forwards, and mirrors deployed and microVM workspaces through Mutagen sync sessions. Mutagen connects over SSH aliases the app writes. This document covers that path end to end.

## The Exec Transport

- Each sandbox runs a small SSH server from the base image and has its own keypair in the app-data root, installed into the sandbox at create.
- The app writes one SSH alias per sandbox into an app-managed config file that the user's `~/.ssh/config` includes. The alias's `ProxyCommand` starts that SSH server over the engine's own `exec` (`podman exec` or `msb exec`), so the SSH session lands inside the sandbox and the sandbox's loopback ports are directly reachable.
- Both engines tunnel to the same in-sandbox SSH server, so everything above the alias is engine-neutral.
- The alias pins the user, the sandbox's key and a per-sandbox known-hosts file. The host key is baked into the base image and changes when the image is rebuilt, so it is kept out of the user's own known hosts.
- A routing service resolves a sandbox to its provider, machine and alias for every caller, and rewrites the alias on every pane or port open, because the alias file may not survive between app runs the way the sandbox does.

## Mutagen

- Mutagen is bundled with the app and runs with an app-private data directory, so it never shares state with a user-installed Mutagen. Its daemon outlives the app.
- **Sync sessions** mirror a workspace between the host and a sandbox: one per deployed sandbox ([Remote](remote.md)), and one per microsandbox sandbox to keep its disk volume in step with the host folder.
- **Forwards** connect a fresh OS-assigned host loopback port to a port inside a sandbox, through the sandbox's alias, so concurrent app instances never collide.
- **Reverse forwards** run the other way: each running local sandbox gets a loopback listener relaying to the host inference server ([Local Inference](inference.md)).
- At startup the app ends forwards whose sandbox no longer exists, and every inference forward, since no server runs yet.

## Ports and Exposure

- A network manager tracks every forward by sandbox and service (terminal, agent web UI, inference, or an exposed port) and pushes changes to the frontend. A tracked forward's own in-sandbox listener is never auto-exposed.
- Per sandbox, a poller exposes the ports declared in the workspace's `.mio/ports.json` and the ports detected listening inside the sandbox. Each source removes only the ports it added; a port the user exposed by hand is never removed automatically.
- Re-applying a sandbox's network, undeploying or destroying it closes its panes and forwards first; a plain stop closes only the inference forward and leaves the rest registered.

## Invariants

- The managed SSH config is written only under a cross-process lock, because two app instances can race.
- Every sandbox, Podman or microsandbox, is reached through its own SSH alias; no port is published and no relay runs inside a sandbox.
- Callers reach a sandbox only through the routing service, which refreshes the alias.
- The in-sandbox SSH server is named in `ProxyCommand` by bare name, never by absolute path, so Git Bash path conversion cannot rewrite it.
- Creating a forward is serialised and idempotent: it replaces any same-named forward.
