---
type: ideas
title: "Local Models UX"
feature: local-models-ux
---
# Local Models UX

This document scopes a rework of the Models page (`src/routes/models/+page.svelte`) and of the runtime teaching steps (`src/lib/models/RuntimeSteps.svelte`, shown on the Models page and at the bottom of `src/routes/settings/+page.svelte`). The recent onboarding work (`docs/features/onboarding-story/`, `docs/features/fastest-time-to-wow/`, `docs/features/chat-agent-anatomy/`) set a voice, a vocabulary and a set of components for non-technical users; the Models page predates all three and still reads as an engineer's control panel. The brief has three parts: the page feels different from the rest of the app, the teaching steps feel bolted on, and the model list gives a non-technical user no way to tell models apart or to understand a model the computer cannot run. The work is frontend-only: the catalogue (`src/lib/models/catalogue.ts`, `src/lib/models/types.ts`), the ranking (`src/lib/models/recommend.ts`), the page, and the settings section. The inference engines, the download and start commands and the sandbox forwards (`docs/architecture/inference.md`) stay unchanged. The user was not available during ideation, so every decision that needs the user is an Open Question with a recommendation.

## Current State

### The Models page today

- **Layout.** `src/routes/models/+page.svelte` renders an `h1` "Local Models" (`models_title`, while the sidebar says "Models", `nav_models`), a subtitle "Engine: llama" (`models_engine_label`), the top-ranked model as one `Card.Root` row, and an `AdvancedSection` labelled "Choose a different model" (a `TODO(i18n)` literal) holding the hardware card, every other model row and a raw test box (`docs/features/desktop-app-feel/02_dev.md`, Phase D).
- **A model row** (`modelRow` snippet) shows the catalogue `label`, a coloured text rating ("Recommended", "Will run slowly", "Won't fit"), a running line with "Endpoint:" and the server's base URL in `<code>`, and Download, Start or Stop buttons. A non-recommended row adds "This model's RAM floor exceeds your free RAM; it may run slowly or fail to load." With Foundry selected, a row without a Foundry alias adds the literal "— (llama only)".
- **The hardware card** lists CPU cores, free and total RAM in GB, GPU names and NPU presence, with a re-detect icon button (`data-specs`, `data-redetect`).
- **The test box** (`data-chat`) is a `Textarea` that posts one request to the server's chat-completions URL with a 256-token cap and prints the reply as plain text. It only renders while a server runs, inside the collapsed disclosure.
- **State is component-local.** Download percentages, the starting model, setup events and the test reply are `$state` inside the route, so leaving the page drops them; the root `AGENTS.md` Common Gotchas names this pattern as a bug source. The download still shows in the activity center.
- **The chat sends users here.** When the server is down, the native chat's banner links to `/models` ("Start a model", `agent_chat_start_model` in `src/lib/agent-chat/ChatView.svelte`), so the page is also a recovery destination.

### The teaching steps today

- **Origin.** `docs/features/local-llm/02_dev.md`, Phase F2, made the feature "informative by design": each engine declares ordered setup step ids (`inference_setup_steps`), and `RuntimeSteps.svelte` renders them with a Paraglide title and explanation per id (`runtime_step_<id>_title`, `runtime_step_<id>_explain` in `messages/en.json`).
- **Two renderings.** Live on the Models page during start, ticking each row with Unicode glyphs (○ ◐ ✓ ↷ ✗) in emerald, blue or destructive text, then auto-collapsing after two seconds behind a bespoke underlined "View details" button. Static at the bottom of Settings, under "What starting this runtime involves:", below the Save button and the resource usage card, outside any disclosure.
- **Copy.** The explanations name Vulkan, `llama-server`, execution providers, Intel OpenVINO, winget and "an OpenAI-compatible endpoint". The onboarding introduces none of these words.
- **Onboarding narrates the same start differently.** The first-run model card (`src/lib/welcome/SetupProgress.svelte`) says "Downloading… 42%" with a `Progress` bar, then "Waking it up…", then "Ready ✓", and passes an empty `Channel` to `start_inference` in `runModelSetup()` (`src/lib/onboarding/setup.svelte.ts`), discarding the step events.

