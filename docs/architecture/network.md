---
type: architecture
title: "Outbound Network Control"
description: "Per-sandbox traffic viewer, secret swapping and egress policy, and how Podman and microsandbox each implement them."
generated:
  by: claude-code/sonnet-5-5
  at: 2026-11-19T18:00:00+11:00
stale_after: 2027-02-05T00:00:00+11:00
code:
  - src-tauri/src/proxy/
  - src-tauri/src/net_policy.rs
  - src-tauri/src/provider/msb_policy.rs
  - src-tauri/src/services/sandbox.rs
  - src-tauri/src/services/start_env.rs
  - src-tauri/resources/mitm_flowlog_addon.py
  - src/lib/proxy/
  - src/routes/projects/NetPolicyEditor.svelte
sources:
  - resource: docs/features/network-proxy/02_dev.md
  - resource: docs/features/proxy-traffic-viewer/02_dev.md
  - resource: docs/features/microsandbox-network-policy/02_dev.md
  - resource: docs/features/local-llm-reachability/02_dev.md
  - resource: docs/features/podman-egress-boundary/02_dev.md
  - resource: docs/features/proxy-credential-secrets/02_dev.md
---
# Outbound Network Control

A sandbox can opt into these controls over its outbound traffic, all switched on by the create dialog's proxy switch:

- **Traffic viewer**: a per-sandbox proxy sidecar records every HTTP(S) request (never headers, bodies or secret values) for the traffic panel.
- **Secret swapping**: the sandbox holds only a placeholder; the real credential replaces it outside the sandbox, only in requests to the binding's hosts.
- **Egress policy**: an allowlist, or open mode, for outbound destinations, stored in the workspace's `.mio/net-policy.json` with the secret bindings.
- **App-owned binding**: a binding the app renders from its own state, never from the workspace policy, so the user cannot edit or remove it. The GitHub login uses one: gh and mise hold a fixed placeholder, and the sidecar swaps in the host-held token on requests to GitHub's API, web, gist, upload and raw-content hosts.

Podman and microsandbox enforce one policy format with different engines.

## Capability Matrix

| Control | Podman | microsandbox |
|---|---|---|
| Network boundary | The sandbox sits on an internal network; an iron-proxy sidecar, also on Podman's default network, is the only exit | msb's own network stack |
| Traffic viewer | The sidecar's per-request audit log, copied into the app's flow record | A mitmproxy sidecar microVM that msb chains the sandbox's traffic to over SOCKS5 |
| Secret swapping | The sidecar swaps placeholders per binding | msb swaps placeholders per binding |
| Egress policy | Enforced by the sidecar: host allows and CIDR denies only | Enforced by msb, with ports, groups and violation actions |
| Non-HTTP traffic | Blocked | Governed by the policy |
| Changing the policy after create | Applied live by reloading the sidecar | Recreate the VM around its preserved workspace volume |
| Remote deploy | Refused | Not applicable |

A Podman sidecar created before the boundary existed records traffic but has no boundary or policy until the sandbox is recreated.

## Where State Lives

- **Workspace** (`.mio/`, visible inside the sandbox): the egress policy and secret bindings, which reference secrets by id only.
- **App data** (never mounted into the sandbox): secret values, each sidecar's CA key, config and secret files, the snapshot of the policy last applied, and the flow record.
- Whether a sandbox has a proxy is derived from the sidecar's existence.

## Lifecycle

- **Create**: the sidecar is created first and denies everything until the workspace policy is applied. Any later failure removes it.
- **Policy change**: Podman reloads the sidecar, restoring the previous config if the reload is rejected; microsandbox recreates the VM. The UI shows drift between the workspace file and the last applied snapshot.
- **Traffic record**: while the traffic panel watches a Podman sandbox, new audit lines are copied into the flow record, so history survives log truncation.
- **Destroy**: the sandbox, then the sidecar, then its network and state directory.
- **Startup**: sidecars whose sandbox no longer exists are swept.

## Invariants

- No secret value is ever written into the workspace, passed as a process argument, logged, or returned over IPC.
- A Podman sandbox behind a sidecar has no route anywhere but the sidecar; a stopped sidecar leaves it with no network at all.
- The sidecar refuses upstream connections to private, loopback, link-local and metadata addresses, whatever hostname resolved to them.
- The sidecar's management and metrics listeners are reachable only from inside the sidecar.
- Local-model traffic reaches the host over the sandbox's own loopback forward, so neither proxy nor egress policy sees it.
- The GitHub credential is never resolved from a workspace binding.
- A sidecar is recognised by its image, never by its `-proxy` name suffix.
- The msb store is shared by every worktree on the host, so a microsandbox sidecar is swept only by the app-data root that created it.
- The flow-record line cap is one value shared by ingest, poller and frontend store.
