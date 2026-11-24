---
type: dev-plan
title: "Local Models UX: Build Plan"
feature: local-models-ux
---
# Local Models UX: Build Plan

This document is the build plan for the Models page rework and the re-homing of the runtime teaching steps. It continues [01_ideas.md](01_ideas.md) and supersedes it wherever the two differ; the departures are listed in "Departures from 01_ideas.md". The work is frontend-only: no Tauri command, Rust code, host path, host port or environment variable is added. After review the Models page is deliberately sparse: it renders no "For those who know" disclosure, hardware list, model facts, engine, endpoint, wake steps or Try it box, and "Download anyway" is a plain link; the first-run onboarding card keeps its "For those who know" disclosure with the wake steps. Status: all phases (A to F) are implemented, and the copy is in Paraglide.

## The Concrete Model

The Models page (`src/routes/models/+page.svelte`) becomes a column of three sections in the dashboard rhythm of `src/lib/workspace/DashboardOverview.svelte`, titled "Models" to match the sidebar label `nav_models`:

1. **Your Model** (`src/lib/models/CurrentModelCard.svelte`): the Model that is awake, or else the one the page would wake. It shows the brain avatar (`src/lib/models/ModelAvatar.svelte`), the name, the maker chip, the one-line summary, one status line in the tour's voice, one primary button, and a green border with a green dot on the status line while awake. There is no test box and no disclosure; the endpoint is exposed to tests as the `data-base-url` attribute.
2. **Your computer** (`src/lib/models/ComputerSummary.svelte`): one plain sentence about memory and graphics, with a small re-detect icon button beside the title. The CPU, RAM, GPU and NPU list is removed.
3. **Other Models** (`src/lib/models/ModelShelf.svelte`): every other catalogue model as a `src/lib/models/ModelCard.svelte`, grouped under "Runs well here", "Runs here, slowly" and "Needs more than this computer has". An empty group is not rendered.

All job state (download progress, wake-up events, serving status, last error) lives in one module-scoped store, `src/lib/models/model.svelte.ts`, shared by the Models page and the first-run onboarding (`src/lib/onboarding/setup.svelte.ts`). Progress therefore survives navigation, and both places narrate a wake-up with the same component, `src/lib/models/WakeUpStatus.svelte`.

Model information comes from curated catalogue fields (`src/lib/models/types.ts`, `src/lib/models/catalogue.ts`) plus English copy in `src/lib/models/copy.ts`. Two pure functions turn numbers into plain language: `fitFor()` in `src/lib/models/recommend.ts` (will it run here, and why not) and `describeModel()` in `src/lib/models/describe.ts` (size, expected speed, room needed). `ratingFor()` and `recommendModels()` keep their exact contract because `modelOffer()` in `src/lib/onboarding/setup.svelte.ts` depends on them.

Plain-language explanations use one shared pinnable hover card, `src/lib/components/Explainer.svelte`, generalised from `src/lib/agent-chat/RoleExplainer.svelte`.

The Settings page keeps the engine picker; the static checklist moves from the bottom of `src/routes/settings/+page.svelte` into the "tuning" `AdvancedSection`, directly under the engine buttons, and the engine buttons get plain labels.

### Card anatomy

| Part | Your Model card | Shelf card |
|---|---|---|
| Avatar and name | `ModelAvatar.svelte` plus `descriptor.label` | Same |
| Maker chip | Swatch in the `MAKERS` colour (`src/lib/onboarding/chapters/agents-and-sandboxes/copy.ts`) plus the maker name | Same |
| Summary | `MODEL_SUMMARIES[id]` from `src/lib/models/copy.ts` | Same |
| Good-for tags | Labels from `GOOD_FOR_LABELS` | Same |
| Scales | Size, expected speed, room needed, each with an `Explainer` | Same; speed is omitted on a card in "Needs more" |
| Fit line | The `fitFor()` sentence when the verdict is not "fits" | The `fitFor()` sentence for every verdict except "fits" |
| "Our pick for this computer" badge | When the card is the pick, with a "Why this one?" `Explainer` | When the card is the pick |
| Status line | `WakeUpStatus.svelte` while waking; otherwise "Not downloaded yet", "Downloading… 42%" with `Progress`, "Asleep", "Awake: your sandboxes can use it" | "Downloading… 42%" with `Progress` while downloading |
| Primary action | "Download and wake up", "Wake it up" or "Put it to sleep"; "Try anyway" when the verdict is not "fits" | "Use this Model" when the verdict is "fits" or "tight"; none in "Needs more" |
| Failure detail | The failed line plus the raw error beneath it in small muted text | The failed line plus the raw error; a "Download anyway" link in "Needs more" |

