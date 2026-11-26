---
type: feature
title: "xtask-hardening"
description: "Harden the xtask build tool: strict CLI arguments, a correct staged-version manifest, SHA-256-pinned binary downloads, and yaml-rust2 and pulldown-cmark in place of the hand-written frontmatter and Markdown parsers."
status: implemented
architecture: [docs/architecture/testing.md]
depends_on: [codebase-restructure]
---

# xtask-hardening

Harden the xtask build tool: strict CLI arguments, a correct staged-version manifest, SHA-256-pinned binary downloads, and yaml-rust2 and pulldown-cmark in place of the hand-written frontmatter and Markdown parsers.

Status note: all phases of [02_dev.md](02_dev.md) are implemented and reviewed; the overall picture in `docs/architecture/testing.md` is unchanged. The `xtask` package carries no unit tests, by user instruction.
