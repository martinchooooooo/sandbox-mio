---
type: feature
title: "remove-microsandbox"
description: "Remove the microsandbox provider and make Podman the only provider, keeping the provider, capability, process and net-policy seams so a future provider is a small addition."
status: implemented
depends_on: [podman-egress-boundary]
architecture: [docs/architecture/providers.md, docs/architecture/network.md, docs/architecture/transport.md, docs/architecture/overview.md, docs/architecture/inference.md, docs/architecture/remote.md, docs/architecture/testing.md]
---

# remove-microsandbox

Remove the microsandbox provider and make Podman the only provider, keeping the provider, capability, process and net-policy seams so a future provider is a small addition.

## Status note

Implemented (`02_dev.md`, Phases A to G, I and J; Phase H dropped by user decision). The plans `microsandbox-provider` and `microsandbox-network-policy` are superseded by this feature. Two full `standard` runs on 2026-11-21 each failed a different set of three specs (`agent-web`, `embedded-workspace`, `expose-port`, then `agent-setup`, `agent-web-plugins`, `expose-port`); `expose-port`, the only repeat, passes alone, so the failures are recorded as load flakiness, not regressions.
