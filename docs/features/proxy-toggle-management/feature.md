---
type: feature
title: "proxy-toggle-management"
description: "App-global proxy default and turning the proxy on or off for an existing sandbox by recreating it."
status: abandoned
architecture: [docs/architecture/network.md, docs/architecture/providers.md]
depends_on: [podman-egress-boundary]
---

# proxy-toggle-management

App-global proxy default and turning the proxy on or off for an existing sandbox by recreating it.

## Status note

Abandoned: every sandbox now always has an iron-proxy sidecar, so there is no proxy to turn on or off (`docs/features/proxy-remote-deploy/feature.md`). The note below is the plan's last state before that.

Phase A (the `default_proxy` setting) is still valid as written. Phases B and C must be refreshed with `feature-dev` before implementation. The provider-level recreate exists for Podman only (`PodmanProvider::recreate_instance`, `src-tauri/src/provider/mod.rs`); `CliBackend::recreate` still refuses and points here (`src-tauri/src/backends/mod.rs`). The microsandbox-only `reapply_sandbox_network` command and its task kind are gone (`docs/features/remove-microsandbox/02_dev.md`, Resolved Decision 2), so this feature adds its own command and task kind. A Podman recreate must now build a bounded sandbox: attach the container to the internal `mio-proxy-<sandbox>` network or back to the default network, create or remove the iron-proxy sidecar and its in-process CA, render and apply the iron config, and set the placeholder variable for each secret binding. The plan's `create_proxy` and mitmproxy steps no longer describe that. The feature has also gained two uses from `podman-egress-boundary`: it is the upgrade path for Podman sidecars created before the boundary existed, which have no boundary until recreated, and it gives a secret binding added after create its placeholder without a manual export (`docs/architecture/network.md`, Capability Matrix).
