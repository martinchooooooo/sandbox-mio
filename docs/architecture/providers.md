---
type: architecture
title: "Providers, Machines and the Process Layer"
description: "The Podman and microsandbox engines behind one provider seam, how each is made ready on the host, and how every external process is run."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-10T23:30:00+11:00
stale_after: 2027-02-05T00:00:00+11:00
code:
  - src-tauri/src/provider/
  - src-tauri/src/machine/
  - src-tauri/src/process/
  - src-tauri/src/backends/
  - src-tauri/resources/Containerfile
sources:
  - resource: docs/features/microsandbox-provider/02_dev.md
  - resource: docs/features/podman-startup-recovery/02_dev.md
  - resource: docs/features/podman-process-isolation/02_dev.md
  - resource: docs/features/sandbox-runtime-management/02_dev.md
  - resource: docs/features/techdebt-sweep/02_dev.md
---
# Providers, Machines and the Process Layer

A sandbox runs on Podman or microsandbox behind one provider seam. This document covers how the engines differ, how each is made ready, and the process layer.

## The Provider Seam

- A provider exposes an engine-neutral instance API: build or load the base image; create, start, stop, recreate and remove instances; run commands inside them; manage sidecars and networks.
- Create takes a provider-neutral spec (name, image, workspace, environment, network, optional egress policy) that each provider translates into its own engine's flags.
- Each provider declares its capabilities as flags. The frontend hides controls a provider cannot honour, and the backend refuses them as a second gate.
- The engine is chosen per sandbox at create, defaulting to an app setting, and is never stored: the registry asks each engine in turn, Podman first. Sandbox names are unique across both engines.

## Comparison

| | Podman | microsandbox |
|---|---|---|
| Runs as | Containers in a WSL-backed Podman machine, through a bundled Podman client | libkrun microVMs, through the bundled `msb` CLI, never a host install |
| Host prerequisite | A Podman machine | The Windows hypervisor platform and a clean `msb doctor` |
| Base image | Built locally from the repository's Containerfile; never pulled | Built by Podman, then exported and loaded into msb |
| Workspace | Host folder bind-mounted into the container | Disk volume inside the VM, mirrored to the host folder by a Mutagen sync session |
| Recreate, keeping the workspace | Not supported yet | Supported; used to apply network changes |
| Remote deploy | Supported ([Remote](remote.md)) | Not supported |
| Egress policy | Not enforced | Enforced by msb's network stack ([Network](network.md)) |
| Engine event stream | Yes | No |

## Podman Machine Readiness

- Machine lifecycle (WSL install, machine init and start, socket activation, readiness polling) always targets the default Podman machine, never the container connection, which may name a remote host no machine command can act on.
- At startup a stopped machine or missing socket surfaces as a recoverable error; starting the machine runs as an activity.
- Onboarding's stepped setup and the projects page's recovery run one guarded bring-up: it never starts a running machine and never shuts WSL down while another machine runs.

## microsandbox Readiness

- The bundled `msb` must meet a minimum version, the hypervisor platform must be enabled, and `msb doctor` must pass.
- The msb store is shared host-wide unless relocated, which the e2e harness does per worktree.
- microsandbox needs Podman once per base-image version, to build the image it loads.

## The Process Layer

- Every external binary gets a windowless console of its own, and the app clears handle inheritance at startup; otherwise packaged builds flash a window per call and terminal-launched dev builds can hang.
- One-shot calls go through a runner seam whose scripted implementations back the `light` test tier ([Testing](testing.md)). Streaming calls (interactive exec, event streams) bypass the seam and must check the mock switch themselves.
- Only one container event stream runs at a time: a new subscription, such as a frontend reload's, ends the previous one and its Podman process, since the old stream cannot tell its page is gone.
- Podman calls carry per-call-shape timeouts. A timeout trips a process-wide breaker: while it is open, one probe call at a time goes through and the rest fail fast until an answer closes it.
- msb and Mutagen one-shot calls are bounded too; a timeout kills only the called process, never a VM it started.
- The bundled `msb` runs from a copy staged in the app-data root, so a running VM never locks the app's own files.

## Invariants

- Engine-specific flags are rendered inside the provider, never added to the shared create spec.
- Which provider runs a sandbox is derived from the engines on every lookup.
- Machine lifecycle never follows the container connection setting.
- No external binary is spawned outside the process layer.
- A secret value never appears in a command line or `mio.log`; exec secrets travel in a process environment or on stdin.