"Use this Model" downloads when needed, then wakes the model; `start_inference` (`src-tauri/src/commands/inference.rs`) already stops a prior server before starting another, so switching needs no extra stop.

### Choosing the Your Model card

`currentEntry()` in `src/lib/models/model.svelte.ts` picks, in order: the model of the job in flight (download or wake); else the model the server is serving; else the first downloaded model in ranking order that the engine can serve; else the pick. The pick is `pickFor(specs, engine, testModel)` in `src/lib/models/recommend.ts`: the test model when `MIO_TEST_MODEL` is set, else the first `recommendModels()` entry rated "recommended" that the engine can serve, else `null`. With no pick, the card falls back to the smallest model the engine can serve, shows its fit line, and labels the button "Try anyway", matching the onboarding's low-RAM offer in `modelOffer()`.

### Fit verdicts

`fitFor(specs, descriptor, engine)` returns `{ verdict, group, freeGb, totalGb, needGb }`.

| Verdict | Condition | Group | Card sentence (draft) |
|---|---|---|---|
| `fits` | `ratingFor()` is "recommended" | Runs well here | "Runs well on this computer." |
| `tight` | `ratingFor()` is "slow" | Runs here, slowly | "Runs here, but slowly. Closing other apps helps." |
| `busy` | `ratingFor()` is "wont-fit" and total memory is enough (see Gotchas on the 1 GB tolerance) | Needs more | "Needs about {need} GB free; about {free} GB is free right now. Close some apps, then check again." |
| `too-small` | Total memory below the floor | Needs more | "Needs a computer with at least {need} GB of memory. This one has {total} GB." |
| `engine` | Engine is Foundry and `foundryAlias` is null | Needs more | "Not available with Microsoft Foundry Local. The built-in engine can run it." |

`engine` takes precedence over the memory verdicts. Figures are whole GB, rounded to nearest.

### Plain scales

`describeModel(descriptor, specs, fit)` returns three labels; thresholds are constants at the top of `src/lib/models/describe.ts`.

- **Size** from `paramsB`: under 5 small, under 15 medium, under 25 large, otherwise very large.
- **Expected speed** from `activeParamsB`: up to 4 quick, up to 12 steady, otherwise slow; one step faster when `specs.gpuNames` is non-empty; one step slower when the verdict is `tight`; omitted in the "Needs more" group. Always worded "expected".
- **Room needed**: download size from `llama.sizeBytes` and memory from `ramFloorBytes`, both whole GB.

### Wake-up narration

`WakeUpStatus.svelte` folds `RuntimeSetupEvent`s (`src/lib/models/types.ts`) the same way `RuntimeSteps.svelte` does and shows one line: the plain title (`PLAIN_STEP_TITLES` in `src/lib/models/copy.ts`) of the step currently running, "Waking it up…" before the first event, "Ready ✓" on `ready`, and "That did not go through." with a "Try again" button on `stepFailed`, and the card shows the raw error beneath it.

## Resolved Decisions

