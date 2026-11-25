---
type: architecture
title: "Outbound Network Control"
description: "Per-sandbox traffic viewer, secret swapping and egress policy, enforced by an iron-proxy sidecar on Podman."
generated:
  by: claude-code/sonnet-5-5
  at: 2026-11-24T22:00:00+11:00
stale_after: 2027-02-21T00:00:00+11:00
code:
  - src-tauri/src/proxy/
  - src-tauri/src/net_policy.rs
  - src-tauri/src/services/sandbox.rs
  - src/lib/proxy/
  - src/routes/projects/NetPolicyEditor.svelte
  - src/routes/projects/NetPolicyProposalDialog.svelte
sources:
  - resource: docs/features/network-proxy/02_dev.md
  - resource: docs/features/proxy-traffic-viewer/02_dev.md
  - resource: docs/features/microsandbox-network-policy/02_dev.md
  - resource: docs/features/local-llm-reachability/02_dev.md
  - resource: docs/features/podman-egress-boundary/02_dev.md
  - resource: docs/features/proxy-credential-secrets/02_dev.md
  - resource: docs/features/remove-microsandbox/02_dev.md
  - resource: docs/features/proxy-remote-deploy/02_dev.md
  - resource: docs/features/net-policy-review/02_dev.md
---
# Outbound Network Control

Every new sandbox gets a proxy sidecar and these controls over its outbound traffic; there is no option to create one without:

- **Traffic viewer**: a per-sandbox proxy sidecar records every HTTP(S) request (never headers, bodies or secret values) for the traffic panel.
- **Secret swapping**: the sandbox holds only a placeholder; the real credential replaces it outside the sandbox, only in requests to the binding's hosts.
- **Egress policy**: an allowlist, or open mode, for outbound destinations, with the secret bindings. The app keeps the accepted policy; the workspace's `.mio/net-policy.json` is a mirror and only ever a proposal.
- **App-owned binding**: a binding the app renders from its own state, never from the workspace policy, so the user cannot edit or remove it. The GitHub login uses one: gh and mise hold a fixed placeholder, and the sidecar swaps in the host-held token on requests to GitHub's API, web, gist, upload and raw-content hosts.

One policy format is shared across providers; Podman enforces it with a sidecar. A provider whose engine enforces policy itself declares the engine-enforced capability instead, and the sidecar dialect then does not apply.

## Podman Enforcement

| Control | Behaviour |
|---|---|
| Network boundary | The sandbox sits on an internal network; an iron-proxy sidecar, also on Podman's default network, is the only exit |
| Traffic viewer | The sidecar's per-request audit log, copied into the app's flow record |
| Secret swapping | The sidecar swaps placeholders per binding |
| Egress policy | Host allows and CIDR denies only; ports, address groups, host denies and UDP are refused |
| Non-HTTP traffic | Blocked |
| Changing the policy after create | Applied live by reloading the sidecar |
| Remote deploy | The network, sidecar and secrets are recreated on the target; the local sidecar stays stopped meanwhile |

## Where State Lives

- **Workspace** (`.mio/`, writable inside the sandbox): a copy of the policy and bindings (secrets by id only). Nothing enforces it; a differing copy is a proposal.
- **App data** (never mounted into the sandbox): the accepted policy with its revision, each sandbox's own secret values, each sidecar's CA key, config and secret files, and the flow record.
- **Remote site** (deployed sandboxes): the target holds its own sidecar and network. Config and CA are copied in; secret values sit in a memory-only directory pushed from the laptop; the laptop's staged copy is the source of truth.
- A sandbox has a proxy exactly when its sidecar exists; only auth containers and pre-sidecar sandboxes lack one.

## Lifecycle

- **Create**: the sidecar is created first and denies everything until the workspace policy is applied. Any later failure removes it.
- **Policy change**: an edit or reviewed proposal becomes the accepted policy and the sidecar reloads, restoring the previous config if rejected; the workspace file is then rewritten to match. A stale-revision apply is refused.
- **Proposal**: a differing workspace file shows as a banner and a diff dialog listing risky changes first; editing waits until it is applied or discarded. Deploy and background renders use the accepted policy.
- **Traffic record**: while the traffic panel watches a sandbox, new audit lines are copied into the flow record, surviving log truncation.
- **Deploy and undeploy**: the traffic record continues across sites, with one read cursor per site.
- **Destroy**: the sandbox, then the sidecar, then its network and state directory.
- **Startup**: orphaned sidecars are swept.

## Invariants

- No apply enforces a policy or binding the user has not seen; the workspace file is never an apply input.
- A secret id resolves only within its own sandbox.
- No secret value is ever written into the workspace, passed as a process argument, logged, or returned over IPC.
- A sandbox behind a sidecar has no route anywhere but the sidecar; a stopped sidecar leaves it with no network at all.
- The sidecar refuses upstream connections to private, loopback, link-local and metadata addresses, whatever hostname resolved to them.
- The sidecar's management and metrics listeners are reachable only from inside the sidecar.
- Local-model traffic reaches the host over the sandbox's own loopback forward, so neither proxy nor egress policy sees it.
- The GitHub credential is never resolved from a workspace binding.
- A sidecar is recognised by its image, never by its `-proxy` name suffix.
- The flow-record line cap is one value shared by ingest, poller and frontend store.
