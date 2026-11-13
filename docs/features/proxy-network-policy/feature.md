---
type: feature
title: "proxy-network-policy"
description: "Domain allow/deny and interactive approval for Podman sandboxes through the mitmproxy sidecar."
status: superseded
superseded_by: podman-egress-boundary
architecture: [docs/architecture/network.md]
depends_on: [network-proxy, microsandbox-network-policy]
---

# proxy-network-policy

Domain allow/deny and interactive approval for Podman sandboxes through the mitmproxy sidecar.

## Status note

Superseded by `podman-egress-boundary`: iron-proxy behind an internal Podman network replaces the hand-written mitmproxy policy addon, and interactive approval is deferred (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decisions 1–2). Spikes A1/A2 here remain valid history.
