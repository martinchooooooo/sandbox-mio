---
name: document-code
description: >
  Add inline doc comments to code in any language so future agents know which API to call,
  which env vars control behaviour, and what is internal vs public API.
  Use when asked to document types, functions, classes, or modules.
---

## Audience

Comments are read by future AI agents navigating an unfamiliar codebase.
Every comment must be interpretable without reading surrounding code.

## What to document

**Always document:**
- Public API surfaces — interfaces, traits, exported classes, and modules. State the construction or injection pattern (e.g. how a Rust trait is held in Tauri state, how a class is instantiated or imported) and what each method's contract is.
- Public data types — one line on the transport or access mechanism and any per-call resource allocation behaviour.
- Opaque selectors or magic values — when a string, query, or constant carries meaning its literal form does not (e.g. a CSS selector, a status code), state what it represents.

**Skip documenting:**
- Methods that are straightforward delegations to an already-documented API.
- Private helpers whose name already states intent.
- Error enum variants whose `#[error("…")]` message already carries the full diagnostic — the display string is the documentation.

## Content rules

**State the contract, not the implementation.** Describe what the function guarantees to the caller, not how it achieves it. Where the contract is non-obvious, state when the call returns or what it resolves to.

**Name env vars explicitly.** If a function reads an environment variable, name the variable and its default in the comment. Include the name of any test harness that overrides the variable.

**No source code in comments.** Reference other items by name and file path, never by quoting their body.

**One sentence per idea.** No filler, no restating the function name.

**Trait and interface methods get one or two lines, not paragraphs.** State the non-obvious contract (idempotency, ordering constraints, what the return value represents). Skip anything the parameter names and return type already convey.

**Module-level comments state role and wiring, not contents.** Name the public trait or entry point, where it is registered (e.g. Tauri state), and which file does the wiring. Do not list every function in the module.

**Match the language's comment idiom.** Use the doc-comment convention the surrounding code already uses (e.g. `///` or `/** */` in Rust, JSDoc `/** */` or `//` in JS/TS, docstrings in Python) and keep one comment to its smallest sufficient form.
