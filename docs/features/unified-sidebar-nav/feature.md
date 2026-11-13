---
type: feature
title: "unified-sidebar-nav"
description: "One persistent left sidebar replaces the top page tabs and the multi-sandbox switcher bar, pages use the full content area, and navigation labels speak plain language."
status: in-progress
depends_on: [consolidate-app-container, manage-dashboard-redesign]
architecture: [docs/architecture/workspace-ui.md]
---

# unified-sidebar-nav

One persistent left sidebar replaces the top page tabs and the multi-sandbox switcher bar, pages use the full content area, and navigation labels speak plain language.

## Status note

Phases A to I of `02_dev.md` are implemented; `pnpm check`, `./build e2e light` and `./build e2e` (`standard`) pass against the new shell, while the `heavy` tier has not been run. The feature stays `in-progress` until the user has reviewed the new shell.
