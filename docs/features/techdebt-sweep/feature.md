---
type: feature
title: "techdebt-sweep"
description: "Resolve the open tech debt found by the 2026-11-07 audit: msb build locks, duplicated Podman bring-up, the legacy relay path, secrets on disk and in logs, unbounded calls."
status: implemented
architecture: [docs/architecture/providers.md, docs/architecture/transport.md, docs/architecture/agents.md, docs/architecture/workspace-ui.md]
---

# techdebt-sweep

Resolve the open tech debt found by the 2026-11-07 audit: msb build locks, duplicated Podman bring-up, the legacy relay path, secrets on disk and in logs, unbounded calls.

## Status note

Implemented: Phases A–K. The `standard` tier verified every Phase K spec on 2026-11-10. Deferred work, including an intermittent e2e app launch that never loads the UI (pre-existing), is in `04_improvements.md`. The optional `heavy` `local-llm` run was not done.
