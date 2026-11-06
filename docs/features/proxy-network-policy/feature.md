---
type: feature
title: "proxy-network-policy"
description: "Domain allow/deny and interactive approval for Podman sandboxes through the mitmproxy sidecar."
status: planned
architecture: [docs/architecture/network.md]
depends_on: [network-proxy, microsandbox-network-policy]
---

# proxy-network-policy

Domain allow/deny and interactive approval for Podman sandboxes through the mitmproxy sidecar.

## Status note

Gating spikes A1/A2 done. 02_dev.md predates the shared .mio/net-policy.json from microsandbox-network-policy and needs a refresh.