### The catalogue and ranking today

- **`ModelDescriptor`** (`src/lib/models/types.ts`) carries `id`, `label`, `paramsB`, `ramFloorBytes`, the GGUF source with `sizeBytes`, `foundryAlias`, `contextWindow` and `reasoning`. It carries no maker, description, use, speed or quality information. Facts a user would care about live only in code comments in `catalogue.ts`: the 35B-A3B entry is a mixture of experts that runs faster per token than the dense 27B, and Foundry's gpt-oss lacks tool calling.
- **Ranking.** `recommendModels()` rates each model by free RAM against `ramFloorBytes` (`ratingFor()`: recommended, slow within 2 GB under the floor, wont-fit below that), then orders recommended first and largest first. GPU presence and total RAM play no part. `modelOffer()` in `src/lib/onboarding/setup.svelte.ts` uses the same ranking for the first-run offer, so a ranking change also changes onboarding.
- **Probe.** `detect_machine_specs` (`src-tauri/src/commands/inference.rs`) caches the static fields for the process lifetime and re-reads RAM through `sysinfo` on every call (`docs/features/models-page-caching/02_dev.md`). The probe does not branch on `MIO_MOCK_BACKEND`, so the light e2e tier sees the host's real memory.

### What "the rest of the app" now looks like

| Pattern | Where it lives | What it establishes |
|---|---|---|
| Tour voice and analogies | `src/lib/onboarding/chapters/agents-and-sandboxes/AgentsAndSandboxes.svelte`, `src/lib/welcome/SetupProgress.svelte` | Short plain sentences; "a pretend computer"; "Waking it up…"; "Ready ✓"; friendly failure text with "Try again" |
| One word: Model | `docs/features/fastest-time-to-wow/01_ideas.md`, "One word: Model" | "Model" is the only visible term; "Brain (AKA Model)" appears at the start of onboarding |
| "For those who know" | `AdvancedSection.svelte` with that label in `HeadsUpStep.svelte` and `SetupProgress.svelte` | Real names (llama.cpp, Podman, file names) and raw backend errors sit behind one disclosure; visible text never names them |
| Brain and Arms identity | `--brain` and `--arms` tokens in `src/app.css`, `src/lib/agent-chat/AgentAvatar.svelte` | A model is drawn as a brain-coloured disc with a brain icon |
| Pinnable explainers | `src/lib/agent-chat/RoleExplainer.svelte` with copy in `anatomy-copy.ts` | An info icon opens a hover card with a title and two or three sentences; a click pins it |
| Many brains, many makers | "There are many brains" page and `MAKERS` in `src/lib/onboarding/chapters/agents-and-sandboxes/copy.ts` | Models are introduced by maker with a maker colour; "some are quicker, some think more carefully" |
| Book kit | `src/lib/onboarding/book/` (`Term`, `Callout`, `Glossary`) | Reusable highlighted terms and boxed asides |
| Module-scoped job stores | `src/lib/onboarding/setup.svelte.ts` (`modelRun()`) | A long job survives navigation and every surface reads one source |
| Dashboard rhythm | `src/lib/workspace/DashboardOverview.svelte` | A `stagger` column of plain sections, primary action first, detail on demand |

## Diagnosis: Where the Models Page Diverges

