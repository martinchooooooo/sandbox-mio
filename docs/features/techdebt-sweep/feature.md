---
type: feature
title: "techdebt-sweep"
description: "Resolve the open tech debt found by the 2026-11-07 audit: msb build locks, duplicated Podman bring-up, the legacy relay path, secrets on disk and in logs, unbounded calls."
status: planned
architecture: [docs/architecture/providers.md, docs/architecture/transport.md, docs/architecture/agents.md, docs/architecture/workspace-ui.md]
---

# techdebt-sweep

Resolve the open tech debt found by the 2026-11-07 audit: msb build locks, duplicated Podman bring-up, the legacy relay path, secrets on disk and in logs, unbounded calls.

## Status note

No phase implemented yet. Phases A–J are independent and run in parallel worktrees; Phase K runs after they merge.
