---
type: feature
title: "podman-egress-boundary"
description: "Fail-closed egress for Podman sandboxes: an internal Podman network with the proxy sidecar as the only exit, and iron-proxy replacing the mitmproxy sidecar for policy, secret swap and the traffic record."
status: implemented
depends_on: [network-proxy, proxy-traffic-viewer, microsandbox-network-policy]
architecture: [docs/architecture/network.md, docs/architecture/providers.md]
---

# podman-egress-boundary

Fail-closed egress for Podman sandboxes: an internal Podman network with the proxy sidecar as the only exit, and iron-proxy replacing the mitmproxy sidecar for policy, secret swap and the traffic record.

## Status note

Build plan written; Phase A (gating spikes) ran on 2026-11-13 and the boundary works (`02_dev.md`, Spike Results; evidence in `spike/README.md`). The Resolved Decisions were amended with the spike findings on 2026-11-14, and the user kept the hard cut with placeholder-swap bindings only. Phase B (provider seam) is implemented: internal networks, the dual-homed sidecar and log-driver flags, `--dns`, `sidecar_logs`, and the `sidecar_egress` capability, with no behaviour change for existing sandboxes. Phase C (policy dialect and config renderer) is implemented: the sidecar dialect in `net_policy::validate`, the capability-aware starter policy, the placeholder format, and the pure iron-proxy config renderer pinned against the config captured in Phase A. Phase D (iron-proxy sidecar lifecycle) is implemented: a Podman sandbox created with the proxy on gets the internal network, the dual-homed iron-proxy sidecar, an in-process CA, the rendered policy with secret files, and the `apply_sandbox_policy` command; it was checked live against iron-proxy 0.56.0. Phase E (traffic record from the audit stream) is implemented: the sidecar's audit lines are translated into the existing traffic rows and appended to `flows.ndjson`, with rejected and would-block rows marked in the traffic panel; it was checked live. Phase F (hard cut and frontend) is implemented: the mitmproxy injection rules and their commands are deleted, `ProxyStatus` reports the sidecar engine, and the proxy panel, policy editor and create dialog speak the sidecar dialect, with live apply and an export hint for bindings added after create. The Paraglide pass is pending user approval of behaviour and wording: new strings are hard-coded English marked `TODO(i18n)`. The end-to-end plan (`03_e2e.md`) is written: a `standard` spec proving the boundary, live apply, secret swap and teardown on `mio-tests`, plus a `light` companion; neither spec is written yet, and the `mio-tests` default-network check is still open. `proxy-network-policy` is superseded by this feature (`02_dev.md`, Resolved Decision 1).
