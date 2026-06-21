---
name: feature-dev
description: >
  Write the technical build plan (02_dev.md) for a feature under
  docs/features/<feature>/. Turns the ideation into a concrete implementation
  plan: resolves the open questions from 01_ideas.md, breaks the work into phases,
  tracks implemented-vs-remaining, cites a file path for every seam..
---

Write to `docs/features/<feature>/02_dev.md`. Read the feature's `01_ideas.md`
first; the build plan continues and supersedes it.

The steop after this is `03_e2e.md` (the test plan).

## Audience and base conventions

Written for future AI agents. Every sentence must stand alone. Follow the
`write-docs` skill conventions exactly:

## Purpose of the dev document

Be the build plan a future agent can execute or continue. It states the concrete
model (not the option space — that was `01_ideas.md`), records which design
decisions are now settled, names every seam by file path, and tracks what is built
versus what remains.

## Structure

1. **Overview paragraph** — what the document is the build plan for, which `01_ideas.md` assumptions it continues or supersedes, and a one-line status of implemented vs remaining work.
2. **The concrete model** — one `##` section describing the chosen architecture as it is being built, not as options.
3. **Resolved Decisions** — restate each Open Question from `01_ideas.md` and give its resolution. This is where the ideas document's open questions get answered.
4. **Implementation phases** — break the work into phases (A, B, C…). For each, a `## Implemented: Phase X` or `## Remaining: Phase X` section naming every function, struct, trait, command, and file touched, with paths. State explicitly which phases are done and which are not.
5. **Lifecycle / cross-cutting concerns** — daemon lifecycle, cleanup, state derivation, and similar concerns that span phases.

## Tracking implemented vs remaining

State the build status precisely and per-phase. Use section headings that encode
status ("Already Implemented", "Implemented", "Remaining work"). When a refactor
supersedes an earlier approach, record both the old approach and why it was
replaced, so a future agent does not reintroduce the removed code. Name the
`#[allow(dead_code)]` markers added and removed as phases wire up callers, when
relevant.

## Resolved Decisions

For every Open Question in `01_ideas.md`, write the resolution here in one
sentence: the decision made and the concrete mechanism. Decisions still open stay
open — say so explicitly rather than omitting them.
