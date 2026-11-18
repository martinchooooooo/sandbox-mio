---
type: feature
title: "host-tool-ssh-access"
description: "Let host SSH clients (e.g. the Claude Code desktop app) attach to a sandbox over the existing exec transport."
status: in-progress
architecture: [docs/architecture/transport.md]
depends_on: [podman-exec-transport, microsandbox-provider]
---

# host-tool-ssh-access

Let host SSH clients (e.g. the Claude Code desktop app) attach to a sandbox over the existing exec transport.

## Status note

Phases A–E are implemented: the `sandbox_ssh_access` handoff command, the non-interactive `PATH` fix (image 9), alias keepalives and quoting, the "Connect from desktop apps" panel, and destroy-time teardown. Spike S2 and the headless half of S4 have run; S1 is done: Claude Desktop resolves the alias and connects, once the sandbox's host keys are pinned (the app now seeds them from the running container) and the client's port and key fields are left empty. Phase F (standard-tier regression and the new spec) remains.
