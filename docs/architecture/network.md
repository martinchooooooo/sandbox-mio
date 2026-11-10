---
type: architecture
title: "Outbound Network Control"
description: "Per-sandbox traffic viewer, secret injection and egress policy, and how Podman and microsandbox each implement them."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-10T18:00:00+11:00
stale_after: 2027-02-05T00:00:00+11:00
code:
  - src-tauri/src/proxy/
  - src-tauri/src/net_policy.rs
  - src-tauri/src/provider/msb_policy.rs
  - src-tauri/src/services/sandbox.rs
  - src-tauri/src/services/start_env.rs
  - src-tauri/resources/mitm_addon.py
  - src-tauri/resources/mitm_flowlog_addon.py
  - src/lib/proxy/
sources:
  - resource: docs/features/network-proxy/02_dev.md
  - resource: docs/features/proxy-traffic-viewer/02_dev.md
  - resource: docs/features/microsandbox-network-policy/02_dev.md
  - resource: docs/features/local-llm-reachability/02_dev.md
---
# Outbound Network Control

A sandbox can opt into three independent controls over its outbound traffic:

- **Traffic viewer**: a per-sandbox mitmproxy sidecar records every HTTP(S) flow (method, host, path, status, timing, sizes; never headers, bodies or secret values), and the sandbox's traffic panel tails that record.
- **Secret injection**: real credentials are added to matching requests outside the sandbox, so the agent inside never holds them.
- **Egress policy**: an allowlist, or open mode, for outbound destinations, stored in the workspace's `.mio/net-policy.json`.

Podman and microsandbox implement each control differently.

## Capability Matrix

| Control | Podman | microsandbox |
|---|---|---|
| Traffic viewer | Sidecar container on a private bridge network; the sandbox uses it as its HTTP proxy and trusts its CA | Sidecar microVM; msb chains the sandbox's traffic to it over SOCKS5 |
| Secret injection | The mitmproxy addon adds headers per injection rule | msb substitutes placeholders per secret binding |
| Egress policy | Not implemented | Enforced by msb's own network stack |
| Changing settings after create | Injection rules apply live through a sidecar restart; toggling the viewer is not implemented | Recreate the VM around its preserved workspace volume |
| Remote deploy | Refused | Not applicable |

## Where State Lives

- **Workspace** (`.mio/`, visible inside the sandbox): the egress policy and the injection rules, which reference secrets by id only.
- **App data** (never mounted into the sandbox): secret values, each sidecar's CA private key, materialised injection rules, and the flow record.
- Whether a sandbox has a proxy is derived from the sidecar's existence, never stored.

## Lifecycle

- **Create**: the sidecar is created before the sandbox, so the sandbox can be pointed at the sidecar at create time; any later failure removes the sidecar.
- **Re-apply** (microsandbox only): msb fixes network rules at create, so a policy or viewer change recreates the VM. Saving the policy file changes nothing on its own; the UI shows drift between the file and the snapshot taken when the policy was last applied.
- **Destroy**: the sandbox, then the sidecar, then the sidecar's state directory, which holds the CA key.
- **Startup**: sidecars whose sandbox no longer exists are swept.

## Invariants

- No secret value is ever written into the workspace, passed as a process argument, logged, or returned over IPC.
- Local-model traffic never leaves the sandbox's network stack: it reaches the host over the sandbox's own loopback forward, so neither proxy nor egress policy sees it.
- A sidecar is recognised by its image, never by its `-proxy` name suffix.
- The msb store is shared by every worktree on the host, so a microsandbox sidecar is swept only by the app-data root that created it.
- The flow-record line cap is one value shared by the addon, the backend poller and the frontend store.
- The sidecar streams large bodies through rather than buffering them, so a single large download cannot exhaust its memory.
- A stopped sidecar still means the viewer is on: the sandbox fails closed, and the captured history stays readable until the sidecar is removed.
