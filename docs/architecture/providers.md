---
type: architecture
title: "Providers, Machines and the Process Layer"
description: "The Podman engine behind the provider seam, how it is made ready on the host, how a provider is added, and how every external process is run."
generated:
  by: claude-code/sonnet-5-5
  at: 2026-11-21T20:00:00+11:00
stale_after: 2027-02-21T00:00:00+11:00
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
  - resource: docs/features/podman-egress-boundary/02_dev.md
  - resource: docs/features/remove-microsandbox/02_dev.md
---
# Providers, Machines and the Process Layer

A sandbox runs on Podman, the only provider, behind a provider seam that keeps engine specifics out of the rest of the app so a second provider is a small addition.

## The Provider Seam

- A provider exposes an engine-neutral instance API: build or load the base image; create, start, stop, recreate and remove instances; run commands inside them; manage sidecars and networks.
- Create takes a provider-neutral spec (name, image, workspace, environment, network, optional egress policy) that each provider translates into its own engine's flags.
- Each provider declares its capabilities as flags. The frontend hides controls a provider cannot honour, and the backend refuses them as a second gate.
- The engine is chosen per sandbox at create and is never stored: the registry asks each engine in turn. Sandbox names are unique across engines.

## Podman Capabilities

| Capability | Podman |
|---|---|
| Runs as | Containers in a WSL-backed Podman machine, through a bundled Podman client |
| Base image | Built locally from the repository's Containerfile; never pulled |
| Workspace | Host folder bind-mounted into the container |
| Recreate, keeping the workspace | Not supported yet |
| Remote deploy | Supported ([Remote](remote.md)) |
| Engine-enforced egress policy | No; a proxy sidecar enforces it ([Network](network.md)) |
| Engine event stream | Yes |

## Adding a Provider

Add a provider identifier, a registry field and one arm in each registry lookup, a provider implementation of the engine-neutral API, a process runner with a scripted counterpart, capability values, and a machine manager for readiness. One-arm lookups stay one-arm matches so the next provider extends them.

## Podman Machine Readiness

- Machine lifecycle (WSL install, machine init and start, socket activation, readiness polling) always targets the default Podman machine, never the container connection, which may name a remote host no machine command can act on.
- At startup a stopped machine or missing socket surfaces as a recoverable error; starting the machine runs as an activity.
- Onboarding's stepped setup and the projects page's recovery run one guarded bring-up: it never starts a running machine and never shuts WSL down while another machine runs.

## The Process Layer

- Every external binary gets a windowless console of its own, and the app clears handle inheritance at startup; otherwise packaged builds flash a window per call and terminal-launched dev builds can hang.
- One-shot calls go through a runner seam whose scripted implementations back the `light` test tier ([Testing](testing.md)). Streaming calls (interactive exec, event streams) bypass the seam and must check the mock switch themselves.
- Only one container event stream runs at a time: a new subscription, such as a frontend reload's, ends the previous one and its Podman process, since the old stream cannot tell its page is gone.
- Podman calls carry per-call-shape timeouts. A timeout trips a process-wide breaker: while it is open, one probe call at a time goes through and the rest fail fast until an answer closes it.
- A Podman call's stdin reaches the child through a pipe the app writes and closes. Stdout and stderr are captured through temporary files, because a pipe held open by a grandchild never reaches end-of-file.
- Mutagen one-shot calls are bounded too; a timeout kills only the called process.

## Invariants

- Engine-specific flags are rendered inside the provider, never added to the shared create spec.
- Which provider runs a sandbox is derived from the engines on every lookup.
- Machine lifecycle never follows the container connection setting.
- No external binary is spawned outside the process layer.
- A secret value never appears in a command line or `mio.log`; exec secrets travel in a process environment or on stdin.
