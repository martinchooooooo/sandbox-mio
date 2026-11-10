---
type: dev-plan
title: "Onboarding Story — Build Plan"
feature: onboarding-story
---
# Onboarding Story — Build Plan

This document is the build plan for putting the illustrated explainer in `onboarding-story/` into the app's first-run welcome flow. Planning started without an `01_ideas.md`: the scope came from the standalone story itself (`onboarding-story/AGENTS.md` describes its sixteen pages and fixed vocabulary) and from three stated requirements, restated under Requirements below. The welcome flow this feature extends is described in `docs/features/welcome-onboarding/02_dev.md`, and the task ledger it reports through in `docs/features/activity-center/02_dev.md`. Phases A to E are implemented; Phase F (translations) remains.

## Requirements

1. The onboarding flow follows the story in `onboarding-story/`: five acts and a recap, sixteen pages.
2. While the user reads, first-run setup (the virtual machine and the base image) runs in the background, so the user is ready to create a sandbox when the story ends. The user must be told that setup is running.
3. A fork of the repository can add its own onboarding material, using existing UI components so the result matches the app.
4. Onboarding text is multilingual, in Paraglide files separate from `messages/{locale}.json`. For this build only the files and one title key exist; every other string stays a hard-coded English literal until the copy is approved.

## The Concrete Model

**A book made of chapters made of pages.** `src/lib/onboarding/book/` is a kit of Svelte components. `Book` renders an animated stage, the current page's text, and Back / dots / Next. A `Page` registers itself with the enclosing `Book` while it initialises and renders its children only while it is the current page, so a chapter is written declaratively: a Svelte component whose markup is a list of `Page`s, grouped by `Act`. `src/lib/onboarding/chapters.ts` is the ordered chapter list, and `OnboardingBook.svelte` renders every listed chapter inside one `Book`.

**A scene per page.** `src/lib/onboarding/stage/` is the whiteboard animation engine, ported from the standalone story's classic scripts into typed modules. `createStage` builds one engine instance inside an SVG element and returns the drawing and timing surface (`StoryStage`). A `Scene` is a function from that surface to a layout, an optional setup and an async script; `Page` takes one as an optional prop. A page without a scene is text-only and the stage collapses.

**One shared setup run.** `src/lib/onboarding/setup.svelte.ts` is a module-scoped store that owns first-run setup: bringing the machine online, then building the base image. `startBackgroundSetup` runs whatever is missing without a click and stops at the first step that needs the user. The two existing setup cards render the store's state and start work only through the store, so a card never starts a second run beside the background one.

**Setup is visible in two places.** The backend records both steps as activity tasks, so they appear in the activity center like any other long-running work. During first-run onboarding the welcome flow shows the activity center in a sticky top bar, widened into a status pill that names the current step, with a one-time coach mark explaining it.

**Flow.** `WelcomeFlow.svelte` gains a `story` step: hero, welcome, story, then the setup cards with Finish. Continuing from the welcome step starts background setup in the onboarding variant only. The story's last Next and a "Skip the tour" link both lead to the setup cards, where Finish stays disabled until both steps are done.

## Resolved Decisions

