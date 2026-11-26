---
type: feature
title: "codebase-restructure"
description: "Behaviour-preserving restructure: break backend import cycles, move dashboard panels into src/lib, add a typed IPC layer, split oversized backend files, and turn the build tool into a Cargo crate."
status: in-progress
architecture: [docs/architecture/overview.md]
---

# codebase-restructure

Behaviour-preserving restructure: break backend import cycles, move dashboard panels into src/lib, add a typed IPC layer, split oversized backend files, and turn the build tool into a Cargo crate.

Status note: Phases 1 and 2 of [02_dev.md](02_dev.md) are implemented (commits `8849b28`, `60d6b0f`). Phase 3 (typed IPC layer) is implemented. Phases 4 and 5 remain; Phase 5 needs a user decision on how `build.exe` is produced, and Phase 6 waits on the user. Resume at Phase 4.
