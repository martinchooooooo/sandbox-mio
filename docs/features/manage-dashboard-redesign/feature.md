---
type: feature
title: "manage-dashboard-redesign"
description: "Make a sandbox's Manage dashboard open instantly and read simply: an Overview for launching and agents, advanced controls in a section sidebar that loads on demand."
status: implemented
depends_on: [workspace-app-tabs, consolidate-app-container]
architecture: [docs/architecture/workspace-ui.md]
---

# manage-dashboard-redesign

Make a sandbox's Manage dashboard open instantly and read simply: an Overview for launching and agents, advanced controls in a section sidebar that loads on demand.

## Status note

Phases B to E are implemented: the user approved the working dashboard on 2026-11-09, every user-facing string is translated, and `docs/architecture/workspace-ui.md` describes the Overview plus lazily mounted sections. Phase A (baseline timing against a real Podman sandbox) was not run and is deferred; it does not block the feature (`02_dev.md`, Resolved Decision 13). No `03_e2e.md` exists yet, so the new behaviour (section memory, lazy mounting, summary links) has no dedicated specs beyond the adapted existing ones.
