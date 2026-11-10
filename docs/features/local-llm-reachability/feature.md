---
type: feature
title: "local-llm-reachability"
description: "Make the host inference server reachable from every sandbox, microsandbox included, through a per-sandbox loopback forward, and keep every local-model agent's provisioned config current."
status: implemented
architecture: [docs/architecture/inference.md, docs/architecture/transport.md, docs/architecture/agents.md, docs/architecture/network.md]
depends_on: [local-llm, microsandbox-provider, microsandbox-network-policy, pi-agent]
---

# local-llm-reachability

Make the host inference server reachable from every sandbox, microsandbox included, through a per-sandbox loopback forward, and keep every local-model agent's provisioned config current.

## Status note

Implemented on 2026-11-10: Phases A to E are done, the architecture documents are updated, and the Paraglide pass is done (`02_dev.md`). `./build e2e light` and `./build e2e` pass; `03_e2e.md` is not yet written. The per-sandbox guest-loopback forward was proven end to end with pi in a proxied microsandbox sandbox on 2026-11-10 (`spike/01_mutagen-reverse-msb.md`).
