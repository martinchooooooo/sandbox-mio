---
name: feature-implement
description: >
  Implement a feature that has a build plan (02_dev.md) under
  docs/features/<feature>/. Use when asked to implement, build, or continue a
  planned feature. Works phase by phase, defers Paraglide i18n until after the
  user has approved the working feature, keeps 02_dev.md's phase status current,
  and enforces the project's code commenting rules.
---

Implementation follows the plan in `docs/features/<feature>/02_dev.md`. Do not
re-derive the design: `01_ideas.md` records the option space and rejected
alternatives, `02_dev.md` records the settled decisions, and `03_e2e.md` (when it
exists) records the DOM/selector contract the UI must satisfy. If the plan turns
out to be wrong during implementation, stop and say so — do not silently deviate.

## Before writing any code

1. Read all docs in `docs/features/<feature>/` (skip any that don't exist): the
   three core documents, plus `04_improvements.md` (deferred work and known seam
   limitations) and `05_dev_review.md` (review findings and their resolutions)
   when present. `02_dev.md`'s Resolved Decisions are binding; its Common Gotchas
   table is a checklist of mistakes already made once. Selectors or page-object
   contracts in `03_e2e.md` (e.g. `data-service`, `data-status` attributes) are
   requirements — build the UI to that contract verbatim.
2. Read the root `AGENTS.md` and every `AGENTS.md` in a directory you are about
   to modify (subdirectory files override the root). Error handling, command
   registration, UI component, and code commenting conventions live there —
   `AGENTS.md` is the source of truth, not this skill.

3. Read the `docs/architecture/` documents for the subsystems you will touch
   (`docs/architecture/index.md` maps subsystems to documents), then the code
   under each document's `code` frontmatter. Where the code has moved on since
   `02_dev.md` was written, the code describes reality; stop and say so before
   building on a stale plan.
4. In the feature's `feature.md`, set `status: in-progress`, then run
   `./build docs --fix` to regenerate the indexes (formats:
   `docs/features/README.md`).

## Phase discipline

Implement in the phase order `02_dev.md` gives. Complete and compile/typecheck
each phase before starting the next. After finishing a phase, edit `02_dev.md`:
retitle its section from `## Remaining: Phase X` to `## Implemented: Phase X`
and correct anything the implementation changed, so the document never claims
unbuilt work is done or vice versa.

## Out-of-scope improvements

An improvement noticed during implementation that is out of scope goes into
`docs/features/<feature>/04_improvements.md` (one `##` section per improvement:
what it is and why it was deferred) — not into the code as a TODO comment and
not silently built.

## Parallel-worktree constraints

The isolation rules the plan settled are binding at the keyboard too — the `feature-dev` skill lists
them, `e2e/AGENTS.md` explains the workflow they protect. Two slips account for most breakage: a
host path built from `%APPDATA%`, the bundle identifier, or a fixed `$HOME` name instead of
`app_data_dir()` in `src-tauri/src/paths.rs`; and a hard-coded host port instead of one from the
run's window. Both compile, both pass a single-worktree run, and both corrupt a neighbouring
worktree's run. Grep new code for the bundle identifier and for literal port numbers before calling a
phase done.

## i18n: hard-code English first, Paraglide last

Paraglide strings are deliberately deferred: every key added to
`messages/en.json` must be mirrored into every other locale file, which is
expensive while wording is still churning. During implementation, write all
user-facing strings as hard-coded English literals marked with a `TODO(i18n)`
comment on the same line; do not touch `messages/*.json` or import from
`$lib/paraglide/messages.js` for new strings.

Only after the feature works and the user has approved behaviour and wording,
do the i18n pass as one batch: grep `TODO(i18n)` to enumerate the strings, add
each key to `messages/en.json` and mirror it into every other locale file,
replace each literal with its `m.<key>()` call, and finish with a grep proving
no `TODO(i18n)` remains. Backend-originated status strings go through
`translateStatus()` in `src/lib/sandbox.ts`, not new ad-hoc keys.

## Code comments

`AGENTS.md`'s Code Comments section is the rule. Feature-specific notes: the
gotchas tables in the feature docs are the usual source of the rare justified
comment (a hidden constraint or confirmed-bug workaround); `TODO(i18n)` is the
one sanctioned temporary comment and must be gone by the end of the i18n pass.

## Closing out: architecture docs and status

Once every non-deferred phase is implemented, the plan becomes history.
Architecture documents are a brief, high-level picture, not a record of every
feature, so they change only when the picture does:

1. Update a `docs/architecture/` document only if the feature changes the overall
   picture: a new subsystem, seam, capability, cross-cutting invariant, or a row
   of a capability table. Follow the `write-docs` skill's architecture rules
   (concepts, not symbols; no gotchas table; word budget), refresh
   `generated.at`, and add the feature's `02_dev.md` to `sources` and any new
   top-level code location to `code`. If no document covers a new subsystem,
   create one. A feature that only adds behaviour inside an existing picture
   changes no architecture document.
2. Put implementation-level knowledge where an implementer will meet it: a
   hidden constraint as a comment at the code site, an area-wide trap in that
   area's `AGENTS.md` Common Gotchas.
3. In `feature.md`, set `status: implemented`, make `architecture` list the
   documents for the subsystems the feature changed, and record any deferred
   phases in the status note. Add an `**Implement**` entry to the top of
   `docs/features/log.md`, then run `./build docs --fix`.

## Done means

Every phase section in `02_dev.md` says Implemented (or explicitly says what
remains and why), the app compiles and relevant tests pass, secrets/credential
content never appears in logs or IPC payloads if the feature handles any,
`feature.md` is set to `implemented` with any architecture change made per the
section above, `./build docs` passes, and — only after user approval — the i18n
pass has run to zero `TODO(i18n)` markers.
