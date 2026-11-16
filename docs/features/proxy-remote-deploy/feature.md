---
type: feature
title: "proxy-remote-deploy"
description: "Let a proxy-enabled sandbox be deployed to a remote host instead of being refused."
status: idea
architecture: [docs/architecture/network.md, docs/architecture/remote.md]
depends_on: [podman-egress-boundary, remote-sync]
---

# proxy-remote-deploy

Let a proxy-enabled sandbox be deployed to a remote host instead of being refused.

## Status note

Returned to `idea` on 2026-11-17. `02_dev.md` and `01_ideas.md` describe the mitmproxy sidecar (`addon.py`, a materialised `rules.json`, `apply_rules`, `restart_proxy_container`, `MIO_PROXY_IMAGE`), which `podman-egress-boundary` removed from Podman. Both are kept only as history; do not implement from them. A proxied Podman sandbox is now a bounded sandbox (`docs/architecture/network.md`), so a remote deploy must recreate the internal network, the dual-homed iron-proxy sidecar, the per-binding secret files, the policy reload and the audit-log polling on the target host. The gating spike changes with it: the boundary was proven only on the local WSL podman machine, never on a remote rootless host. The routing parts of the old plan (`SandboxRouter::locate`, a `SandboxBackend::connection_info` seam, a remote state directory under `<home>/mio-proxy/<sandbox>/`) may still carry over. Restart with `feature-ideas`, then `feature-dev`. Until then, deploying a proxied sandbox stays refused (`BackendError::ProxyDeployUnsupported`).