1. **Vocabulary.** "Local Models", "Engine: llama", "Endpoint", "RAM floor", "free RAM", "GPU layers", "Vulkan" and "OpenAI-compatible" appear in visible text. The fastest-time-to-wow rule puts real names behind "For those who know"; the Models page puts them in the default view.
2. **Inverted disclosure.** Choosing a model, the one decision a non-technical user makes on the page, sits behind a collapsed disclosure styled as "Advanced", while the endpoint URL (a developer fact) shows in the default view.
3. **The teaching steps speak to engineers, twice.** The checklist explains engine internals in jargon, uses glyphs and a blue colour found nowhere else, and collapses behind its own underlined toggle instead of `AdvancedSection`. On Settings the same list floats below Save with no link to the engine choice it describes. Onboarding narrates the identical start as one friendly status line, so a user meets two narrations of one event.
4. **No identity for a model.** A row is a bare label like "Qwen3.6 35B-A3B". The onboarding taught that models come from makers and differ in speed and care, but the page shows neither the maker nor any difference beyond a RAM verdict.
5. **Verdicts without reasons.** "Won't fit" and the RAM-floor warning say the model cannot run here, but not what the model would offer, why it cannot run, or what would change that: closing other apps when total memory would suffice, or a computer with more memory when it would not.
6. **A secondary row can download what it cannot run.** Secondary rows offer Download regardless of rating, so a user can fetch a 22 GB file rated "Won't fit".
7. **Bare test box.** The test box is a textarea and plain text reply, while the native chat now renders replies with the AI Elements components and Brain avatars.
8. **State resets on navigation.** The page holds its job state locally, unlike the onboarding's module-scoped `modelRun()`, so a user who leaves mid-start returns to a page that no longer shows the start.

## Proposed Model

### Page structure: Your Model, Your computer, The shelf

The page becomes a column of three sections in the dashboard rhythm, titled "Models" to match the sidebar.

