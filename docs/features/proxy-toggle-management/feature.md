---
type: feature
title: "proxy-toggle-management"
description: "App-global proxy default and turning the proxy on or off for an existing sandbox by recreating it."
status: planned
architecture: [docs/architecture/network.md, docs/architecture/providers.md]
depends_on: [network-proxy]
---

# proxy-toggle-management

App-global proxy default and turning the proxy on or off for an existing sandbox by recreating it.

## Status note

SandboxBackend::recreate now exists for microsandbox (microsandbox-network-policy Phase G); Phase B should build on it.
