---
name: feature-ideas
description: >
  Write the ideation document (01_ideas.md) for a feature under
  docs/features/<feature>/. Use when starting a new feature or 
  asked to ideate, scope, or explore a design.
---

Write to `docs/features/<feature>/01_ideas.md`. Ask for the feature name if it is
not given; the directory name is the feature slug (e.g. `remote-sync`).

This is the **first** of three feature documents. Later on we'll come up with dev and tests plans, so don't focus on that in this doc.

## Audience and base conventions

Written for future AI agents, not human wiki browsers. Every sentence must be
interpretable in isolation. Follow the `write-docs` skill conventions exactly.

## Purpose of the ideas document

Capture the thinking *before* the build so a future agent can continue the design
without re-deriving it. The ideas document answers: what is the feature, what is
already in place, what is the proposed model, which technology choices were made
and why, how was the approach validated, and what is still undecided.

## Running and Recording a spike

- If you are unsure about a decision to make, offer the user to run a spike. 
- Try and keep the user in the loop so they are informed of where the tradeoffs may lie. 
- When a spike is run by hand, record it as evidence: the date, each risk that
cleared , and any blocker with its verbatim
error.

## Structure

1. **Overview paragraph** — one paragraph stating what the document covers, the current state of the codebase relative to the feature (what phase is already done vs unimplemented), and where the relevant files live.
2. **Current State** — what exists today that the feature builds on, with file paths to the seams it will extend.
3. **Proposed model** — one `##` section per major concept of the design. Name the seam (function/struct/file) each concept hangs off.
4. **Technology choices with tradeoffs** — for each significant technology decision, name the chosen option, *and name the alternatives that were evaluated and rejected, with the reason for rejection*. This is the defining content of an ideas document; do not omit the rejected options.
5. **Validation** — if the approach was proven out (a spike, a manual run, a two-machine topology), record what was done and what it confirmed.
6. **Open Questions** — the decisions deliberately left unresolved, stated precisely enough that a later document can resolve them by name.
7. **Common Gotchas** — final section, table format below.

## Tradeoff sections

The ideas document is the place to argue technology and architecture choices. For
each decision write: the option chosen, the alternatives considered, and the
concrete reason each alternative was rejected (e.g. "rclone bisync reconciles on a
schedule rather than live"). A decision with no stated alternative is a weak
ideas-document entry — either find the alternative or state that none was viable.

## Open Questions

List decisions that are genuinely undecided, not TODOs. Phrase each so the
`02_dev.md` build plan can later cite and resolve it. When the dev plan resolves
an open question, that resolution moves into `02_dev.md`; the ideas document keeps
the original question as the record of where the decision came from.
