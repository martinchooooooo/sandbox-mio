---
type: workflow
title: "Features"
description: "How feature plans are written, the frontmatter keys every document carries, and what the feature statuses mean."
---
# Features

Each directory under `docs/features/` is one feature of Sandbox-mio, planned and recorded in up to five documents:

- `01_ideas.md`: what the feature is, which technologies it could use, and the trade-offs between options.
- `02_dev.md`: the technical build plan, phase by phase, with its resolved design decisions.
- `03_e2e.md`: the test plan that proves the feature works and guards it against regression.
- `04_improvements.md` (optional): work deferred as out of scope, plus known seam limitations discovered during implementation.
- `05_dev_review.md` (optional): post-implementation code review findings and the resolution recorded against each one.

The first three documents are the core pipeline; each is produced by a matching project skill: `feature-ideas` → `01_ideas.md`, `feature-dev` → `02_dev.md`, `feature-implement` → code (keeps `02_dev.md` phase status current), `feature-e2e` → `03_e2e.md`. The optional follow-ons have no dedicated skill: every skill in the pipeline appends to `04_improvements.md`, and `05_dev_review.md` is written by a review pass over finished code. When resuming a feature, start from the most advanced core document already present in the feature's directory. Some features also carry a `spike/` directory with throwaway reproduction scripts referenced from `02_dev.md`.

Plans describe intent at the time they were written. Current behaviour lives in `docs/architecture/`, which stays short and high-level; a plan keeps the detail and the rationale.

## Document format (OKF)

`docs/` is an [Open Knowledge Format](https://okf.md/) v0.2 bundle. `./build docs` enforces every rule in this section; the rules are code in `docs_check.rs` at the repository root, which wins wherever this prose and the code disagree.

- Every `.md` file except `index.md` and `log.md` starts with a YAML frontmatter block holding a `type` and a `title`.
- Every `index.md` except the root `docs/index.md` is generated from frontmatter by `./build docs --fix`; never edit one by hand. `./build docs` fails when an index is out of date. Generated indexes carry no frontmatter; the hand-written root `docs/index.md` declares only `okf_version`.
- `log.md` holds `## YYYY-MM-DD` sections, newest first, with entries that start with a bold verb: `**Create**`, `**Implement**`, `**Supersede**` or `**Abandon**`. The log records feature status changes only; git is the record of everything else. `.gitattributes` merges the log with `merge=union`, so parallel branches adding entries do not conflict.
- Every relative markdown link resolves, and every document is linked from some `index.md`.
- Citations of missing `docs/...` paths are warnings, except inside `log.md` and the documents of `implemented`, `superseded` or `abandoned` features, which are history and may cite files that have since moved. The same warning covers citations from code, skills and `AGENTS.md` files.

Links between documents are relative markdown links so they also resolve on GitHub. Code paths stay in backticks. Paths inside frontmatter (`architecture`, `sources`, `code`) are relative to the repository root.

### Frontmatter keys

| Key | Where | Meaning |
|---|---|---|
| `type` | every document | `feature`, `ideas`, `dev-plan`, `e2e-plan`, `improvements`, `review`, `research`, `handover`, `spike`, `workflow`, `reference`, `backlog`, `architecture` |
| `title` | every document | The document's H1 (`feature.md`: the directory name) |
| `description` | `feature.md`, architecture documents; optional elsewhere | One sentence; becomes the document's line in its generated `index.md` |
| `feature` | documents inside a feature directory | The feature directory name |
| `status` | `feature.md` | Lifecycle state, below |
| `depends_on` | `feature.md`, optional | Feature directory names this feature builds on |
| `superseded_by` | `feature.md`, optional | Feature directory name that replaced this one |
| `architecture` | `feature.md`, optional | `docs/architecture/` documents for the subsystems the feature changes. Set it from the ideation stage on: in-flight features with this key are listed under their architecture documents in `docs/architecture/index.md` |
| `generated` | architecture documents | `by` (an OKF actor: `<producer>/<version>`, e.g. `claude-code/opus-5-5`, or `human:<id>`) and `at` (timestamp of the last meaningful change) |
| `verified` | architecture documents, optional | A `{by, at}` mapping or a list of them, added when someone confirms the document against the code; a `human:<id>` actor marks it human-reviewed |
| `stale_after` | architecture documents | Timestamp after which `./build docs` warns that the document needs re-verifying |
| `sources` | architecture documents | `resource:` items naming the plans the document consolidates |
| `code` | architecture documents | The directories and files the document describes; the jumping-off points into the code |

Timestamps are ISO 8601 with seconds and a UTC offset. Plan documents carry no date keys: git records when they changed.

### Feature status

Each feature directory has a `feature.md` that is the single source of truth for its state. Its `status` is one of:

| Status | Meaning |
|---|---|
| `idea` | Only `01_ideas.md` exists |
| `planned` | `02_dev.md` exists; no phase implemented |
| `in-progress` | At least one phase implemented, work remaining that is not deferred |
| `implemented` | All non-deferred phases shipped; deferred work lives in `04_improvements.md` |
| `superseded` | Replaced by another feature (set `superseded_by`) |
| `abandoned` | Dropped without replacement |

When a feature's status changes: update `status` and the status note in `feature.md`, run `./build docs --fix` to regenerate the indexes, and, for a change to `idea` (creation), `implemented`, `superseded` or `abandoned`, add an entry to the top of `features/log.md`. A `feature.md` status note may record where `02_dev.md` phase headings are stale; `feature.md` wins over the phase headings.

When a feature reaches `implemented`, the plan becomes history. Update a `docs/architecture/` document only when the feature changes the overall picture (a new subsystem, seam, capability or invariant), following the `write-docs` skill; either way, set `architecture` in `feature.md` to the documents that describe the subsystems it changed.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `./build docs` reports an `index.md` as out of date | A document's `title`, `description` or `type`, or a feature's `status`, changed, or a document was added or removed | Run `./build docs --fix`; never edit a generated index by hand |
| An agent follows a stale `02_dev.md` and rebuilds removed behaviour | Plans are history once `status: implemented`; their phase text is not updated afterwards | Read `docs/architecture/` for current behaviour; open a plan only for in-flight work or design rationale |
| An OKF validator reports a missing `type` | A new `.md` file was written without frontmatter | Add the frontmatter block before the H1 |
| `./build docs` rejects a new `type`, status or log verb | The vocabularies are the `TYPES`, `STATUSES` and `LOG_VERBS` constants in `docs_check.rs` | Add the value to the constant and to the tables in this file in the same change |
| `./build docs` rejects a log entry starting with `**Update**` | The log records status changes only | Drop the entry; git records the change |
