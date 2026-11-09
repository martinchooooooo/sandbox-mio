---
type: feature
title: "manage-dashboard-redesign"
description: "Make a sandbox's Manage dashboard open instantly and read simply: an Overview for launching and agents, advanced controls in a section sidebar that loads on demand."
status: in-progress
depends_on: [workspace-app-tabs, consolidate-app-container]
architecture: [docs/architecture/workspace-ui.md]
---

# manage-dashboard-redesign

Make a sandbox's Manage dashboard open instantly and read simply: an Overview for launching and agents, advanced controls in a section sidebar that loads on demand.

## Status note

Phases B to D are implemented and verified with `pnpm check` and the light e2e tier; the dashboard awaits the user's review of behaviour and wording. Phase E (Paraglide i18n pass and architecture-document close-out) is held until the user approves the working feature, so every new user-facing string is a hard-coded English literal marked `TODO(i18n)`. Phase A (baseline timing against a real Podman sandbox) was not run; it does not block the feature (`02_dev.md`, Resolved Decision 13).
