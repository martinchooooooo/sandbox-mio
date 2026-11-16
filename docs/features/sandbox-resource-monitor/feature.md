---
type: feature
title: "sandbox-resource-monitor"
description: "Sample each local sandbox's CPU and memory, show a discrete health indicator with a restart hint, and keep a local usage history that tells the maintainer whether sandboxes need more default memory."
status: in-progress
depends_on: [microsandbox-provider, podman-process-isolation]
architecture: [docs/architecture/providers.md, docs/architecture/workspace-ui.md]
---

# sandbox-resource-monitor

Sample each local sandbox's CPU and memory, show a discrete health indicator with a restart hint, and keep a local usage history that tells the maintainer whether sandboxes need more default memory.

Planning started directly at `02_dev.md`; there is no `01_ideas.md`. Phases A to E are implemented with hard-coded English strings awaiting approval; the Health pool rule waits on a decision (see `02_dev.md`, Phase D), and Phase F (translations, architecture, `03_e2e.md`) remains.