1. **Port the engine instead of embedding the standalone page.** The story moves into `src/lib/onboarding/` as typed modules and Svelte components. Loading the standalone files in an iframe or as global scripts was rejected: it cannot use the app's components, cannot call Paraglide, and its single global namespace cannot be mounted twice (the first-run route and the `/intro` replay both render the flow).
2. **The standalone `onboarding-story/` directory stays as it is.** It remains a dependency-free page that opens by double-click. The app copy is now the source for what ships; a change to a story beat must be made in both until the standalone copy is retired, which is the author's decision.
3. **Pages are components, not data.** Page text is Svelte markup so it can use kit components and, later, Paraglide calls. HTML strings rendered with `{@html}` were rejected: they cannot contain components, which is what a fork needs for a consistent look.
4. **Registration during component init.** A `Page` registers in its init script, which Svelte runs in source order. Registering in `onMount` was rejected because mount order runs children first and would reorder pages.
5. **One engine instance per mounted stage.** `createStage` returns an object with no module-level state, so a remount (leaving and re-entering the flow) starts clean.
6. **The whiteboard keeps a light palette in dark mode.** `stage/stage.css` defines its own `--s-*` tokens scoped under `.story-stage`, mirroring the light theme in `src/app.css`. The page text below the stage uses the app's theme tokens.
7. **Setup starts on Continue, not at launch.** The welcome step's copy says setup will run in the background, and the click that leaves that step starts it. Starting at launch was rejected: on a fresh Windows machine the first step can raise an elevation prompt for WSL2, which should not appear before the user has been told why.
8. **A replay never starts setup.** The `/intro` replay variant only walks through, as `docs/features/welcome-onboarding/02_dev.md` (Phase D) already requires.
9. **Finish stays gated on both steps.** A user who finishes the story before setup does waits on the setup cards, which show live progress. Letting the user leave early was rejected: the projects page would open with nothing to create a sandbox on.
10. **The activity center is reused, with two additions.** `ActivityCenter.svelte` takes an optional `headline` (a status summary that widens the bell into a labelled pill, with an optional coach mark and an optional action button in the panel) and a `placement` (`corner`, the default, or `inline` for a caller-positioned toggle whose panel opens downward). A separate onboarding-only progress widget was rejected: it would duplicate the task list and teach the user a surface that disappears after onboarding.
11. **The pill sits in a top bar during onboarding, not the bottom-right corner.** A light-tier run showed the corner pill covering the book's Next button. Toasts move to the top-left for the same reason while onboarding is incomplete.
12. **The book fits the viewport.** The stage's height is capped and the page text scrolls inside the card, so Back and Next never scroll out of view on a short window.
13. **Separate Paraglide files through path patterns.** `project.inlang/settings.json` lists `./messages/onboarding/{locale}.json` after `./messages/{locale}.json`; the message-format plugin merges them. Keys carry an `onboarding_` prefix so a later file can never silently override an app key. A second inlang project was rejected: it would need its own compile step and a second `m` object.
14. **No new host-global state.** The feature adds no path, port, bundled binary or environment variable, so the parallel-worktree rules in `e2e/AGENTS.md` are unaffected.

## Implemented: Phase A — Animation engine

`src/lib/onboarding/stage/`: `core.ts` (timing primitives and cancellation), `board.ts` (layers, actor registry, placement, caption callback), `characters.ts`, `computers.ts`, `tools.ts`, `screens.ts`, `arms.ts`, `effects.ts`, and `index.ts` (`createStage`, `StoryStage`, `Scene`). `stage.css` holds every drawing class under `.story-stage`, with keyframes prefixed `story-` so they cannot collide with the app's.

## Implemented: Phase B — Book kit and the first chapter

`src/lib/onboarding/book/`: `Book.svelte`, `Stage.svelte`, `Page.svelte`, `Chapter.svelte`, `Act.svelte`, `Term.svelte`, `Glossary.svelte`, `GlossaryItem.svelte`, `Callout.svelte`, `context.svelte.ts` (`BookState`) and `index.ts`.

`src/lib/onboarding/chapters/agents-and-sandboxes/`: `AgentsAndSandboxes.svelte` (all sixteen pages), `act1.scenes.ts` to `act5.scenes.ts` (the recap scene shares `act5.scenes.ts`, whose arrangement it reuses), and `copy.ts` (shared strings and the product names).

`src/lib/onboarding/chapters.ts`, `OnboardingBook.svelte`, and `src/lib/onboarding/AGENTS.md` (how to add a chapter and write a scene).

## Implemented: Phase C — Background setup

Backend: `TaskKind::MachineSetup` and `TaskKind::BaseImageBuild` in `src-tauri/src/activity/types.rs`; `prepare_machine` (`src-tauri/src/commands/machine.rs`) and `build_base_image` (`src-tauri/src/commands/image.rs`) each run inside `activity::scope`. `prepare_machine` still streams its checklist events to the caller's channel, mirrored into task progress by `activity::step_progress_channel`.

Frontend: `src/lib/onboarding/setup.svelte.ts` (`startBackgroundSetup`, `runMachineSetup`, `probeImage`, `runImageDownload`, `machineRun`, `imageRun`, `setupComplete`, `setupHeadline`). `CreateVmStep.svelte` and `DownloadNecessitiesStep.svelte` in `src/lib/welcome/` read the store instead of holding their own run state. The two task kinds are mirrored in `src/lib/activity/types.ts` and labelled in `src/lib/activity/activity.svelte.ts`.

