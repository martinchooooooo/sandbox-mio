---
name: document-code
description: >
  Add inline doc comments to Rust code so future agents know which trait to call,
  which env vars control behaviour, and what is internal vs public API.
  Use when asked to document traits, structs, or functions in the backend.
---

## Audience

Comments are read by future AI agents navigating an unfamiliar codebase.
Every comment must be interpretable without reading surrounding code.

## What to document

**Always document:**
- Public traits — state the injection pattern (e.g. how the trait is held in Tauri state) and what each method's contract is.
- Public structs — one line on the transport mechanism and any per-call resource allocation behaviour.

**Skip documenting:**
- `impl` methods that are straightforward delegations to an already-documented trait method.
- Private helper functions whose name already states intent.

## Content rules

**State the contract, not the implementation.** Describe what the method guarantees to the caller, not how it achieves it.

**Name env vars explicitly.** If a function reads an environment variable, name the variable and its default in the comment. Include the name of any test harness that overrides the variable.

**No source code in comments.** Reference other items by name and file path, never by quoting their body.

**One sentence per idea.** No filler, no restating the function name.
