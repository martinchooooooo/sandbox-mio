---
type: feature
title: "sandbox-runtime-management"
description: "Record installed software with mise and reproduce it on a remote host at deploy."
status: implemented
architecture: [docs/architecture/providers.md, docs/architecture/remote.md]
---

# sandbox-runtime-management

Record installed software with mise and reproduce it on a remote host at deploy.

## Status note

Phases A-D implemented. The deploy verification passed on 2026-11-10: `runtime-management.spec.ts` (asserting the mise flow) and `deploy-remote.spec.ts`, both in the `standard` tier.
