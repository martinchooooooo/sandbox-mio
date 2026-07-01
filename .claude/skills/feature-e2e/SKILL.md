---
name: feature-e2e
description: >
  Write the end-to-end test plan (03_e2e.md) for a feature under
  docs/features/<feature>/. Use when planning E2E coverage,
  recording a test spike, or guarding a feature against regression.
---

Write to `docs/features/<feature>/03_e2e.md`. Read the feature's `02_dev.md`
first; the test plan covers only how to test the build it describes.

## Audience and base conventions

Written for future AI agents. Every sentence must stand alone. Follow the
`write-docs` skill conventions exactly.

## Purpose of the e2e document

Be the plan for proving the feature works through the **real application**, not a
side-channel. State plainly which actual code path (which Tauri commands, which
bundled binaries) the test exercises, so a future agent knows the test has teeth.
Record any de-risking spike so a blocker found by hand is not rediscovered.

## Tier selection

The e2e suite has three tiers — `standard`, `light`, `heavy` — with the taxonomy and decision rules
owned by `e2e/AGENTS.md`; read it before choosing. **`standard` is the default**: write the
happy/common-path spec against the real backend first, since that is the tier that actually proves
the feature works. Promote to `heavy` only if the path downloads a large asset, loads a real
model/engine, or chains a real remote deploy. Drop to `light` (mock backend, no podman) only for an
edge case that's awkward to provoke against real infra, or to give a fast debug loop alongside the
real spec — never as the only coverage for a new feature's main path.

## Structure

1. **Overview paragraph** — what the document is the test plan for, which `02_dev.md` it depends on, and whether a spike has already been run.
2. **Goal** — the exact real code path the test must prove (name the commands and the bundled components), and explicitly what a weaker test would *fail* to cover.
3. **Existing test styles** — the test styles already in the repo (e.g. app-driven WebdriverIO specs vs standalone POCs), with paths, and which one the new test belongs to.
4. **Chosen approach** — the tier (`standard` by default; see Tier selection above) and the spec to add (path, e.g. `e2e/test/specs/standard/<name>.spec.ts`), the topology it uses (e.g. a second local Podman machine as the deploy target, no external host), and why. If a `light` spec is also planned for an edge case or debug loop, name it and say what it covers that the `standard` spec doesn't.
5. **Harness changes** — concrete edits to the harness (e.g. `e2e/wdio.conf.ts`): provisioning, cleanup honouring env flags, and any build/staging step the test needs.
6. **Spec scenario** — the numbered steps the spec performs, each naming the UI action driven and the assertion made (read state back through the CLI and filesystem, mirroring an existing spec).
7. **De-risking spike results** — if a spike was run, record the date, what cleared, and any blocker found. Mark a blocker as feature-fix-vs-test-concern and whether it must be fixed before the spec is worth writing.
8. **Sequencing** — the order to implement fixes and the spec, especially when a spike inverted the original plan.
9. **Environment notes** — state a future session needs (machines that exist, sidecars staged, ports, sockets to re-activate).
10. **Common Gotchas** — final section, table format below.

## Recording a spike

When a spike is run by hand, record it as evidence: the date, each risk that
cleared (with the command that cleared it), and any blocker with its verbatim
error. Classify each blocker as a **feature fix** or a **test concern** and say
whether the spec should wait on it — a spec that would go red at a known blocker
is not worth writing yet.

## Common Gotchas Table

End the document with this table. Each row self-contained.

```
| Gotcha | Cause | Fix |
|---|---|---|
| <exact error or symptom> | <root cause — no pronouns> | <concrete action — no pronouns> |
```

- **Gotcha**: the exact error or observable symptom, quoted if a string.
- **Cause**: the root cause in explicit nouns.
- **Fix**: the concrete command, config change, or code location, with file paths.
