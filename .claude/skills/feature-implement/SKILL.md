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

1. Read all three docs in `docs/features/<feature>/` (skip any that don't exist).
   Treat `02_dev.md`'s Resolved Decisions as binding; treat its Common Gotchas
   table as a checklist of mistakes already made once.
2. Read the `AGENTS.md` at the repo root, and every `AGENTS.md` in a directory
   you are about to modify. Subdirectory files override the root.
3. If `03_e2e.md` specifies selectors or page-object contracts (e.g.
   `data-service`, `data-status` attributes), those are requirements on the
   implementation, not suggestions — build the UI to that contract verbatim so
   the spec can be written against it unchanged.

## Phase discipline

Implement in the phase order `02_dev.md` gives (typically A, B, C…). Complete
and compile/typecheck each phase before starting the next. After finishing a
phase, edit `02_dev.md`: retitle its section from `## Remaining: Phase X` to
`## Implemented: Phase X` and correct anything the implementation changed
(renamed functions, moved files), so the document never claims unbuilt work is
done or vice versa.

## i18n: hard-code English first, Paraglide last

Paraglide strings are deliberately deferred to minimise reading tokens: every
key added to `messages/en.json` must be mirrored into every other locale file,
and iterating on UI copy through three JSON files plus generated message
imports is expensive while wording is still churning.

**During implementation:** write all user-facing strings as hard-coded English
literals directly in the components. Do not touch `messages/*.json` and do not
import `m` from `$lib/paraglide/messages.js` for new strings. Mark every such
literal with a `TODO(i18n)` comment on the same line so none are lost.

**Only after** the feature works, tests pass, and the user has said they are
happy with the behaviour and wording, do the i18n pass as its own step:

1. `grep` for `TODO(i18n)` across the changed files to enumerate every string.
2. Add each key to `messages/en.json` first, then mirror it into every other
   locale file (`messages/de.json`, `messages/ru.json`, and any others present).
3. Replace each literal with the `m.<key>()` call and delete the `TODO(i18n)`
   marker. Finish with a final `grep` proving no `TODO(i18n)` remains.
4. Backend-originated status strings go through `translateStatus()` in
   `src/lib/sandbox.ts` (see root `AGENTS.md`), not through new ad-hoc keys in
   components.

Never do the i18n pass pre-emptively "while you're in the file" — one batch
pass at the end, after user sign-off.

## Code comments

The root `AGENTS.md` Code Comments section is the rule; the `document-code`
skill elaborates it. In practice for new feature code:

- Default to **no comment**. Comment only a hidden constraint, a workaround for
  a specific confirmed bug, or behaviour that would surprise a reader — the
  gotchas tables in the feature docs are the usual source of these (e.g. "why
  `exec -i` + `cat` instead of `podman cp`" deserves one line; a load function
  named `load_manifest` deserves none).
- Never write comments that narrate the plan ("Phase C: capture"), restate the
  code, or justify the change to a reviewer. `TODO(i18n)` markers are the one
  sanctioned temporary comment and must all be gone by the end of the i18n pass.
- New trait/interface method declarations: one or two lines of non-obvious
  contract only. New modules: one or two lines stating role and where they are
  wired (e.g. registered in `src-tauri/src/lib.rs`).

## Project conventions that always apply

- Tauri commands return `Result<T, AppError>` — never `Result<T, String>`. New
  error enums get a `#[from]` arm in `AppError` and a matching `kind()` arm
  (`src-tauri/src/error.rs`), plus the `AppErrorKind` union member in
  `src/lib/errors.ts`. Frontend catch blocks use `parseError(e)`, never
  `String(e)` or message substring matching.
- New Tauri commands must be added to `tauri::generate_handler![]` in
  `src-tauri/src/lib.rs` or the frontend gets `command not found` at runtime.
- UI: reach for existing `src/lib/components/ui/` shadcn-svelte components
  first; ask before hand-rolling an alternative or adding a new shadcn
  component.

## Done means

Every phase section in `02_dev.md` says Implemented (or explicitly says what
remains and why), the app compiles and relevant tests pass, secrets/credential
content never appears in logs or IPC payloads if the feature handles any, and —
only after user approval — the i18n pass has run to zero `TODO(i18n)` markers.
