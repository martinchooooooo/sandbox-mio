---
type: dev-plan
title: "Fastest Time to WOW: Build Plan"
feature: fastest-time-to-wow
---
# Fastest Time to WOW: Build Plan

This is the build plan for the first-run onboarding changes in [01_ideas.md](01_ideas.md): a consented local-model download that runs during the tour, setup copy in the tour's voice, a first sandbox called "hello world" with pi, agent pre-install for later sandboxes, and one word for models. Status: Phases B to H are implemented and typecheck; Phase A (the baseline spike) and Phase I (translations, after the user approves the English flow) remain.

The governing metric is "Fastest time to WOW". Where a step costs time before the wow, the plan says "Sergie is sad" and names the cost.

## The Concrete Model

First run is one line the user walks through once.

1. **Hero.** Unchanged.
2. **Heads-up (replaces the welcome page).** One screen, one primary button. It says in the tour's voice that Sandbox mio will prepare a computer-brain (Brain, AKA Model) and a place to run it, how large the download is, and that the tour can be read while it happens. When the machine is short on memory it carries the low-RAM note and an explicit "Try anyway". Confirming starts everything. Sergie is sad: this screen is a step before any progress, kept because the download uses the user's bandwidth and disk.
3. **Tour.** The existing illustrated tour, with the first-run store running three jobs behind it: machine, base image and model. The model job downloads the chosen model and starts the llama.cpp server. The status bar shows one combined headline.
4. **Progress page (the current two-card setup page, rewritten).** Shown when the tour ends or is skipped. It reports the three jobs in the tour's voice, offers retry on failure, and moves on automatically when all are done. Sergie is sad if the user reaches it before the jobs finish; that wait is the main cost of the whole flow.
5. **Hello world.** The app tells the user it is preparing their first sandbox, creates it as `hello-world`, installs pi, points pi at the running model, and opens the pi chat pane. The wow moment is the first reply from pi.

After first run, sandbox creation pre-installs agents from host state: pi when a model has been downloaded, Claude Code when the credential store holds an Anthropic login. Claude Code is excluded from first-run onboarding. Sergie is sad if it is ever added to the first-run line, since it is a cloud path and slows the local one.

One word, "Model", is used everywhere. "Brain" appears only in the tour and on the heads-up screen, as "Brain (AKA Model)" on its first use there and never later.

## Resolved Decisions

Each Open Question from 01_ideas.md, with its resolution.

| Question | Resolution |
|---|---|
| OQ-wow-moment | The clock ends at the first pi reply shown in the hello-world chat pane. Proposed here; the Expert confirms. The clock starts at app first open, recorded by the instrumentation in Phase H. |
| OQ-heads-up-placement | The heads-up replaces the welcome step and comes before the tour, so the download overlaps the tour. Implemented as a new step in `src/lib/welcome/WelcomeFlow.svelte`. |
| OQ-bandwidth-contention | Decided by the Expert: the model download starts together with machine and image setup and runs in parallel with it. The machine and image still chain with each other as today. The spike measures the overlap to report it, not to choose between orders. |
| OQ-tour-shorter-than-download | The progress page waits, shows combined progress in the tour's voice, and advances on its own. There is no shortened tour. |
| OQ-ram-basis | The rating stays on free RAM through `ratingFor` in `src/lib/models/recommend.ts`. The probe runs when the app opens, before the app starts any heavy job, so free RAM is as high as it will be. |
| OQ-no-local-model | Declining the download, or a model or engine failure, never blocks onboarding: hello-world is still created, with no agent installed, and the user is told why. Whether that outcome also points at a cloud login stays open for the Expert. |
| OQ-hello-world-special | Hello world is an ordinary sandbox the user can rename or delete. If the name `hello-world` already exists, onboarding creates nothing and opens the existing sandbox. |
| OQ-skipped-tour | Consent is given before the tour, so skipping changes nothing: the skip link goes to the progress page and the jobs are already running. |
| OQ-replay | The replay variant omits the heads-up and the progress page's start actions, and starts nothing, as `beginStory` already does for setup. |
| OQ-brain-scope | "Brain (AKA Model)" is allowed in the tour and on the heads-up screen only. |
| OQ-claude-code-trigger | Automatic: any sandbox created after first run gets Claude Code when the credential store holds an Anthropic login. There is no opt-in. |
| OQ-pi-ready-order | The user never enters the sandbox with pi pending. The progress page waits for the model to be running and pi to be provisioned. |

