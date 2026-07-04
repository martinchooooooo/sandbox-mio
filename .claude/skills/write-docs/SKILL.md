---
name: write-docs
description: >
  Write technical documentation following project conventions: concise prose
  optimised for AI agent consumption, no inline source code, explicit nouns
  instead of demonstratives, and a Common Gotchas table at the end.
  Use when asked to write or update documentation.
---

## Audience

Documents are read by future AI agents, not humans browsing a wiki. Optimise for
machine retrieval: every sentence must be interpretable in isolation.

## Structure

1. Short overview paragraph — what the document covers and where relevant files live.
2. One `##` section per major concept.
3. `## Common Gotchas` table as the final section (see format below).

## Content Rules

**No source code or snippets.** Reference code by file path and function/command name only.
Acceptable: "`podman_pipe()` in `src-tauri/src/lib.rs`"
Not acceptable: a code block showing the function body.

**No demonstratives or vague pronouns.** Replace "this", "that", "the above", "it", "the pull",
"the machine", "the setting" with the explicit noun the reader needs to search for.

DONT: "When the problem mentioned above occurs, delete the data."
INSTEAD: "When data integrity errors occur, delete the data."

DON'T: "The pull is done in `ensurePodmanMachine` and is idempotent."
INSTEAD "Image pre-pulling is handled in `ensurePodmanMachine` (`e2e/wdio.conf.js`) and is idempotent."

**Be concise.** One sentence per idea. No filler phrases. No restating what was just said.

**File paths are mandatory** whenever a file, function, or config key is mentioned.
Always include the path so a future agent can locate the artefact without context.

## Common Gotchas Table

End every document with this table. Each row must be self-contained — a reader
seeing only that row must understand the problem, cause, and fix without reading
surrounding rows.

```
| Gotcha | Cause | Fix |
|---|---|---|
| <exact error or symptom> | <root cause — no pronouns> | <concrete action — no pronouns> |
```

Columns:
- **Gotcha**: the exact error message or observable symptom, quoted if it is a string.
- **Cause**: the root cause stated with explicit nouns. No "this happens because…".
- **Fix**: the concrete command, config change, or code location to address. Include file paths where relevant.
