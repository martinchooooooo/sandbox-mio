---
type: feature
title: "proxy-toggle-management"
description: "App-global proxy default and turning the proxy on or off for an existing sandbox by recreating it."
status: planned
architecture: [docs/architecture/network.md, docs/architecture/providers.md]
depends_on: [podman-egress-boundary, microsandbox-network-policy]
---

# proxy-toggle-management

App-global proxy default and turning the proxy on or off for an existing sandbox by recreating it.

## Status note

Phase A (the `default_proxy` setting) is still valid as written. Phases B and C must be refreshed with `feature-dev` before implementation. `SandboxBackend::recreate` now exists, but only for microsandbox (microsandbox-network-policy Phase G); `CliBackend::recreate` refuses Podman and points here (`src-tauri/src/backends/mod.rs`). A Podman recreate must now build a bounded sandbox: attach the container to the internal `mio-proxy-<sandbox>` network or back to the default network, create or remove the iron-proxy sidecar and its in-process CA, render and apply the iron config, and set the placeholder variable for each secret binding. The plan's `create_proxy` and mitmproxy steps no longer describe that. The feature has also gained two uses from `podman-egress-boundary`: it is the upgrade path for Podman sidecars created before the boundary existed, which have no boundary until recreated, and it gives a secret binding added after create its placeholder without a manual export (`docs/architecture/network.md`, Capability Matrix).
