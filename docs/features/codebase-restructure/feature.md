---
type: feature
title: "codebase-restructure"
description: "Behaviour-preserving restructure: break backend import cycles, move dashboard panels into src/lib, add a typed IPC layer, split oversized backend files, and turn the build tool into a Cargo crate."
status: in-progress
architecture: [docs/architecture/overview.md]
---

# codebase-restructure

Behaviour-preserving restructure: break backend import cycles, move dashboard panels into src/lib, add a typed IPC layer, split oversized backend files, and turn the build tool into a Cargo crate.

Status note: Phases 1 and 2 of [02_dev.md](02_dev.md) are implemented (commits `8849b28`, `60d6b0f`). Phase 3 (typed IPC layer) is implemented. Phase 4 (oversized backend files split, `ProxyStore`, `resources/` subsystem) is implemented. Phase 5 (the build tool as the `xtask` Cargo package, run as `cargo xtask`) is implemented. Phase 6 waits on a user decision.