## Remaining: Phase A: Baseline Spike

Measure before building, so every later phase can be judged against the metric. Throwaway scripts and captured output go in `docs/features/fastest-time-to-wow/spike/`, with a markdown record carrying `type: spike`.

- Download size and time of the smallest and the top-ranked entry in `src/lib/models/catalogue.ts`.
- Base-image build time on a cold machine.
- The overlap: how much the parallel start slows each job compared with running it alone. The parallel start is already decided (OQ-bandwidth-contention); a large slowdown is reported to the Expert, not acted on.
- Time from `create_sandbox` to a running pi reply, including the install step.
- Whether a sandbox returned by `create_sandbox` is already running when `install_agent` is called (Phase D depends on it).

## Implemented: Phase B: Terminology Sweep

Copy only; no behaviour change. English literals first, translations after approval (Phase I).

- Replace "Local LLM" and "AI Models" with "Model" in `src/lib/agents/pi.ts` (the model name constant), `src/lib/agents/opencode.ts` and `src/lib/agents/local-llm.ts`, and in every matching string in `messages/en.json`.
- In the tour at `src/lib/onboarding/chapters/agents-and-sandboxes/` (`AgentsAndSandboxes.svelte`, `copy.ts`, the `act*.scenes.ts` captions), use "Brain (AKA Model)" on first use and "Model" afterwards. Stage actor names, `LOCAL_LLM_MODEL_NAME`-style identifiers and file names keep their current spelling.
- The tour keeps its own nouns: the first page is titled "Meet the Brain (AKA Model)" and later pages say "Model". The Models page nav label and the chat model label now read "Models" and "Model" in `messages/en.json` only; the other locale files keep their old wording until Phase I.
- Search three places so nothing survives: hard-coded literals under `src/`, `messages/*.json`, and the agent configuration the app writes. The model name pi writes is user-visible, so existing sandboxes keep the old label until re-provisioned; `reprovisionInstalledAgents` in `src/lib/agents/reprovision.ts` already refreshes it at startup.

## Implemented: Phase C: Shared Agent Install

Extract the install-and-provision logic out of `src/routes/projects/AgentManager.svelte` into a module both the manager and the create path call.

- New `src/lib/agents/install.ts` (`installAgentInSandbox`): a function that, given a sandbox name, workspace path and agent descriptor, runs `install_agent` with the descriptor's packages, npm post-install, credential service and onboarding seed, then runs `provisionAgent` from `src/lib/agents/provision.ts` against the running model from `inference_status`. It accepts an optional progress callback so the manager keeps its install log.
- `AgentManager.svelte` becomes a caller; its behaviour and install log stay the same.

## Implemented: Phase D: Pre-install on Create

- In `runCreate` in `src/lib/sandbox/lifecycle.svelte.ts`, after `create_sandbox` returns and before `openSandbox`, run a best-effort pre-install step using Phase C.
- Verified from `services::sandbox::create` that the sandbox is started before `create_sandbox` returns, so `install_agent` can run immediately (Phase A item, resolved by reading the code, not measured).
- A pure planning function decides (`planPreinstall` in `src/lib/agents/preinstall.ts`) which agents to install: pi when `downloaded_models` is non-empty; Claude Code when `list_credential_bundles` contains the `anthropic` service. It lives beside the catalogue in `src/lib/agents/` and takes both facts as arguments so it is unit-testable without IPC.
- A pre-install failure never fails the create: it is toasted, the sandbox stays, and the Manage tab still offers the manual install.
- `CreateArgs` gains `agents`, an explicit agent id list that replaces planning; first-run onboarding passes `["pi"]` or `[]`, so Claude Code never appears. `provider` became optional so the backend resolves the default provider.
- The step text feeds the existing `createStep` reader so the sidebar placeholder shows "Installing pi".