| Question | Resolution |
|---|---|
| OQ-cant-run-download | A card in "Needs more than this computer has" has no primary button; it shows a plain "Download anyway" link that calls `downloadModel()` in `src/lib/models/model.svelte.ts`. |
| OQ-editorial-ownership | The implementer of Phase A writes the summaries, tags and size buckets into `src/lib/models/copy.ts` and `catalogue.ts`, each checked against the model's card; the user approves the first set in Phase F before Paraglide. After that, whoever adds a catalogue model writes them in the same change as the llama.cpp architecture check. |
| OQ-use-case-chooser | No chooser. Good-for tags are passive labels on every card. |
| OQ-speed-basis | Heuristic speed from `describeModel()`, always labelled "expected". Measured speed stays in [04_improvements.md](04_improvements.md). |
| OQ-ram-wording | Whole GB, rounded to nearest, with "right now" in every sentence about free memory. The group still follows `ratingFor()` on raw bytes. |
| OQ-brain-on-models-page | `ModelAvatar.svelte` draws the brain disc in the `--brain` token; visible text says "Model"; only the "What is a Model?" explainer says "the Brain from the tour", once. |
| OQ-maker-identity | A maker chip: a round swatch in the `MAKERS` `fill` with a 1px border, plus the name; no logos. A model whose maker is not in `MAKERS` (the test model) shows no chip. |
| OQ-try-it | Superseded after review: the test box is removed. The Your Model card keeps only the line "Your sandboxes' agents use this Model." with a link to the `hello-world` sandbox when that sandbox exists. |
| OQ-engine-home | The engine picker stays in Settings. The checklist moves under it. The Models page has no link to the engine setting. |
| OQ-shared-store | Extracted in this feature (Phase C), because progress lost on navigation is part of the inconsistency. The onboarding keeps its `ModelRun` shape and flags; only the download and wake-up calls move. |
| OQ-onboarding-narration | Yes: the first-run model card in `src/lib/welcome/SetupProgress.svelte` renders `WakeUpStatus.svelte` in place of its fixed "Waking it up…" line (Phase C). |
| OQ-foundry-presentation | A model without a Foundry alias goes into "Needs more than this computer has" with the `engine` verdict. |
| Validation: copy review | Folded into Phase F: the user reviews the running page rather than a static mock. |
| Validation: deterministic fit groups | No probe mock. The light spec asserts structure that holds on any machine (every card has `data-fit`, its group matches it, a "Needs more" card has no `data-start`). A mocked probe is recorded in [04_improvements.md](04_improvements.md). |
| Parallel-worktree constraints | None apply: no new path, host port, host-global resource, bundled binary, teardown or harness environment variable. The store is per-window frontend state, and the inference server stays the one host-global server already covered by `e2e/test/specs/heavy/local-llm.spec.ts`. |

## Departures from 01_ideas.md

- **No `toolCalling` field.** Every catalogue GGUF supports tool calling, and the one Foundry model without it (gpt-oss) already has `foundryAlias: null`, so the field would carry no information. Add it when a catalogue entry first needs it.
- **No `summary` key on `ModelDescriptor`.** Summaries live in `MODEL_SUMMARIES` in `src/lib/models/copy.ts`, keyed by model id, so all copy sits in one file for the Paraglide move; the catalogue stays data.
- **The shelf excludes the Your Model card's model** and is titled "Other Models", so one model never appears twice.
- **The avatar is not maker-coloured.** The disc keeps the `--brain` token, as in the chat; the maker colour appears only in the chip. Google's `MAKERS` fill is white, which a disc cannot carry.

## Superseded Approaches

- **"Choose a different model" disclosure** (`docs/features/desktop-app-feel/02_dev.md`, Phase D) hid every other model and the hardware card behind an `AdvancedSection` so a first-time user saw one button. It is removed because choosing a model is the page's primary decision. The one-button default survives as the Your Model card's primary action; do not re-wrap the shelf in a disclosure.
- **Auto-collapsing checklist** on the Models page (`stepsCollapsed`, `collapsedOnce` and the two-second timer in `src/routes/models/+page.svelte`, with its underlined "View details" button) is removed. The Models page no longer renders the checklist at all (the Settings page and the onboarding card do), and the visible narration is `WakeUpStatus.svelte`.
- **Unicode step glyphs and blue running colour** in `RuntimeSteps.svelte` are replaced by HugeIcons and the theme's status colours.
- **Page-local job state** (`downloadPct`, `startingModel`, `setupEvents` and the action runner in the route) is replaced by `src/lib/models/model.svelte.ts`.

