---
type: feature
title: "remove-mitmproxy"
description: "Remove the legacy pre-boundary mitmproxy sidecar path, keeping ProxyEngine with one variant as the seam for another proxy engine."
status: implemented
depends_on: [podman-egress-boundary, remove-microsandbox]
architecture: [docs/architecture/network.md]
---

# remove-mitmproxy

Remove the legacy pre-boundary mitmproxy sidecar path, keeping ProxyEngine with one variant as the seam for another proxy engine.

## Status note

Implemented (`02_dev.md`). The compatibility path for a pre-boundary mitmproxy sidecar was removed. `ProxyEngine` stays with one variant, `IronProxy`, as the seam for another engine. A sidecar whose `meta.json` has no `engine` is treated as iron-proxy. Nothing cleans up old sidecars.