## Implemented: Phase E: First-Run Model Job

Add a third job to the first-run store `src/lib/onboarding/setup.svelte.ts`, read only through the store, as the file's existing gotcha requires.

- State: choice (which model, or a low-RAM "try anyway" flag), download progress, start status, error.
- Choice: `recommendModels` over the specs from `detect_machine_specs`. The top entry when it rates "recommended"; otherwise the entry with the smallest `ramFloorBytes`, reached only after the user chose "Try anyway".
- Job: skip the download when `downloaded_models` already lists the model; otherwise `download_model` with the entry's GGUF source; then `start_inference` on the llama engine; then `reprovisionInstalledAgents` as the Models page does. The download already reports itself as a `ModelDownload` activity, which the activity center shows.
- `setupHeadline()` reports one combined line across the three jobs. A new `firstSandboxReady()` accessor is true when machine, image and model are all done. The existing `setupComplete()` keeps its meaning.
- The store function that starts the model job is called only from the heads-up's confirm action, never from the tour's first page, a skip or a replay.
- The probe call is moved earlier: the layout's startup block in `src/routes/+layout.svelte` calls `detect_machine_specs` for the navigation panel once on mount, so the cached result is ready when the heads-up opens.

## Implemented: Phase F: Heads-Up Screen

- A new step `consent` in the step machine of `WelcomeFlow.svelte`, replacing `welcome`; a new component under `src/lib/welcome/`.
- Content: what will be prepared, the download size from the chosen entry's `sizeBytes`, the tour-while-you-wait promise, and "For those who know" with the real names (model file, engine). The low-RAM variant replaces the primary text with a plain-language explanation and a "Try anyway" button next to the smaller-model offer.
- Confirm calls `startBackgroundSetup` and the Phase E model job together. Replay does not render this step. Beyond the plan, a "Set up without a Model" link (`skipModelSetup`) implements OQ-no-local-model: it starts machine and image only, and the first sandbox is created with no agent.

## Implemented: Phase G: Setup Copy in the Tour's Voice

- Rewrite `CreateVmStep.svelte` and `DownloadNecessitiesStep.svelte` into one progress page that renders the three jobs with the tour's nouns. Retire the title "Setting up your workspace".
- Implemented as `src/lib/welcome/SetupProgress.svelte` (the two old step components are deleted). `MachineSteps.svelte` labels are unchanged but render only inside "For those who know", so the visible text never names Podman or WSL; a restart is described without naming WSL.
- Relabel `AdvancedSection` usage to "For those who know" and put Podman, WSL, virtual machine and the raw backend message there. The visible text never names them.
- Map the structured failure kinds the machine layer returns (`PodmanUnresponsive`, reboot required, generic failure) to tour-voiced messages; the restart message says the computer needs a restart to finish, with no mention of WSL. Step labels in `src/lib/machine/MachineSteps.svelte` get the same treatment.
- The status-bar strings in `setupHeadline()` follow the same vocabulary.

## Implemented: Phase H: Hello-World Handoff and Instrumentation

- When `firstSandboxReady()` becomes true on the progress page, show the "preparing your first sandbox, hello world" message and call `createSandbox` with the name `hello-world`, the default provider, and the pre-install list `[pi]` when the model job succeeded, or none when it did not.
- If `hello-world` exists, skip creation and open it.
- After the sandbox is created and pi is provisioned, call `completeOnboarding`, then open the sandbox and its chat pane (`open_agent_chat` in `src/lib/workspace/apps.ts`). The finish button is replaced by this automatic handoff; the user can still cancel to the sandbox list.
- Handoff lives in `src/lib/onboarding/handoff.svelte.ts`; the progress page starts it when `firstSandboxReady()` turns true, and a "Go to my sandbox list" button cancels the automatic open.
- Instrumentation (`src/lib/onboarding/milestones.ts`, Rust `log_milestone`): the first pi reply is not recorded, because the native chat renders it outside the webview code this plan touches; the other milestones are. Record in-memory milestones (app first open, heads-up confirmed, model downloaded, server started, sandbox created, pi installed, first pi reply) and write them to the application log through the existing tracing path on the Rust side, with a new thin command if none exists. This feeds the metric; it holds no user data.

