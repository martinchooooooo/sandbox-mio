---
type: feature
title: "proxy-non-http-capture"
description: "Surface connections that bypass the proxy (non-HTTP(S) traffic) in the proxy panel."
status: superseded
superseded_by: podman-egress-boundary
architecture: [docs/architecture/network.md]
depends_on: [network-proxy]
---

# proxy-non-http-capture

Surface connections that bypass the proxy (non-HTTP(S) traffic) in the proxy panel.

## Status note

Superseded by `podman-egress-boundary` on 2026-11-17, before any phase was built. Every proxied Podman sandbox now sits on an internal network whose only exit is the iron-proxy sidecar, so a connection can no longer bypass the proxy: it fails instead of leaving (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 20). Requests the sidecar rejects already appear in the traffic panel. On microsandbox, non-HTTP traffic is governed by msb's own egress policy. The `/proc/net/tcp` bypass count planned here has nothing left to count. Showing the non-HTTP attempts the boundary now drops is recorded as "Non-HTTP visibility with an eBPF sensor" in `docs/features/podman-egress-boundary/04_improvements.md`.