## Implemented: Phase A: Catalogue Metadata and Pure Helpers

- `src/lib/models/types.ts`: add to `ModelDescriptor` the fields `maker: string | null` (a `MAKERS` name), `goodFor: GoodFor[]` and `activeParamsB: number`. Add the union `GoodFor` (`quick-replies`, `everyday-chat`, `coding-agent`, `careful-thinking`, `long-documents`), the union `FitVerdict` and the union `FitGroup` (`runs-well`, `runs-slowly`, `needs-more`).
- `src/lib/models/catalogue.ts`: fill the three fields on all eight `MODELS` entries from the draft table in [01_ideas.md](01_ideas.md), "Catalogue metadata a non-technical user needs" (`activeParamsB` equals `paramsB` except 3 for `qwen3.6-35b-a3b`). `testModelFromEnv()` returns `maker: null`, `goodFor: []`, `activeParamsB: 0.1`. Extend the comment above `MODELS` to say a new entry also needs its copy in `src/lib/models/copy.ts`.
- `src/lib/models/copy.ts` (new, `TODO(i18n)` header like `src/lib/agent-chat/anatomy-copy.ts`): `MODEL_SUMMARIES`, `GOOD_FOR_LABELS`, size and speed labels, `FIT_SENTENCES` (functions taking the GB figures), `PLAIN_STEP_TITLES` for all ten step ids in `RuntimeSteps.svelte`'s `titles` map, `EXPLAINERS` (title and body for "What is a Model?", memory, download size, expected speed, "Our pick", "Why can't this computer run it?"), and the page's section titles, status lines and button labels.
- `src/lib/models/recommend.ts`: add `fitFor()`, `pickFor()` and a small `engineServes()` helper as specified above; `fitFor()` derives `fits`/`tight` from `ratingFor()` so the two can never disagree. `ratingFor()` and `recommendModels()` are unchanged.
- `src/lib/models/describe.ts` (new): `describeModel()` and the threshold constants. `copy.ts` also exposes `fitSentence(fit)` and `ENGINE_NAMES`.
- Verification: `pnpm check` (svelte-check). The repository has no unit-test runner; the pure helpers are exercised by the light spec in Phase D.

## Implemented: Phase B: Shared Explainer

- `src/lib/components/Explainer.svelte` (new): the hover-card body of `src/lib/agent-chat/RoleExplainer.svelte` with props `title` and `body` (the trigger is the info icon), passing other props (`data-*` hooks) to the trigger button and a `contentProps` record to `HoverCard.Content`.
- `src/lib/agent-chat/RoleExplainer.svelte`: becomes a thin wrapper that reads `ROLE_COPY[role]` and passes `data-anatomy-info`, `data-anatomy-explainer` and `data-explainer-role` through, so the chat's existing hooks and e2e selectors keep working.
- Models-page explainers carry `data-explainer={key}` on the trigger and `data-explainer-card={key}` on the content.

## Implemented: Phase C: Shared Model Store and Onboarding Narration

