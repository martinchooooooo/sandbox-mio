---
type: feature
title: "e2e-tiering"
description: "Split the e2e suite into light (mocked), standard (real Podman) and heavy (large assets) tiers."
status: implemented
architecture: [docs/architecture/testing.md]
---

# e2e-tiering

Split the e2e suite into light (mocked), standard (real Podman) and heavy (large assets) tiers.

## Status note

Moved here from `docs/e2e-refactor.md` on 2026-11-05. The plan's tier taxonomy and harness wiring shipped in `b3302bf` (2026-07-01). Its hand-written mock design (`Mock{Backend,Runtime,Sync}`, `MockMachineManager`) was later superseded by the scripted process layer of scriptable-mock-backend. Its decision rule 4 ("prefer the lowest tier") was reversed: `standard` is now the default tier (`e2e/AGENTS.md`).
