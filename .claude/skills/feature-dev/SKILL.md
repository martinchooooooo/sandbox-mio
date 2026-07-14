---
name: feature-dev
description: >
  Write the technical build plan (02_dev.md) for a feature under
  docs/features/<feature>/. Turns the ideation into a concrete implementation
  plan: resolves the open questions from 01_ideas.md, breaks the work into phases,
  tracks implemented-vs-remaining, cites a file path for every seam.
---

Write to `docs/features/<feature>/02_dev.md`. Read the feature's `01_ideas.md`
first; the build plan continues and supersedes it. The step after this is
`03_e2e.md` (the test plan). Written for future AI agents; follow the
`write-docs` skill conventions.

## Purpose

Be the build plan a future agent can execute or continue: the concrete model
(not the option space — that was `01_ideas.md`), the design decisions now
settled, every seam named by file path, and what is built versus what remains.

## Structure

1. **Overview paragraph** — what the document is the build plan for, which `01_ideas.md` it continues, and a one-line status of implemented vs remaining work.
2. **The concrete model** — one `##` section describing the chosen architecture as it is being built, not as options.
3. **Resolved Decisions** — restate every Open Question from `01_ideas.md` and give its resolution in one sentence: the decision made and the concrete mechanism. Decisions still open stay open — say so explicitly rather than omitting them.
4. **Implementation phases** — phases A, B, C…, each a `## Implemented: Phase X` or `## Remaining: Phase X` section naming every function, struct, trait, command, and file touched, with paths.
5. **Lifecycle / cross-cutting concerns** — cleanup, state derivation, and similar concerns that span phases.

State build status precisely and per-phase via the section headings. When a
refactor supersedes an earlier approach, record the old approach and why it was
replaced, so a future agent does not reintroduce the removed code.

## Out-of-scope improvements

An improvement noticed while planning that is out of scope goes into
`docs/features/<feature>/04_improvements.md` (one `##` section per improvement:
what it is and why it was deferred), not into the phase list. Improvements can
be added during any phase of the feature workflow.
