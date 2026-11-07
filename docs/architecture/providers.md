---
type: architecture
title: "Providers, Machines and the Process Layer"
description: "The Podman and microsandbox engines behind one provider seam, how each is made ready on the host, and how every external process is run."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-05T23:55:00+11:00
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
---
# Providers, Machines and the Process Layer

A sandbox runs on one of two engines, Podman or microsandbox, behind one provider seam. This document covers what differs between the engines, how each is made ready on the host, and the process layer every external binary runs through.

## The Provider Seam

- A provider exposes an engine-neutral instance API: build or load the base image; create, start, stop, recreate and remove instances; run commands inside them; manage sidecars and networks.
- Create takes a provider-neutral spec (name, image, workspace, environment, network, optional egress policy). Each provider translates the spec into its own engine's flags, so engine detail never leaks above the seam.
- Each provider declares its capabilities as flags. The frontend hides controls a provider cannot honour, and the backend refuses them as a second gate.
- The engine is chosen per sandbox at create, defaulting to an app setting. Which engine runs a sandbox is never stored: the registry asks each engine in turn, Podman first. Sandbox names are unique across both engines.

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

- Machine lifecycle (WSL install, machine init and start, API socket activation, readiness polling) always targets the default Podman machine, never the connection that container calls use, because a connection may name a remote host that no machine command can act on.
- At startup the frontend asks whether Podman is ready. A stopped machine or missing socket surfaces as a recoverable error, and starting the machine runs as an activity.
- First run provisions WSL and the machine through the stepped setup flow shown in onboarding.

## microsandbox Readiness

- The bundled `msb` must meet a minimum version, the hypervisor platform must be enabled, and `msb doctor` must pass.
- The msb store is shared host-wide unless relocated, which the e2e harness does per worktree.
- microsandbox needs Podman once per base-image version, to build the image it loads.

## The Process Layer

- Every external binary is spawned with a windowless console of its own, and the app clears handle inheritance at startup. Without both, packaged builds flash a console window per call and terminal-launched dev builds can hang on inherited handles.
- One-shot calls go through a runner seam with real and scripted implementations; the scripted runners back the `light` test tier ([Testing](testing.md)). Streaming calls (interactive exec, event streams) bypass the seam and must check the mock switch themselves.
- Podman calls carry per-call-shape timeouts. A timeout trips a process-wide breaker: while the breaker is open, one probe call at a time is let through and every other call fails fast, until any answer closes it.

## Invariants

- Engine-specific flags are rendered inside the provider, never added to the shared create spec.
- Which provider runs a sandbox is derived from the engines on every lookup.
- Machine lifecycle never follows the container connection setting.
- No external binary is spawned outside the process layer.
- A secret value never appears in a command line or `mio.log`: exec secrets reach Podman through the client's environment, and microsandbox, whose exec flag takes only a literal value, through the guest's stdin.
