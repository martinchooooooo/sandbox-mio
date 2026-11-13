---
type: feature
title: "podman-egress-boundary"
description: "Fail-closed egress for Podman sandboxes: an internal Podman network with the proxy sidecar as the only exit, and iron-proxy replacing the mitmproxy sidecar for policy, secret swap and the traffic record."
status: in-progress
depends_on: [network-proxy, proxy-traffic-viewer, microsandbox-network-policy]
architecture: [docs/architecture/network.md, docs/architecture/providers.md]
---

# podman-egress-boundary

Fail-closed egress for Podman sandboxes: an internal Podman network with the proxy sidecar as the only exit, and iron-proxy replacing the mitmproxy sidecar for policy, secret swap and the traffic record.

## Status note

Build plan written; Phase A (gating spikes) ran on 2026-11-13 and the boundary works (`02_dev.md`, Spike Results; evidence in `spike/README.md`). The Resolved Decisions were amended with the spike findings on 2026-11-14, and the user kept the hard cut with placeholder-swap bindings only. Phase B (provider seam) is implemented: internal networks, the dual-homed sidecar and log-driver flags, `--dns`, `sidecar_logs`, and the `sidecar_egress` capability, with no behaviour change for existing sandboxes. Phase C (policy dialect and config renderer) is implemented: the sidecar dialect in `net_policy::validate`, the capability-aware starter policy, the placeholder format, and the pure iron-proxy config renderer pinned against the config captured in Phase A. Phase D (iron-proxy sidecar lifecycle) is next. `proxy-network-policy` is superseded by this feature (`02_dev.md`, Resolved Decision 1).
