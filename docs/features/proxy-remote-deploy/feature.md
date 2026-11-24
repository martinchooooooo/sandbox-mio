---
type: feature
title: "proxy-remote-deploy"
description: "Deploy a bounded sandbox to a remote Podman host by recreating its internal network, iron-proxy sidecar, secret files, policy reload and audit polling there; a deployed sandbox keeps its egress boundary on the target."
status: implemented
architecture: [docs/architecture/network.md, docs/architecture/remote.md]
depends_on: [podman-egress-boundary, remote-sync]
---

# proxy-remote-deploy

Deploy a bounded sandbox to a remote Podman host instead of refusing it.

## Status note

Implemented on 2026-11-24. Deploy recreates the internal network, dual-homed iron-proxy sidecar and RAM-only secret files on the target, applies policy changes live, retargets the traffic panel, and starts the sidecar and then the container on the active site when the sandbox starts; the earlier mitmproxy plans are in git history only. Every sandbox is assumed to have a sidecar: a sidecar-less sandbox is refused at deploy, and deploy refuses an already-deployed sandbox and a target holding a container of the same name. The `standard` remote specs (`deploy-remote`, `base-image-remote`, `runtime-management`) are un-skipped and pass on `mio-tests-remote`. Deferred, recorded in `04_improvements.md`: a run on a real non-WSL Linux host (Phase A check 4, so the Podman 4.0 floor is unmeasured), a cross-connection sweep, air-gapped targets, and a strict local start for a sidecar-less sandbox. The new UI strings still carry `TODO(i18n)` until the wording is approved.