## Deviation: First Sandbox in the Background Setup

Phase H originally created `hello-world` and installed pi only after `firstSandboxReady()` turned true, which left the progress page waiting on the pi install. The slow part now runs in the background setup.

- The first-sandbox job lives in `src/lib/onboarding/setup.svelte.ts` (`startFirstSandbox`, `retryFirstSandbox`, `sandboxRun`). The heads-up's two choices mark it wanted; once machine and image are ready it runs in parallel with the model job. A replay marks nothing, so it creates nothing.
- The job reuses an existing `hello-world`, otherwise creates it with pi pre-installed. pi's install does not wait for the model: whichever of the two finishes last re-points pi at the served model (`reprovisionInstalledAgents`).
- With no model ("Set up without a Model" or "Go on without it"), the sandbox is still created and no agent is installed, because pi is provisioned only for the local model and is useless without it.
- `SetupProgress.svelte` gets a fourth card, "Preparing your first sandbox", with its own retry; the technical detail sits under "For those who know". `firstSandboxReady()` also requires the job, so the handoff in `handoff.svelte.ts` only opens the sandbox, finishes onboarding and opens the pi chat. "Go to my sandbox list" still prevents the automatic open.
- The model job now reuses a server that already serves the chosen model and confirms the running state before reporting ready. `inference_status` drops a server whose process has exited.

## Remaining: Phase I: Translations

After the user approves the English flow, add Paraglide keys in `messages/onboarding/{locale}.json` for every `TODO(i18n)` literal this plan adds or rewrites, matching `src/lib/onboarding/AGENTS.md`.

## Parallel-Worktree Constraints

- **New paths:** none. Model weights already land under the app-data root through `app_data_dir()`.
- **New host port:** none. The inference server already takes a free loopback port.
- **Host-global resources:** the single running inference server is host-global, and a real model download is large. Specs that run the real flow belong to the host-locked tier and serialise against other real-infra runs. Fast-tier specs use the tiny test model steered by `MIO_TEST_MODEL` (precedent in `e2e/AGENTS.md`).
- **New bundled binary, teardown, environment variable:** none.

## Lifecycle and Cross-Cutting Concerns

- **App restart mid-onboarding:** onboarding is not complete, so the heads-up shows again. The model job skips a finished download through `downloaded_models`; a partial download restarts unless the downloader resumes, which the spike checks.
- **Back button and skip:** the store outlives step changes, so neither restarts or duplicates a job.
- **Model server and sandbox reach:** a new sandbox gets its reverse forward when it starts, so pi can reach the model; nothing here changes the reach service.
- **Replay:** starts nothing and shows no heads-up.
- **Documentation:** when the feature is implemented, `docs/architecture/workspace-ui.md`, `inference.md` and `agents.md` need a short update; the pre-install step and first-run model job are new behaviour.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The model download starts for a user who has not confirmed | The start is wired to the tour's first page, a skip or a replay | Start only from the heads-up's confirm action |
| Creating "hello world" fails | `validate_name` rejects spaces | Use `hello-world`; show "hello world" as copy only |
| The first sandbox starts before machine and image are ready | The job was started from the heads-up instead of from the setup-complete effect | Start it only when `setupComplete()` is true |
| pi cannot reach a model after install | pi was provisioned before the server was running | Start the server first; `reprovisionInstalledAgents` repairs it after the next start |
| The progress page and status bar disagree | A component kept its own state for the model job | Read only the store's accessors |
| `install_agent` fails right after create | The new sandbox is not running yet | Verify in Phase A; if needed, start it before installing |
| A user on a small machine sees no model offer | `recommendModels` returns nothing recommended | Offer the smallest entry through "Try anyway" |
| A copy sweep misses a term | "Brain" and "Local LLM" live in literals, locale files and agent configuration | Search all three places; keep identifiers unchanged |
| Claude Code appears in first run | The pre-install planner ignored the first-run exclusion | Pass the first-run agent list explicitly |
