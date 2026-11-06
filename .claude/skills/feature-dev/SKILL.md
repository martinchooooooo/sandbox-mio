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

## Bookkeeping

`docs/features/README.md` defines the frontmatter keys, statuses and log format.

- Start `02_dev.md` with frontmatter: `type: dev-plan`, `title` (the H1), `feature: <slug>`.
- If the feature has no `feature.md` yet (planning started without `01_ideas.md`), create it and
  its `**Create**` log entry as the `feature-ideas` skill's Bookkeeping section describes.
- In `feature.md`, set `status: planned` (unless a phase is already implemented) and make
  `architecture` list the `docs/architecture/` documents for every subsystem the plan changes.
- Run `./build docs --fix` to regenerate the indexes, then `./build docs` must pass. No log entry:
  the log records only creation, implementation, supersession and abandonment.

Cite seams against the code as it is now: `docs/architecture/` describes current behaviour, and
an implemented feature's `02_dev.md` may describe a design that has since changed.

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
6. **Common Gotchas** — final section, table format defined in the `write-docs` skill.

State build status precisely and per-phase via the section headings. When a
refactor supersedes an earlier approach, record the old approach and why it was
replaced, so a future agent does not reintroduce the removed code.

## Parallel-worktree constraints

Several worktrees of this repository run their `light` e2e tier simultaneously on one machine, and a
single `standard`/`heavy` run may run host-wide alongside them. A plan that introduces new
host-global state breaks that workflow silently: the failure surfaces as one worktree's run
corrupting another's, never as a compile error. `e2e/AGENTS.md` owns the operational rules and
`docs/features/parallel-worktree-e2e/02_dev.md` the design; what follows is what a *plan* must
settle.

State each of the following explicitly — in Resolved Decisions, or in the phase that introduces it:

- **A new path the app reads or writes.** Route it through `app_data_dir()` in
  `src-tauri/src/paths.rs`, which honours the per-worktree `MIO_APP_DATA_DIR` override. A path
  assembled from the bundle identifier, from `%APPDATA%`, or from a fixed name under `$HOME` bypasses
  the override, and every run on the machine then shares one directory.
- **A new host port.** Allocate from the run's window (`allocatePortPair()` in
  `e2e/lib/run-identity.ts`) instead of pinning a constant. A fixed port *inside* a sandbox or podman
  machine is fine — the collision exists only on the host.
- **A new host-global external resource** — a podman machine or connection name, an SSH alias, a
  named Mutagen session, a Windows service. These cannot be slug-keyed cheaply, so a feature needing
  one confines its specs to the host-locked tier and serialises against every other real-infra run on
  the machine. Say so in the plan: it is a schedule cost, not an implementation detail.
- **A new bundled binary under `src-tauri/binaries/`.** Give it its own asset group in `build.rs`: a
  pinned version constant, a sentinel file, and a `fetch_*` that records the version into the
  staged-versions manifest. Without the group, a fresh worktree cannot be staged, and a hard link
  from a sibling worktree can hand over a version this checkout does not pin.
- **New teardown that deletes host-global state.** Gate it on the tier, as the `~/.ssh` cleanup in
  `e2e/wdio.conf.ts` is. Ungated, it destroys state belonging to a run in another worktree.
- **A new environment variable the harness sets.** Document it in the root `AGENTS.md`, marked
  harness-set when the Rust side does not read it.

Stray-process cleanup is already worktree-scoped by an absolute `target\debug\` prefix match. A plan
that adds process killing uses the same filter rather than matching on process name alone.

## Out-of-scope improvements

An improvement noticed while planning that is out of scope goes into
`docs/features/<feature>/04_improvements.md` (one `##` section per improvement:
what it is and why it was deferred), not into the phase list. Improvements can
be added during any phase of the feature workflow.