- **Your Model.** One card at the top for the Model that is serving, or the one that would be picked. It shows a model avatar (the brain disc from `AgentAvatar.svelte` in the maker's colour), the name, the one-line summary, and one status line in tour voice: "Not downloaded yet", "Downloading… 42%" with a `Progress` bar, "Waking it up…", "Awake: your sandboxes can use it", "Asleep". One primary button follows the state ("Download and wake up", "Wake it up", "Put it to sleep"). "For those who know" holds the engine, the GGUF file name, the endpoint URL and the context size. Seam: a new `src/lib/models/CurrentModelCard.svelte`.
- **Your computer.** One sentence in human terms ("This computer has 16 GB of memory; about 9 GB is free right now. No graphics chip that helps with Models was found.") with an explainer on "memory". CPU cores, GPU names, NPU and re-detect move into "For those who know". Seam: a new `src/lib/models/ComputerSummary.svelte` reading `MachineSpecs`.
- **The shelf.** Every catalogue model, visible by default and grouped by fit on this computer (next sections). Seam: a new `src/lib/models/ModelCard.svelte` built on the `item` component (`src/lib/components/ui/item/`), replacing the `modelRow` snippet.

The "Choose a different model" disclosure goes away: choosing is a primary action, so the shelf is always shown, and only technical detail is disclosed.

### Catalogue metadata a non-technical user needs

`ModelDescriptor` in `src/lib/models/types.ts` gains curated fields, filled in `src/lib/models/catalogue.ts` beside the existing llama.cpp architecture check:

| Field | Meaning | Source |
|---|---|---|
| `maker` | Who built the Model, keyed to the onboarding's `MAKERS` list for its colour | Curated |
| `summary` | One plain sentence: what the Model is like to use | Curated copy |
| `goodFor` | Tags from a fixed list: quick replies, everyday chat, coding with an agent, careful thinking, long documents | Curated, checked against the model card |
| `activeParamsB` | Parameters used per token; equals `paramsB` except for a mixture of experts | Model card |
| `toolCalling` | Whether agents can use it for tools; promotes today's code comment into data | Model card and engine check |

Copy fields hold keys into a new `src/lib/models/copy.ts` of English literals under `TODO(i18n)`, the pattern `src/lib/agent-chat/anatomy-copy.ts` and the onboarding chapter use, and move to Paraglide after the user approves the copy (`feature-implement` skill). `testModelFromEnv()` in `catalogue.ts` must supply neutral defaults for every new field.

A draft of the curated values, grounded only in facts the catalogue already holds; every sentence must be checked against the model's card before it ships (see Open Question "OQ-editorial-ownership"):

| Model | Maker | Size | Draft summary | Draft good for |
|---|---|---|---|---|
| Qwen3.5 2B | Alibaba | Small | The smallest and quickest. Fine for simple questions; makes more mistakes on bigger jobs. | Quick replies |
| Qwen3.5 4B | Alibaba | Small | Small and quick, and steadier than the 2B. | Quick replies, everyday chat |
| Qwen3.5 9B | Alibaba | Medium | A good all-rounder for a computer with 16 GB of memory. | Everyday chat, coding with an agent, careful thinking |
| Gemma 4 12B | Google | Medium | Google's mid-sized open Model. | Everyday chat, careful thinking |
| GPT-OSS 20B | OpenAI | Large | OpenAI's open Model that runs on your own computer. | Everyday chat, careful thinking |
| Devstral Small 2 24B | Mistral | Large | Mistral's Model made for coding agents; answers without thinking out loud. | Coding with an agent |
| Qwen3.8 27B | Alibaba | Very large | The most capable Model on the shelf, and the slowest. | Coding with an agent, careful thinking, long documents |
| Qwen3.6 35B-A3B | Alibaba | Very large | A very large Model that only wakes a small part of itself per word, so it answers faster than its size suggests. Biggest download. | Coding with an agent, careful thinking |

### Tradeoffs in human terms

A pure function `describeModel(descriptor, specs, engine)` in a new `src/lib/models/describe.ts` turns catalogue numbers into three labelled scales, each with an explainer:

- **Size** (small, medium, large, very large), bucketed from `paramsB`. The explainer ties size to the onboarding: a bigger Model has read more and makes fewer mistakes, but needs more memory and answers more slowly.
- **Speed on this computer** (quick, steady, slow), estimated from `activeParamsB`, whether a GPU was detected, and the fit verdict. Labelled "expected" because it is a heuristic, not a measurement (see Open Question "OQ-speed-basis").
- **Room it needs**: download size and memory, in GB, with an everyday comparison in the explainer ("about as big as a two-hour film").

Raw numbers (parameters, context window, quantisation, file name) stay under "For those who know" on each card.

### Fit with a reason

A pure function `fitFor(specs, descriptor, engine)` in `src/lib/models/recommend.ts` returns a verdict plus the numbers behind it. `ratingFor()` and `recommendModels()` keep their contract, because `modelOffer()` in `src/lib/onboarding/setup.svelte.ts` depends on both.

| Reason | Condition | What the card says |
|---|---|---|
| Fits | Free memory at or above the floor | "Runs well on this computer" |
| Tight | Free memory within the existing 2 GB band under the floor | "Runs here, slowly. Closing other apps helps." |
| Busy | Free memory too low, total memory enough | "Needs about 16 GB free; about 9 GB is free now. Close some apps, then check again." |
| Too small | Total memory below the floor | "Needs a computer with at least 32 GB of memory. This one has 16 GB." |
| Not on this engine | Foundry selected and `foundryAlias` is null | "Not available with Foundry Local. The built-in engine can run it." |

The shelf groups cards into "Runs well here", "Runs here, slowly" and "Needs more than this computer has". A card in the last group keeps its full description, scales and good-for tags, so the user still learns what the Model would offer; its primary button is replaced by the reason (see Open Question "OQ-cant-run-download").

### Picking for the user

The existing pick, the first entry of `recommendModels()`, is marked "Our pick for this computer" with a "Why this one?" explainer ("It is the most capable Model that fits comfortably in the memory this computer has free."). It is the default in "Your Model" when nothing is downloaded, which is what the page does today. Good-for tags stay passive labels; a use-case chooser is Open Question "OQ-use-case-chooser".

### Teaching steps, re-homed

The step data stays (engine-owned ids, as `docs/features/local-llm/02_dev.md` Phase F2 decided); only its presentation changes.

- **Visible narration.** While a Model wakes, "Your Model" shows one tour-voiced line per step from a new plain title per step id ("Looking for a graphics chip that can help…", "Bringing the Model into memory…", "Checking it answers…"), the same pattern as the onboarding's "Waking it up…". Seam: a new `src/lib/models/WakeUpStatus.svelte` folding `RuntimeSetupEvent`s exactly as `RuntimeSteps.svelte` does.
- **Full checklist under "For those who know".** `RuntimeSteps.svelte` renders inside that disclosure with the current technical titles and explanations. The Unicode glyphs become HugeIcons with the theme's status colours, and the bespoke "View details" toggle is removed in favour of `AdvancedSection`.
- **Explainers on demand.** `RoleExplainer.svelte` is generalised into a shared `src/lib/components/Explainer.svelte` (title, body, pinnable hover card); the chat keeps using it through its role copy. The Models page uses it for "memory", "download size", "speed", "Our pick" and "Why can't this computer run it?".
- **Settings.** The static checklist moves from the bottom of `src/routes/settings/+page.svelte` into the engine disclosure, directly under the engine picker, introduced as "What waking a Model involves with this engine". The engine options get plain labels ("Built in: works on most computers" and "Microsoft Foundry Local: can use the NPU on Copilot+ PCs"), with llama.cpp, Vulkan and winget in "For those who know".
- **Same narration in onboarding.** `runModelSetup()` in `src/lib/onboarding/setup.svelte.ts` can pass a real `Channel` and render `WakeUpStatus.svelte` in the first-run model card, so both places narrate one event the same way.

### One store for the Model job

A module-scoped store, a new `src/lib/models/model.svelte.ts`, owns download, wake-up, put-to-sleep, progress and step events. The Models page and `runModelSetup()` in `src/lib/onboarding/setup.svelte.ts` both call it, so progress survives navigation and the download and start logic exists once. The onboarding keeps its own `modelRun()` shape as a view over the shared store. See Open Question "OQ-shared-store" for the scope risk.

### Trying the Model

The test box becomes a "Try it" area inside "Your Model", visible whenever the Model is awake rather than behind a disclosure. Replies render with the AI Elements `message` and `bubble` components (`src/lib/components/ai-elements/`) beside the brain avatar, and a line points to where the Model is really used: "Your sandboxes' agents use this Model. Open hello-world to see it at work." See Open Question "OQ-try-it".

### Copy rules for the page

- "Model" is the visible term; "Local Models", "LLM" and "AI models" go.
- Status lines follow `SetupProgress.svelte`: present-tense progress with an ellipsis, "Ready ✓", failures as "That did not go through." plus "Try again", with the raw backend message under "For those who know".
- Memory is "memory" in visible text and "RAM" under "For those who know".
- New visible strings start as English literals under `TODO(i18n)`, as the onboarding and learning-mode copy did.

## Technology Choices with Tradeoffs

| Decision | Chosen | Alternatives and why rejected |
|---|---|---|
| Source of model descriptions | Curated fields in `catalogue.ts`, copy in `src/lib/models/copy.ts` then Paraglide | Fetch Hugging Face model cards at runtime: needs the network on a page that must work offline, returns English marketing text, and changes without review. Generate descriptions with the local Model: needs a running Model before the user has chosen one, and is not deterministic. Leave descriptions out: fails the brief. |
| How tradeoffs are shown | Three labelled scales with explainers; raw numbers under "For those who know" | Raw parameters, GB and context sizes only: today's state, meaningless to the target user. Benchmark scores: hard to source per quantised GGUF, not plain language, and invite unsubstantiated claims. Star ratings: imply a precision the curation does not have. |
| Speed signal | Heuristic from `activeParamsB`, GPU presence and fit, labelled "expected" | Measure tokens per second on this computer after the first wake-up: accurate but only after a download, so it cannot guide the choice; deferred to `04_improvements.md`. No speed signal: hides the main reason to pick a small Model. |
| Grouping | By fit on this computer, with good-for tags | By use case: with eight models most tags overlap, and the user's first question is "will it run?". By maker: teaches branding, not fitness. Flat ranked list: today's state, which hides why a model ranks low. |
| Models this computer cannot run | Shown with full description and a reason, no primary download | Hide them: the brief asks to explain what they would offer. Keep a plain Download button: lets a user fetch tens of GB that will not load (Diagnosis 6). |
| Fit explanation | New `fitFor()` beside the unchanged `ratingFor()` | Change `ratingFor()` to use total memory: shifts the first-run offer, which `docs/features/fastest-time-to-wow/02_dev.md` settled on free memory (OQ-ram-basis). |
| Teaching steps | Tour-voiced status line, technical checklist under "For those who know", explainers on demand | A new onboarding book chapter on local Models: lengthens first run against the fastest-time-to-wow metric, and the book cannot be opened at a given page from elsewhere (`docs/features/chat-agent-anatomy/04_improvements.md`). A live story-stage animation of the wake-up: the engine exists (`src/lib/story-stage/`) but needs new actors and a driver for a ten-second event; deferred to `04_improvements.md`. Delete the steps: drops the "informative by design" intent of `docs/features/local-llm/02_dev.md` Phase F2. |
| Explainer component | Generalise `RoleExplainer.svelte` into a shared `Explainer.svelte` | Copy it into `src/lib/models/`: two pinnable hover cards drift apart. `tooltip`: no pinning, too small for two sentences, and unreachable by touch. |
| Settings explainer | Move it under the engine picker | Leave it below Save: the out-of-place feeling the brief names. Delete it: the engine choice then has no explanation. Move the whole engine section to the Models page: `set_settings` replaces the whole file, so a second writer must round-trip every field (`AppSettings` comment in `src/routes/settings/+page.svelte`); Open Question "OQ-engine-home". |
| Job state | Module-scoped `src/lib/models/model.svelte.ts` shared with onboarding | Keep page-local state: progress vanishes on navigation and duplicates `runModelSetup()`. Read progress from the activity center: it carries task progress but not step events or the served model id. |
| Trying the Model | Inline "Try it" with AI Elements, visible when awake | Open the `hello-world` chat instead: needs that sandbox to exist, run and hold pi, which a user who skipped onboarding lacks. Remove the test: loses the only check that works without a sandbox. |
| Host-global state | None added | The inference server stays the one host-global server (`docs/architecture/inference.md`). Everything here is frontend logic over existing commands, so the light tier (`MIO_MOCK_BACKEND=1`) covers it, except the real wake-up already in `e2e/test/specs/heavy/local-llm.spec.ts`. |

## Validation

Not validated; no spike has run. Two cheap checks should run before `02_dev.md`:

- **Copy review.** Draft the summary, tags and scale for each of the eight catalogue models from their model cards and show them to the user on a static mock of one card in each fit group. This settles "OQ-editorial-ownership" and the tone.
- **Deterministic fit groups in the light tier.** `detect_machine_specs` returns the host's real RAM, so whether a light spec sees all three groups depends on the machine. Confirm whether a frontend-only override (beside `testModelFromEnv()`) or a mock seam for the probe is needed for `03_e2e.md`.

## Open Questions

- **OQ-cant-run-download:** Should a model in "Needs more than this computer has" offer any download? Recommendation: no primary button; a "Download anyway" link under "For those who know" for users who plan to free memory or move the file.
- **OQ-editorial-ownership:** Who writes and approves each model's summary, tags and size bucket, and when? Recommendation: whoever adds a model writes them in the same change as the llama.cpp architecture check in `catalogue.ts`, and the user approves the first set.
- **OQ-use-case-chooser:** Should the page ask "What do you want to do?" and re-rank, or only show good-for tags? Recommendation: tags only; eight models do not need a chooser, and every agent in the app is a coding agent.
- **OQ-speed-basis:** Is a heuristic speed label acceptable, or should speed appear only once measured? Recommendation: heuristic labelled "expected", with measurement deferred.
- **OQ-ram-wording:** Should the fit line show exact free memory, which changes between visits and can move a card between groups, or round it? Recommendation: round to whole GB and say "right now".
- **OQ-brain-on-models-page:** The one-word rule keeps "Brain" to the start of onboarding, while learning mode labels parts "Brain · thinking" everywhere. May the Models page use the brain avatar and the "bigger brain" analogy in explainers? Recommendation: brain avatar yes; visible text says "Model"; an explainer may say "the Model (the Brain from the tour)" once.
- **OQ-maker-identity:** Show the maker as a name with the onboarding's maker colour, or name only? Recommendation: name plus the colour from `MAKERS` in `copy.ts`, no logos, matching the book's monogram rule.
- **OQ-try-it:** Inline "Try it" or a link into the `hello-world` chat? Recommendation: inline, plus a pointer to `hello-world` when that sandbox has pi.
- **OQ-engine-home:** Does the engine picker stay in Settings or move to the Models page? Recommendation: stay in Settings for this feature, with the checklist moved under it and a link from the Models page's "For those who know".
- **OQ-shared-store:** Extract the shared model store in this feature or in a follow-up? Recommendation: in this feature, because progress lost on navigation is part of the inconsistency; the onboarding's model card is the regression risk and needs its light spec run.
- **OQ-onboarding-narration:** Should the first-run model card adopt `WakeUpStatus.svelte` too? Recommendation: yes, in the same feature, so the two narrations of one event match.
- **OQ-foundry-presentation:** With Foundry selected, do models without an alias sit in the "Needs more" group with the "Not on this engine" reason, or in their own group? Recommendation: the "Needs more" group with that reason; Foundry is a niche engine.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The light spec "renders the host-specs panel" fails after the rework | `e2e/test/specs/light/local-llm-ui.spec.ts` asserts the specs text matches "RAM", and the visible copy now says "memory" with RAM under "For those who know" | Point `specsText()` in `e2e/test/pageobjects/models.page.ts` at the "For those who know" hardware list, or update the assertion in the same change |
| `models.page.ts` cannot find `[data-model-advanced]` | The "Choose a different model" disclosure is removed; `open()` in `e2e/test/pageobjects/models.page.ts` expands it first | Keep the `data-model-id`, `data-rating`, `data-start`, `data-running`, `data-base-url`, `data-downloaded`, `data-specs` hooks on the new components and drop the disclosure step from `open()` |
| The heavy spec's step-title check finds no rows | `stepTitles()` in `models.page.ts` reads `[data-step-id] [data-step-title]`, which now sits in a collapsed "For those who know" | Expand that disclosure in the page object before reading, as `settings.page.ts` does for its sections |
| The first-run offer changes after a ranking edit | `modelOffer()` in `src/lib/onboarding/setup.svelte.ts` calls `recommendModels()` | Add `fitFor()` beside `ratingFor()`; never change the rating or order without a decision recorded against `docs/features/fastest-time-to-wow/02_dev.md` |
| A card renders a blank summary or tag | A catalogue entry, or the `MIO_TEST_MODEL` descriptor from `testModelFromEnv()`, lacks a new field | Give every new `ModelDescriptor` field a default in `testModelFromEnv()` and check every `MODELS` entry when adding a field |
| A card jumps between fit groups between visits | Free memory is re-read on every `detect_machine_specs` call | Round the displayed figure and keep the group verdict on the same free-memory rule as `ratingFor()` |
| A new step id shows an empty plain status line | Plain titles map step ids to copy by hand, like the title and explain maps in `RuntimeSteps.svelte` | Add the plain title for every id in `inference_setup_steps` in the same change, and assert non-empty plain titles in the light spec |
| Progress disappears when the user leaves the Models page mid-download | Job state held in the route's `$state` is destroyed on navigation | Keep job state in `src/lib/models/model.svelte.ts`, per the root `AGENTS.md` gotcha on route-local stores |
| Saving from a new inference control un-onboards the user | `set_settings` replaces the whole settings file and omitted fields return to defaults | Round-trip the full object read from `get_settings`, as `save()` in `src/routes/settings/+page.svelte` does |
| Visible copy names llama.cpp, Vulkan or RAM | Technical copy was reused from `runtime_step_*_explain` or `models_*` keys | Keep real names under "For those who know" only, per `docs/features/fastest-time-to-wow/01_ideas.md` |
