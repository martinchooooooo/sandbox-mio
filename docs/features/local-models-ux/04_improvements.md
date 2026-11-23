---
type: improvements
title: "Local Models UX: Deferred Improvements"
feature: local-models-ux
---
# Local Models UX: Deferred Improvements

These ideas were scoped out of `docs/features/local-models-ux/01_ideas.md`. Each is worthwhile, but none is needed to make the Models page match the rest of the app or to make the model list meaningful.

## Measured speed per computer

After a Model's first wake-up, the app could time a short fixed prompt and store words per second per model in the app-data root, replacing the "expected" speed heuristic in `src/lib/models/describe.ts` with a measured label. Deferred because a measurement exists only after a download, so it cannot guide the first choice, and it adds persisted state with its own staleness question (`docs/features/models-page-caching/01_ideas.md` rejected persisted probe data for the same reason).

## Graphics memory in the fit verdict

`MachineSpecs` (`src/lib/models/types.ts`) carries GPU names but no graphics memory, so the fit verdict in `fitFor()` (`src/lib/models/recommend.ts`) reasons over system memory only. A probe of dedicated graphics memory would let the page say "this graphics chip can hold the whole Model". Deferred because it needs a new Rust probe per vendor and the bundled llama.cpp already offloads what fits.

## An animated wake-up on the story stage

The story-stage engine (`src/lib/story-stage/`) could show the Model moving from the shelf (disk) into memory and reaching out to each sandbox, driven by `RuntimeSetupEvent`s the way `docs/features/chat-agent-anatomy/01_ideas.md` drives its live stage from chat beats. Deferred because a wake-up lasts seconds, needs new actors, and the tour-voiced status line already narrates it.

## An onboarding chapter on local Models

A short chapter in `src/lib/onboarding/chapters/` could explain why a Model on your own computer is private and free but smaller, and why memory decides which Model fits. Deferred because the book plays during first run, where every page costs time against the fastest-time-to-wow metric, and explainers can only link into the book once it can be opened at a given page (`docs/features/chat-agent-anatomy/04_improvements.md`).

## Managing downloaded Models

The app offers no way to delete a downloaded GGUF or see the disk space downloads use. The shelf would be the natural home for "Remove from this computer". Deferred because it needs a new backend command and a guard against removing the serving Model.

## Comparing local and cloud Models

The page could place cloud Models a user has a login for (for example Claude through Claude Code) beside local ones, with the same scales. Deferred because cloud Models are reached through agents, not the inference server, and their tradeoffs (cost, privacy) need their own copy.

## A refreshable catalogue

New open Models arrive every few months, and the catalogue ships inside the app. A signed catalogue file fetched on demand would let the shelf grow without a release. Deferred because every entry must declare an architecture the pinned llama.cpp release can load (`docs/architecture/inference.md`, Invariants), which ties the catalogue to the app build.

## A mocked machine probe for the light tier

`detect_machine_specs` (`src-tauri/src/commands/inference.rs`) returns the host's real memory even under `MIO_MOCK_BACKEND=1`, so which fit groups the light tier sees depends on the machine running it. A harness-set override returning fixed specs would let a light spec assert each group and each fit sentence. Deferred because it adds a Rust change and a harness environment variable to an otherwise frontend-only feature; the light spec instead asserts structure that holds on any machine (`02_dev.md`, Resolved Decisions).