- `src/lib/models/model.svelte.ts` (new), module-scoped `$state`:
  - State: `specs`, `entries` (ranked descriptors with the test model prepended), `downloaded`, `status` (`InferenceStatus`), `engine`, `downloadPct` (record by model id), `wake` (`{ modelId, steps, events }`), `busy` (`"download" | "wake" | "sleep" | "redetect" | null`), `jobModelId` (the Model the current or last job acts on; scopes `error` to its card) and `error` (parsed message of the last failed job, or the busy message when a job was refused). `sleep()` keeps the checklist steps and clears only the events; `loadModels()` re-fetches the steps whenever no wake is in flight.
  - `loadModels()`: reads `get_settings`, `detect_machine_specs`, `test_model`, `downloaded_models` and `inference_status`; safe to call on every page mount.
  - `redetect()`: calls `redetect_machine_specs`.
  - `downloadModel(descriptor)`: the body of today's `downloadModel()` in `src/routes/models/+page.svelte`; re-reads `downloaded_models` afterwards and resolves to whether the model is now on disk.
  - `wakeModel(descriptor, engine)`: loads `inference_setup_steps`, then, unless `inference_status` already reports this target running (the reuse rule in `runModelSetup()`), calls `start_inference` with a real `Channel` feeding `wake.events`; re-reads `inference_status`, throws when the target is not running, then fires `reprovisionInstalledAgents()` (`src/lib/agents/reprovision.ts`) without awaiting it, so Ready is not delayed.
  - `downloadAndWake(descriptor)`, `sleep()` (calls `stop_inference`), `currentEntry()`, and `infoOf(descriptor)` (fit verdict plus plain scales, in place of a bare `fitOf`), `needsDownload()`. `downloadModel()` takes an optional progress callback, which the onboarding uses to mirror the percentage.
  - One job at a time: every action returns early while `busy` is non-null, so the onboarding's job and a Models-page click cannot interleave.
- `src/lib/onboarding/setup.svelte.ts`: `runModelSetup()` calls `downloadModel()` and `wakeModel(descriptor, "llama")` from the store and mirrors `downloadPct` into its own `model.downloadPct`; it keeps `recordMilestone()`, the `ModelRun` flags, `skipModelSetup()` and `retryModelSetup()`. Its own trailing `reprovisionInstalledAgents()` call is removed because `wakeModel()` makes it; the sandbox job's re-point after pi installs (`startFirstSandbox()`) stays. `probeMachineSpecs()` and `modelOffer()` are unchanged: the onboarding keeps its early free-memory snapshot.
- `src/lib/models/WakeUpStatus.svelte` (new): reads `wake` from the store. It shows the failure line only; retrying is the Your Model card's primary button, so it has no button of its own.
- `src/lib/welcome/SetupProgress.svelte`: the `model.starting` branch renders `WakeUpStatus.svelte` in place of the fixed "Waking it up…" line; `RuntimeSteps.svelte` with the store's steps and events goes into the model card's existing "For those who know".
- Verification: the onboarding light specs under `e2e/test/specs/light/` that drive the first-run model card still pass.

## Implemented: Phase D: Models Page Rework

- `src/routes/models/+page.svelte`: rewritten as a thin layout over the store: an `h1` "Models", one sentence of introduction with the "What is a Model?" explainer, then `CurrentModelCard`, `ComputerSummary` and `ModelShelf` in a `stagger` column. It calls `loadModels()` on mount and holds no job state.
- New components in `src/lib/models/`: `CurrentModelCard.svelte`, `ComputerSummary.svelte`, `ModelShelf.svelte`, `ModelCard.svelte` (built on `src/lib/components/ui/item/`), `ModelAvatar.svelte`, `MakerChip.svelte`, `ModelScales.svelte` (the three scales with explainers).
- `src/lib/models/RuntimeSteps.svelte`: glyphs become HugeIcons (`@hugeicons/core-free-icons`), the running state uses a spinning icon in `text-muted-foreground`, and the blue colour is removed. Titles and explanations stay the Paraglide `runtime_step_*` keys, because the checklist is shown on the Settings page and in the onboarding card.
- Shared pieces beyond that list: `ModelDetails.svelte` (the part both card kinds share), `ModelExplainer.svelte` (an `Explainer` keyed into `EXPLAINERS`) and `DownloadProgress.svelte`. The hardware list and `TryIt.svelte` are removed, and so are their Paraglide keys (`models_cpu_cores`, `models_ram`, `models_gpu`, `models_no_gpu`, `models_npu`); Phase F deletes the remaining `models_*` keys that no longer render.
- E2E hooks, all kept by name so `e2e/test/pageobjects/models.page.ts` changes minimally:

| Hook | New home |
|---|---|
| `data-model-id` | Root of the Your Model card and of every shelf card |
| `data-rating` | The fit label on every card (non-empty text for every verdict) |
| `data-fit`, `data-fit-group` (new) | `data-fit` on every card root; `data-fit-group` on each shelf group |
| `data-start`, `data-stop` (new) | `data-start` on Your Model's wake button and every "Use this Model" button; `data-stop` on Your Model's sleep button (replaces the `button=Stop` text selector) |
| `data-running` | Root of the Your Model card while awake |
| `data-base-url` | The attribute `data-base-url={url}` on the Your Model card root while awake |
| `data-downloaded` | A marker element inside each card root, the Your Model card included, when the model is on disk |
| `data-specs`, `data-specs-ready`, `data-redetect` | `ComputerSummary.svelte`; `data-specs-ready` is on the summary sentence and `data-redetect` on the icon button beside the title; `data-specs` also carries `data-total-ram`, `data-free-ram` and `data-cpu-cores` with the raw numbers |
| `data-chat`, `data-reply` | Removed with `TryIt.svelte` |
| `data-wake-details` (new) | Removed; the Models page no longer renders `RuntimeSteps.svelte` |
| `data-model-advanced` | Removed |

- `e2e/test/pageobjects/models.page.ts`: `open()` drops `#expandAdvanced()` and expands nothing; `baseUrl()` reads the `data-base-url` attribute; `stopInference()` clicks `[data-stop]`; `stepTitles()`, `chatPanel` and `isChatShown()` are removed (the Settings page object still covers the checklist titles); `startDownload()` keeps its `[data-start]` fallback.
- `e2e/test/specs/light/local-llm-ui.spec.ts`: the memory assertion reads the `data-total-ram` attribute on `[data-specs]` (with `data-free-ram` and `data-cpu-cores`, exposed through `specsFigures()` in the page object) and the "GB" in the summary sentence, because the visible text no longer lists hardware; add the structural fit assertions from Resolved Decisions and an assertion that every visible card has a non-empty summary. Full coverage is planned in `03_e2e.md`.
- `src/lib/agent-chat/ChatView.svelte`: the banner link text "Start a model" becomes "Wake up the Model" (`TODO(i18n)` literal).

## Implemented: Phase E: Settings Re-Home

- `src/routes/settings/+page.svelte`: move the `RuntimeSteps` section from the bottom of the page into the "tuning" `AdvancedSection`, directly after the engine buttons, under the heading "What waking a Model involves with this engine". The engine buttons read "Built in: works on most computers" and "Microsoft Foundry Local: can use the NPU on Copilot+ PCs", each followed by a muted line naming llama.cpp with Vulkan, or Foundry Local. `save()` and its round-trip of `get_settings` are unchanged.
- `e2e/test/pageobjects/settings.page.ts`: `stepTitles()` calls `#expand(this.tuningTrigger)` first, because the checklist now sits inside a closed-by-default disclosure.

## Implemented: Phase F: Copy Approval and Translations

- The user approved the copy and asked for the feature to be finished. The running page was reviewed: summaries, good-for tags, size buckets, fit sentences, plain step titles and explainers. Each sentence is checked against the model's card.
- Per the `feature-implement` skill: moved every `TODO(i18n)` literal from `src/lib/models/copy.ts` and the new components into Paraglide keys in `messages/en.json`, `messages/de.json` and `messages/ru.json`. `src/lib/models/copy.ts` now exposes functions and getters over `m.*` so text follows the locale. Deleted the `models_*` keys that no longer render, including the superseded `settings_engine_llama` and `settings_engine_foundry` (for example `models_title`, `models_engine_label`, `models_memory_warning`, `models_view_details`, `models_hide_details`, `models_foundry_first_run`) and `settings_runtime_explainer`). The onboarding card `src/lib/welcome/SetupProgress.svelte` keeps its page-wide `TODO(i18n)` marker: its literals predate this feature and belong to `docs/features/fastest-time-to-wow/`.
- Updated `docs/architecture/inference.md` to state that one frontend store owns the model job and that the setup steps are shown as plain narration with the technical plan on demand.

