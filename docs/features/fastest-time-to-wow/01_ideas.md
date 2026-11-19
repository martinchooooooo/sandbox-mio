---
type: ideas
title: "Fastest Time to WOW"
feature: fastest-time-to-wow
---
# Fastest Time to WOW

This document covers a set of changes from the Expert UX review of first-run onboarding. The Expert's metric is "Fastest time to WOW": the first onboarding focuses entirely on running a sandbox locally, because that has the highest chance of a wow moment. Every step below is judged against that metric, and each place that costs time is recorded under Deviations from the Metric. Today the welcome flow (`src/lib/welcome/WelcomeFlow.svelte`) runs a hero, a welcome page, an illustrated tour and a two-card setup page, with machine and base-image setup already running in the background during the tour (`src/lib/onboarding/setup.svelte.ts`). Nothing in first run touches a local model, creates a sandbox or installs an agent; those are separate pages afterwards. The scope is: story-voiced setup copy, a consented local-model download in parallel with setup, an automatic first sandbox with pi, agent pre-install for later sandboxes, and one word for models.

## Current State

- **Welcome flow.** `WelcomeFlow.svelte` is a local step machine: hero, welcome, story, actions. Background setup starts when the user leaves the welcome page (`beginStory`), and only for the first-run variant, never for a replay. The finish button stays disabled until `setupComplete()`.
- **Setup store.** `setup.svelte.ts` runs the machine, then probes and builds the base image, as one module-scoped run shared with the status bar and two cards. It stops at the first step needing the user and reports through `setupHeadline()`.
- **Setup cards.** `CreateVmStep.svelte` ("Setting up your workspace") and `DownloadNecessitiesStep.svelte` ("Getting everything ready") already hide the technical sentence inside `AdvancedSection`. The visible parts still leak machine vocabulary: the machine step list (`src/lib/machine/MachineSteps.svelte`), the restart message, the stall message naming `mio.log`, and raw backend errors. Strings are in `messages/en.json` (`welcome_*`).
- **Tour vocabulary.** The tour (`src/lib/onboarding/chapters/agents-and-sandboxes/`) uses actors `you`, `chat`, `brain`, `pc`, `host`, `box`. Its copy and the stage helpers say "brain", and the chapter text is hard-coded English under `TODO(i18n)` markers.
- **Local models.** Hardware probe, ranking, download and start live on the Models page (`src/routes/models/+page.svelte`, `src/lib/models/recommend.ts`, `src/lib/models/catalogue.ts`). Ranking compares free RAM with each model's floor and rates it recommended, slow (within 2 GB under the floor) or wont-fit. The smallest catalogue entry needs 4 GB; the engines are described in `docs/architecture/inference.md`.
- **Sandbox creation.** `createSandbox` in `src/lib/sandbox/lifecycle.svelte.ts` calls the `create_sandbox` command with name, optional path, provider, proxy and network policy. It takes no agent list. Agents are installed afterwards from `src/routes/projects/AgentManager.svelte` through `install_agent`, then provisioned for local models with `provisionAgent` (`src/lib/agents/provision.ts`).
- **Agents and credentials.** The agent catalogue (`src/lib/agents/catalogue.ts`) has `claude-code` (credential service `anthropic`), `opencode` and `pi`. Install injects a stored login if one exists (`docs/architecture/agents.md`). pi's one model entry is labelled "Local LLM" (`src/lib/agents/pi.ts`).
- **Sandbox names.** The backend rejects names with spaces: letters, digits, `_`, `.`, `-`, starting with a letter or digit (`validate_name` in `src-tauri/src/backends/mod.rs`).

## Proposed Model

### The wow path

First run becomes one local-only line: a single heads-up screen before the tour, then the tour while setup and the model download run together in the background, then a sandbox called "hello world" created with pi installed and wired to the model. The wow moment is the user talking to a model running on their own computer inside a sandbox. The end of the tour hands over to that sandbox instead of an empty sandbox list. What a user without a usable local model gets is undecided; see Open Questions.

### Heads-up screen (consent)

One screen, one primary button, shown before the tour so that the download can run while the user reads. It replaces the current welcome page, which already announces background setup, and the user commits here instead of on the two-card setup page. It states in the tour's voice what will be downloaded, how large it is and why, and covers setup and model together so the user is asked once. It carries the low-RAM note when one applies. It is a deliberate cost; see Deviations from the Metric. Seam: a new step in the `WelcomeFlow.svelte` step machine, beside the existing `actions` step.

