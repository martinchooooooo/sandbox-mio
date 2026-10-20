---
name: feature-ideas
description: >
  Write the ideation document (01_ideas.md) for a feature under
  docs/features/<feature>/. Use when starting a new feature or
  asked to ideate, scope, or explore a design.
---

Write to `docs/features/<feature>/01_ideas.md`. Ask for the feature name if it is
not given; the directory name is the feature slug (e.g. `remote-sync`).

First of a few feature documents (dev and test plans come later — don't cover
them here). Written for future AI agents; follow the `write-docs` skill
conventions.

## Purpose

Capture the thinking *before* the build so a future agent can continue the
design without re-deriving it: what the feature is, what already exists, the
proposed model, which technology choices were made and why, how the approach
was validated, and what is still undecided.

The goal is to come to an understanding of the feature we want to build that is 
not too large and focused in scope. If you are unsure of the direction the feature 
is going, ask the user, though be sure to give a recommendation.

## Structure

1. **Overview paragraph** — what the document covers, the current state of the codebase relative to the feature, and where the relevant files live.
2. **Current State** — what exists today that the feature builds on, with file paths to the seams it will extend.
3. **Proposed model** — one `##` section per major concept. Name the seam (function/struct/file) each concept hangs off.
4. **Technology choices with tradeoffs** — for each significant decision: the option chosen, the alternatives evaluated, and the concrete reason each was rejected. This is the defining content of an ideas document; a decision with no stated alternative is a weak entry — find the alternative or state that none was viable. Count a dependency on host-global state (a named podman machine or connection, an SSH alias, a fixed host port, a Windows service) as a real cost on this axis: it cannot be isolated per worktree, so it pins the feature's tests to the host-locked tier and serialises them against every other real-infra run. Prefer the option that stays per-worktree when the two are otherwise close (`e2e/AGENTS.md`).
5. **Validation** — if the approach was proven out (spike, manual run, two-machine topology), what was done and what it confirmed.
6. **Open Questions** — decisions genuinely undecided (not TODOs), each phrased so `02_dev.md` can cite and resolve it by name. When the dev plan resolves one, the resolution moves to `02_dev.md`; the question stays here as the record.
7. **Common Gotchas** — final section, table format defined in the `write-docs` skill.

## Spikes

If unsure about a decision, offer the user a spike and keep them informed of the
tradeoffs. Record a spike as evidence: the date, each risk that cleared, and any
blocker with its verbatim error. Spike artefacts (throwaway scripts, probe code,
captured output) live in `docs/features/<feature>/spike/` — keep them after the
spike concludes.

## Out-of-scope improvements

An idea that is worthwhile but out of scope goes into
`docs/features/<feature>/04_improvements.md` (one `##` section per improvement:
what it is and why it was deferred). Improvements can be added during any phase
of the feature workflow — don't mix them into Open Questions, which are for
decisions the current build must make.
