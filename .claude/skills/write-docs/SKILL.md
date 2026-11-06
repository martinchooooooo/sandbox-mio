---
name: write-docs
description: >
  Write technical documentation following project conventions: concise prose
  optimised for AI agent consumption, no inline source code, explicit nouns
  instead of demonstratives. Architecture documents stay brief and high-level
  (concepts, not symbols; Invariants, no gotchas table); feature plans end with
  a Common Gotchas table. Use when asked to write or update documentation.
---

## Audience

Documents are read by future AI agents, not humans browsing a wiki. Optimise for
machine retrieval: every sentence must be interpretable in isolation.

## Where documents go

| Directory | Holds | `type` |
|---|---|---|
| `docs/architecture/` | A brief, high-level picture of how a subsystem works today. No phases, history, rejected options or implementation detail | `architecture` |
| `docs/features/<feature>/` | Plans and their history, written by the `feature-*` skills; detail and rationale live here | per `docs/features/README.md` |
| `AGENTS.md` files | Code structure, conventions and implementation-level Common Gotchas for one area of the repository | none (not under `docs/`) |

## Frontmatter (OKF)

`docs/` is an [Open Knowledge Format](https://okf.md/) bundle. Every `.md` file except `index.md`
and `log.md` starts with a YAML frontmatter block holding at least `type` and `title` (the H1).
Architecture documents also carry `description` (one sentence), `generated` (`by`: an actor such as
`claude-code/opus-5-5` or `human:<id>`; `at`: timestamp), `stale_after` (about three months out),
`sources` (`resource:` paths to the plans the document consolidates) and `code` (the directories
and files the document describes). Add `verified` (`by`, `at`) when someone confirms an
architecture document against the code. Frontmatter paths are relative to the repository root.
`docs/features/README.md` lists every key.

Never edit an `index.md` other than the root `docs/index.md`: run `./build docs --fix`, which
generates every other index from frontmatter. Link between documents with relative markdown links;
keep code paths in backticks. Run `./build docs` after writing; it enforces these rules (source:
`docs_check.rs` at the repository root).

## Architecture documents

An architecture document paints the picture an agent needs to plan or orient: the parts of a
subsystem, how they relate, where state lives, and the rules that must hold. It is not an index of
the code; the `code` frontmatter is the jumping-off point into the code.

- **Concepts, not symbols.** Test every sentence: would a refactor that keeps behaviour the same
  make the sentence false? If so, cut the sentence. That rules out function, type and constant
  names, numeric values (timeouts, ports, pixel sizes), command and flag inventories, and per-file
  pointers. Keep names a user or another process sees: `.mio/` files, the `light`/`standard`/
  `heavy` tiers, the engines' own CLIs.
- **Code locations go in `code` frontmatter,** at directory or file level, never in the prose.
- **Structure:** an overview paragraph, one `##` section per concept, then `## Invariants`: a
  handful of rules stated in domain terms ("No secret value is ever written into the workspace").
  No Common Gotchas table: an implementation-level gotcha belongs as a comment at the code site,
  or in the Common Gotchas table of the area's `AGENTS.md`.
- **Budget:** about 500 words; `./build docs` warns above 700. Tables, short bullets and one
  mermaid diagram (in a fenced block, which the budget does not count) are welcome.
- **One home per fact.** Each fact lives in exactly one architecture document; others link to it.
- **Change only when the picture changes:** a new subsystem, seam, capability or invariant.
  Set `generated` (`by`, `at`) to yourself and now whenever the content changes. Leave existing
  `verified` entries alone: an entry's `at` shows which version was confirmed.
- **Re-verifying:** when `./build docs` warns a document is stale, check every statement against
  the code under `code`, fix what has drifted, then add a `verified` entry with your actor and move
  `stale_after` about three months forward.
- **Rationale** is cited, not restated: link the plan's Resolved Decision.

## Other documents (plans, backlog, reference)

1. Short overview paragraph: what the document covers and where relevant files live.
2. One `##` section per major concept.
3. `## Common Gotchas` table as the final section (see format below). Required in `01_ideas.md`,
   `02_dev.md`, `03_e2e.md` and workflow documents; optional in improvements, reviews, spike
   records, handovers, research and the backlog. `feature.md` is metadata and carries none.

**File paths are mandatory** whenever a file, function, or config key is mentioned in a plan.
Always include the path so a future agent can locate the artefact without context.

## Content Rules (every document)

**No source code or snippets.** Reference code by file path and function or command name only.
Acceptable: "`ensure_sandbox_alias()` in `src-tauri/src/backends/mod.rs`"
Not acceptable: a code block showing the function body. Mermaid diagrams are not source code.

**No demonstratives or vague pronouns.** Replace "this", "that", "the above", "it", "the pull",
"the machine", "the setting" with the explicit noun the reader needs to search for.

DON'T: "When the problem mentioned above occurs, delete the data."
INSTEAD: "When data integrity errors occur, delete the data."

DON'T: "The build is done in `ensureMiseBaseImage` and is idempotent."
INSTEAD: "Base-image building is handled in `ensureMiseBaseImage` (`e2e/lib/podman-machine.ts`) and is idempotent."

**Be concise.** One sentence per idea. No filler phrases. No restating what was just said.

## Common Gotchas Table

End the documents listed above with this table; never put one in an architecture document. Each
row must be self-contained: a reader seeing only that row must understand the problem, cause, and
fix without reading surrounding rows.

```
| Gotcha | Cause | Fix |
|---|---|---|
| <exact error or symptom> | <root cause — no pronouns> | <concrete action — no pronouns> |
```

Columns:
- **Gotcha**: the exact error message or observable symptom, quoted if it is a string.
- **Cause**: the root cause stated with explicit nouns. No "this happens because…".
- **Fix**: the concrete command, config change, or code location to address. Include file paths where relevant.