### Model download in the background

A model run joins the setup store as a second background run beside the machine and base-image run, started when the user confirms the heads-up, so it overlaps the tour. The hardware probe that picks the model, and the low-RAM check, run before the heads-up is shown; the probe is slow, so it starts as soon as the app opens. The setup page after the tour only reports progress and retries failures. The model comes from `recommendModels`: the top-ranked entry, no picker. The store reports it through `setupHeadline()` so the status bar shows one combined progress. Download uses the existing `download_model` command and start uses `start_inference` on the bundled llama.cpp engine; the Models page remains the place to change the model later.

### Low-RAM path

When no catalogue entry rates "recommended", the heads-up explains in plain words why a local model may be slow or fail (the computer's memory is small), and offers to try anyway with the smallest model. It never hard-blocks. The cutoff is derived from the catalogue floors through `ratingFor`, not a new constant. Seam: `recommendModels` and `ratingFor` in `src/lib/models/recommend.ts`.

### Setup copy in the tour's voice

The cards and the status bar continue the tour's analogies instead of introducing new nouns. Podman, WSL and virtual machine never appear in visible copy unless the tour has introduced them. Their detail moves into a "For those who know" section, the existing `AdvancedSection` seam, which gets that label and the real names. Structured errors need friendly variants too: the stall, restart-needed and failed-step messages are rewritten, and the raw backend message goes behind the same section.

### The hello-world sandbox

After setup and download finish, the user is told the first sandbox is being prepared and it is created through `createSandbox`. The visible name is "hello world"; the real name must satisfy `validate_name`, so it is stored as `hello-world` unless a display name is added. The sandbox is created with pi installed, provisioned against the running model, and opened on the pi chat pane (`docs/architecture/workspace-ui.md`).

### Agent pre-install at create

A post-create step in `runCreate` (`src/lib/sandbox/lifecycle.svelte.ts`) installs agents based on host state: pi when a model is downloaded, Claude Code when the credential store holds an Anthropic login. Claude Code applies to sandboxes created after hello world, not to first onboarding. The install and provision logic now lives inside `AgentManager.svelte`; it is extracted into a shared module so the create path and the manager call one implementation.

### One word: Model

"Model" is the single term. "Local LLM" and "AI Models" are removed from labels, including pi's model entry. "Brain" may appear at the start of onboarding, always as "Brain (AKA Model)" on first use, and never again after that. The work is a copy sweep of the hard-coded tour literals, `messages/*.json`, and the "Local LLM" entries in `src/lib/agents/pi.ts` and `src/lib/agents/opencode.ts`; stage actor names and code identifiers keep `brain` internally.

## Technology Choices with Tradeoffs

| Decision | Chosen | Alternatives and why rejected |
|---|---|---|
| When the model download starts | At the start of the tour, after a one-screen heads-up, in parallel with setup | Silent start during the tour: rejected by the Expert; a large download uses the user's bandwidth and disk. Start after the tour: rejected on the metric, since the tour's reading time is free download time. Start after setup finishes: rejected, since it serialises two long waits. |
| Which model first run uses | Top entry of `recommendModels`, no picker | A picker screen: adds a decision and a step before the wow. Always the smallest model: wastes capable machines and weakens the wow. |
| Engine for first run | Bundled llama.cpp | Foundry Local: needs a winget install of a separate CLI before any model runs, an extra step and a failure surface on the critical path. It stays available on the Models page. |
| How agents get into the new sandbox | Frontend post-create step reusing `install_agent` and `provisionAgent` | New `create_sandbox` agent parameter: provisioning and the catalogue are frontend code, so the backend would need a second copy. Baking pi into the base image: removes an install from the critical path, but the agent would no longer be recorded in `mise.toml` and reproduced on remote deploy (`docs/architecture/agents.md`). |
| Technical detail placement | Existing `AdvancedSection`, relabelled "For those who know" | A separate details page: costs a click for people who want it and adds a screen. Deleting the detail: hides facts from the users the Expert wants served. |
| First sandbox name | `hello-world` | "hello world" literally: the backend rejects the space. Loosening `validate_name`: affects every provider and path conversion for a cosmetic gain. |
| Host-global state | One inference server at a time on a free loopback port | A per-sandbox server: not supported by the engines and memory-prohibitive. Because the server is host-global, real-model runs cannot be isolated per worktree and belong to the host-locked e2e tier; the fast tier uses the mock backend (`e2e/AGENTS.md`). |

## Deviations from the Metric

Each of these costs time before the wow. Sergie is sad about all of them; each is kept only for the reason given.

| Deviation | Why kept | Mitigation |
|---|---|---|
| Heads-up screen before the download | The download is large and uses the user's bandwidth and disk | One screen, one button, setup and model covered together |
| Low-RAM explanation and "try anyway" | A user whose machine cannot run the model deserves to know why it is slow | Shown on the heads-up screen, not a separate one |
| Illustrated tour before the hello-world sandbox | It teaches the vocabulary the later copy relies on | Setup and the model download both run while the tour plays, so reading time is not waiting time |
| Claude Code pre-install | Useful for users with a login | Moved out of first onboarding; later sandboxes only |
| Installing pi in the first sandbox | Needed for the wow | Install can overlap with the model download tail |

## Validation

Not validated. No spike has run. The numbers a spike must produce before the dev plan fixes the flow: the download size and time of the smallest and the top-ranked catalogue model, the base-image build time, how much the two overlap when started together, and the time from sandbox creation to the first pi reply. These form the baseline for the metric. A spike should be offered to the user before `02_dev.md`.

## Open Questions

- **OQ-wow-moment:** Which event ends the clock for "time to wow": the first pi reply, the sandbox opening, or something earlier? The metric cannot be reported without it.
- **OQ-heads-up-placement:** Leaning before the tour, so the download overlaps it (the Expert asked to download during the tutorial and to check with the user first). Open: does the heads-up replace the welcome page or sit between it and the tour?
- **OQ-bandwidth-contention:** The base-image build and the model download both use the network. Do they start together, or does the smaller image build go first so the sandbox is ready earlier? Resolved in `02_dev.md`: they start together, in parallel.
- **OQ-tour-shorter-than-download:** The tour may finish before the download does. What does the user see then: a progress page in the tour's voice, or a shortened tour?
- **OQ-ram-basis:** `ratingFor` compares free RAM, which on a busy first-run machine is lower than total RAM. Should the first-run threshold use total RAM?
- **OQ-no-local-model:** What does a user get who declines the download or cannot run any model? Candidates: the hello-world sandbox with no agent, or a pointer to a cloud login. The Expert asked for local-first, so this needs an explicit decision.
- **OQ-hello-world-special:** Is hello world an ordinary, deletable sandbox, or does it have special treatment, and what happens if the name is taken?
- **OQ-skipped-tour:** The tour has a skip link. Does a skipping user reach the same heads-up screen, and does background setup still start?
- **OQ-replay:** The replay variant must not trigger setup, download or sandbox creation. Does it show the heads-up screen at all?
- **OQ-brain-scope:** Is "Brain (AKA Model)" allowed only in the tour, or also on the heads-up screen where the user first decides on a Model?
- **OQ-claude-code-trigger:** For later sandboxes, is Claude Code installed whenever an Anthropic login exists, or does the user opt in at create time?
- **OQ-pi-ready-order:** If the model download is still running when the sandbox is ready, does the user wait on the sandbox, or enter it with pi pending?

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Creating the sandbox as "hello world" fails | `validate_name` rejects spaces | Use `hello-world`, or add a display name |
| The setup store shows the wrong progress after the model run is added | `setupHeadline()` and the two cards each assume machine then image | Add the model run to the store and read it only through the store |
| The download starts for a user who skipped the tour or never confirmed | The start is wired to the tour's first page instead of the consent button | Start only from the heads-up's confirm action, and never from a skip or a replay |
| The replay starts a download | Background start is gated on the variant, and a new step could bypass the gate | Gate every new start on `variant === "onboarding"` as `beginStory` does |
| pi installs but cannot reach a model | pi is provisioned against the running server, and none is running yet | Start the model first, or re-provision after start (`reprovisionInstalledAgents`) |
| Visible copy still says Podman or WSL | Errors and step names come from the machine layer | Map structured error kinds to tour-voiced text and put the raw text under "For those who know" |
| A copy sweep misses a term | "Brain" and "Local LLM" appear in literals, locale files and agent config | Search all three places; keep code identifiers unchanged |