## Lifecycle and Cross-Cutting Concerns

- **State derivation.** The store re-reads `inference_status` and `downloaded_models` on every `loadModels()`, so a server started or stopped elsewhere (onboarding, a restart) is reflected on the next visit; no polling is added.
- **One job at a time.** `busy` is shared by the Models page and the onboarding. While the first-run job runs, the Models page shows its progress and disables every action rather than queueing a second job.
- **Wake events lifetime.** `wake.events` is cleared when a new wake starts and on `sleep()`; a finished wake keeps its events so the onboarding card can still show the checklist.
- **Agent re-pointing.** Exactly one `reprovisionInstalledAgents()` call per successful wake, inside `wakeModel()`.
- **Free memory drift.** Groups follow live free memory; a card can change group between visits. The page re-reads specs on mount only, never on a timer.
- **Copy.** Visible text never names llama.cpp, Vulkan, RAM, endpoint or GGUF; those appear only in the onboarding "For those who know" disclosure and on the Settings page, per `docs/features/fastest-time-to-wow/01_ideas.md`.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A 32 GB computer is told it "needs a computer with at least 32 GB" | `ramFloorBytes` is 32 GiB while the probe's total is slightly lower after firmware and GPU reservations | `fitFor()` treats total memory within 1 GiB below the floor as enough (`busy`, not `too-small`) |
| The first-run offer changes after a ranking edit | `modelOffer()` in `src/lib/onboarding/setup.svelte.ts` calls `recommendModels()` | Put new logic in `fitFor()`/`pickFor()`; never change `ratingFor()` or the order without a decision recorded against `docs/features/fastest-time-to-wow/02_dev.md` |
| `baseUrl()` or `specsText()` returns an empty string in e2e | WebdriverIO `getText()` returns "" for an element inside a closed `AdvancedSection` | Expand the disclosure in the page object, or read a value attribute such as `data-base-url` |
| `waitForDownloaded()` never resolves | `row.$("[data-downloaded]")` searches descendants, not the row element itself | Render `data-downloaded` on a child element of the card root, never on the root |
| The chat's learning-mode spec loses its explainer | `RoleExplainer.svelte` was refactored and the `data-anatomy-*` hooks were dropped | Pass `data-anatomy-info`, `data-anatomy-explainer` and `data-explainer-role` through `Explainer.svelte` |
| Agents are re-pointed twice or not at all after a wake | `runModelSetup()` kept its own `reprovisionInstalledAgents()` call, or `wakeModel()` skipped it on the reuse path | Re-point once, inside `wakeModel()`, on both the start and the reuse path |
| Waking the same model again drops every sandbox's forward | `start_inference` stops the prior server even when it serves the same model | `wakeModel()` checks `inference_status` first and reuses a server already serving the target |
| A card renders a blank summary or tag | A `MODELS` entry or the `testModelFromEnv()` descriptor lacks a new field or a `MODEL_SUMMARIES` entry | Give every new field a default in `testModelFromEnv()`; the light spec asserts non-empty summaries for catalogue models |
| A new step id shows a blank status line | `PLAIN_STEP_TITLES` maps step ids by hand, like `titles` in `RuntimeSteps.svelte` | Add the plain title with the Paraglide title in the same change |
| Google's maker swatch is invisible on a light theme | Google's `MAKERS` fill is white | The swatch carries a 1px border in the `border` token |
| Saving from the moved Settings checklist resets other settings | `set_settings` replaces the whole file | Keep `save()`'s full round-trip of the object read from `get_settings` |
| Progress disappears after leaving the Models page | Job state put back into a route-level `$state` | Keep job state in `src/lib/models/model.svelte.ts`, per the root `AGENTS.md` gotcha on route-local stores |
| Visible copy names llama.cpp, Vulkan or RAM | Copy reused from `runtime_step_*_explain` or `models_*` keys | Technical names appear only in the onboarding "For those who know" disclosure and on the Settings page |