## Implemented: Phase D — Activity center and welcome flow

`ActivityHeadline` in `src/lib/activity/types.ts`; the `headline` and `placement` props on `src/lib/activity/ActivityCenter.svelte`. `src/lib/welcome/WelcomeFlow.svelte` gains the `story` step, the sticky status bar, the skip link and the way back to the story from the setup cards. `src/routes/+layout.svelte` positions toasts top-left while onboarding is incomplete.

## Implemented: Phase E — Paraglide scaffold and light spec

`messages/onboarding/en.json`, `de.json` and `ru.json` with `onboarding_story_title`, read by `chapters.ts` as the chapter title; the second path pattern in `project.inlang/settings.json`.

`e2e/test/specs/light/onboarding-story-ui.spec.ts` proves the book registers sixteen pages and turns them, that background setup reaches its finished state and unlocks Finish, and that a failed image build turns the status pill to its attention state with a working jump to the setup cards. The mock backend answers instantly, so the running state is not observable in the light tier.

## Remaining: Phase F — Translations

Blocked on approval of the English copy. Enumerate the strings with a search for `TODO(i18n)` under `src/lib/onboarding/`, `src/lib/welcome/WelcomeFlow.svelte` and `src/lib/activity/`. Several markers cover a whole file (the scene scripts, `copy.ts`, the chapter component), not one line. Onboarding keys go into `messages/onboarding/{locale}.json` with the `onboarding_` prefix; the two task labels and the activity center's strings go into `messages/{locale}.json` beside the existing `task_*` and `activity_*` keys. Product names in `copy.ts` are not translated. Captions and labels drawn into the SVG are measured after drawing, so a longer translation widens its bubble or pill; check the chat bubbles and the harness pills in each locale.

## Lifecycle / Cross-Cutting Concerns

**Cancellation.** Every wait in a scene goes through the engine's `sleep`, `tween` or `clicked`. A page turn, a replay and the stage's unmount all cancel pending waits, and the rejection is swallowed by `Stage.svelte`.

**Setup state outlives the flow's steps.** The store is module-scoped, so moving between the story and the setup cards, or leaving and re-entering the route, never loses or restarts a run. A run that stops on a failure or a restart requirement is resumed from the setup cards, not automatically.

**Activity tasks for setup are not onboarding-specific.** Any caller of `prepare_machine` or `build_base_image` now produces a task and its completion toast.

**The standalone story is a second copy.** See Resolved Decision 2.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The status pill or a toast covers the book's Next button | The activity center or the toaster was put back in the bottom-right corner during onboarding | Keep `placement="inline"` in the welcome flow's top bar and the top-left toast position in `src/routes/+layout.svelte` |
| Back and Next scroll out of view on a short window | The book lost its height cap, or the stage lost its `max-height` | Keep the `max-h-*` class `WelcomeFlow.svelte` passes to `OnboardingBook` and the `max-height` on the SVG in `stage/stage.css` |
| Setup runs twice, or a card shows an idle button while setup is running | A setup card kept local busy state and called a command directly | Read `machineRun()` and `imageRun()` and start work only through `src/lib/onboarding/setup.svelte.ts` |
| A new onboarding key shows English in German or Russian | The key was added to `messages/onboarding/en.json` only | Mirror it into every file in `messages/onboarding/` |
| An onboarding key replaces an app string | Later path patterns override earlier ones for the same key | Keep the `onboarding_` prefix on every key in `messages/onboarding/` |
| The activity row for a setup task shows a blank label | A task kind was added in Rust without a label | Add the kind to `TaskKind` in `src/lib/activity/types.ts` and to `kindLabels` in `src/lib/activity/activity.svelte.ts` |
| A light spec cannot observe the "Setting up in the background" state | The mock backend resolves machine and image calls instantly | Assert the finished and attention states in `light`; observe the running state against real infrastructure |
| A story beat differs between the app and the standalone page | The standalone `onboarding-story/` copy was edited alone, or the app copy was | Change both, or retire the standalone copy |
