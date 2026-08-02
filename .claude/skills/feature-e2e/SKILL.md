---
name: feature-e2e
description: >
  Write the end-to-end test plan (03_e2e.md) for a feature under
  docs/features/<feature>/. Use when planning E2E coverage,
  recording a test spike, or guarding a feature against regression.
---

Write to `docs/features/<feature>/03_e2e.md`. Read the feature's `02_dev.md`
first; the test plan covers only how to test the build it describes. Written for
future AI agents; follow the `write-docs` skill conventions.

## Purpose

Be the plan for proving the feature works through the **real application**, not
a side-channel. State which actual code path (which Tauri commands, which
bundled binaries) the test exercises, so a future agent knows the test has
teeth.

## Tier selection

The e2e suite has three tiers — `standard`, `light`, `heavy` — with the taxonomy and decision rules
owned by `e2e/AGENTS.md`; read it before choosing. **`standard` is the default**: write the
happy/common-path spec against the real backend first, since that is the tier that actually proves
the feature works. Promote to `heavy` only if the path downloads a large asset, loads a real
model/engine, or chains a real remote deploy. Drop to `light` (mock backend, no podman) only for an
edge case that's awkward to provoke against real infra, or to give a fast debug loop alongside the
real spec — never as the only coverage for a new feature's main path.

## Structure

1. **Overview paragraph** — what the document is the test plan for, which `02_dev.md` it depends on, and whether a spike has been run.
2. **Goal** — the exact real code path the test must prove (name the commands and bundled components), and explicitly what a weaker test would *fail* to cover.
3. **Chosen approach** — the tier, the spec path (e.g. `e2e/test/specs/standard/<name>.spec.ts`), the topology it uses, and why. Note any harness or environment prerequisites (`e2e/wdio.conf.ts` edits, provisioning, cleanup, machines or sidecars a future session needs).
4. **Spec scenario** — numbered steps, each naming the UI action driven and the assertion made (read state back through the CLI and filesystem, mirroring an existing spec).
5. **De-risking spike results** — if a spike was run (see below).
6. **Common Gotchas** — final section, table format defined in the `write-docs` skill.

## Parallel-worktree constraints

Other worktrees are running their own `light` tier while this spec runs, so a spec that reaches a
fixed host path passes or fails depending on what a neighbouring worktree is doing. Three rules
follow, and the test plan states any decision that touches them:

- Derive every host path from `e2e/lib/run-identity.ts` (`APP_DATA_ROOT`, `WORKSPACE_ROOT`). Never
  rebuild `%APPDATA%/<bundle identifier>` or a fixed `$HOME` name in a spec or helper — the app is
  not writing there.
- Assert that a path was **not modified during this run**, not that it does not exist. A machine that
  ran an earlier version of the suite still carries its leftovers, and an absence assertion fails for
  reasons unrelated to the code under test.
- Choosing `standard` or `heavy` means the run takes the host lock and serialises against every other
  real-infra run on the machine. That is the intended cost of real coverage; it is worth noting in
  the plan when a feature would otherwise have been testable at the `light` tier.

## Recording a spike

Record a spike as evidence: the date, each risk that cleared (with the command
that cleared it), and any blocker with its verbatim error. Classify each blocker
as a **feature fix** or a **test concern** and say whether the spec should wait
on it — a spec that would go red at a known blocker is not worth writing yet.
Spike artefacts (throwaway scripts, probe specs, captured output) live in
`docs/features/<feature>/spike/` — keep them after the spike concludes.

## Out-of-scope improvements

An improvement noticed while planning or running tests that is out of scope goes
into `docs/features/<feature>/04_improvements.md` (one `##` section per
improvement: what it is and why it was deferred). Improvements can be added
during any phase of the feature workflow.
