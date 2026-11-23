---
type: feature
title: "workspace-ux"
description: "Make the in-sandbox workspace friendlier for non-technical users by making the left app sidebar the only app chrome, so hosted apps feel like the app itself."
status: in-progress
depends_on: [manage-dashboard-redesign, unified-sidebar-nav, workspace-app-tabs]
architecture: [docs/architecture/workspace-ui.md]
---

# workspace-ux

Make the in-sandbox workspace friendlier for non-technical users by making the left app sidebar the only app chrome, so hosted apps feel like the app itself.

## Status note

All five phases (Rust layout and folder command; sidebar tree, dashboard title bar, horizontal section tabs and i18n; e2e page objects and light coverage; docs; review revisions) are implemented, awaiting user approval of the working feature. `docs/architecture/workspace-ui.md` already describes the new layout. Resolved Decision 6 in `02_dev.md` supersedes Resolved Decision 10 of `docs/features/manage-dashboard-redesign/02_dev.md` (the 48rem icon-collapse breakpoint).
