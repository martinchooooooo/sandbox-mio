---
type: review
title: "Fastest Time to WOW: Code Review"
feature: fastest-time-to-wow
description: "Code review of the ux-review branch (commits 4706513, d6c1816, ed0f7a4): ranked findings with failure scenarios and fixes."
---
# Fastest Time to WOW: Code Review

This review covers the `ux-review` branch against `main` (commits 4706513, d6c1816, ed0f7a4): the first-run store `src/lib/onboarding/setup.svelte.ts`, the handoff `src/lib/onboarding/handoff.svelte.ts`, the milestones `src/lib/onboarding/milestones.ts`, the pre-install path in `src/lib/sandbox/lifecycle.svelte.ts`, `src/lib/agents/install.ts` and `src/lib/agents/preinstall.ts`, the welcome components `src/lib/welcome/WelcomeFlow.svelte`, `HeadsUpStep.svelte` and `SetupProgress.svelte`, the settings restart in `src/routes/settings/+page.svelte`, the Rust commands `log_milestone` (`src-tauri/src/commands/app.rs`) and `inference_status` (`src-tauri/src/commands/inference.rs`), the e2e specs and the feature documents. Intent was read from [02_dev.md](02_dev.md).

Verdict: the happy path (confirm, read the tour, hand over to `hello-world`) is sound, and the module-scoped stores avoid duplicate jobs across step changes. Two correctness bugs break documented behaviour: "Go to my sandbox list" cannot reliably cancel the automatic open (R1), and a model failure followed by "Go on without it" hands the user a pi chat with no model behind it (R2). The progress page also has dead ends and a stuck status line once any job needs a manual retry (R3, R4). None of the findings needs a design change; every fix is local.

## Findings

Ranked most severe first. Line numbers refer to the branch tip ed0f7a4.

### R1 (high): "Go to my sandbox list" does not cancel the handoff, and the cancel restarts the handoff

- **Status:** Fixed. A terminal `cancelled` phase makes the cancel final, `startHandoff()` checks it after every await, and the cancel button is shown for the whole wait.

- **Where:** `startHandoff()` in `src/lib/onboarding/handoff.svelte.ts:27-44`; the handoff `$effect` and the cancel button in `src/lib/welcome/WelcomeFlow.svelte:75-79` and `:149-151`.
- **Defect:** The cancel button renders only when `firstSandboxReady()` is true, which is the same condition that makes the `$effect` call `startHandoff()` in the same flush, so the cancel window is the single `refreshSandboxState()` round trip. When the cancel does land, `startHandoff()` sets `phase` back to `"idle"`; the `$effect` depends on `handoffPhase()`, re-runs while the component is still mounted (the cancel's `completeOnboarding()` needs two IPC round trips before it unmounts the welcome flow), and calls `startHandoff()` again, which resets the non-reactive `cancelled` flag to false at line 30 and opens the sandbox.
- **Failure scenario:** All four cards turn ready while the user is on the progress page. The user clicks "Go to my sandbox list". Either the click arrives after `phase` became `"opening"` and is ignored, or the first handoff sees `cancelled`, returns to `"idle"`, the effect starts a second handoff, and `hello-world` with its pi chat opens anyway. 02_dev.md ("Deviation: First Sandbox in the Background Setup") promises the cancel prevents the automatic open.
- **Fix:** Add a terminal phase `"cancelled"` to the `Phase` union in `handoff.svelte.ts`; `cancelHandoff()` sets `phase = "cancelled"` when the phase is `"idle"` or `"creating"`, and `startHandoff()` returns early when the phase is `"cancelled"` instead of resetting `cancelled`. After each await in `startHandoff()`, check `phase === "cancelled"` rather than the plain boolean. Only `retryHandoff()` and a fresh onboarding (page reload) clear it. Separately, show the cancel button for the whole time the progress page waits (see R3), so the user can opt out before the handoff starts rather than racing it.

### R2 (high): a failed model followed by "Go on without it" still installs pi and opens its chat

- **Status:** Fixed. The chat opens only when pi is installed and the model is serving (both branches); a toast explains when pi is installed without a model, and the card copy derives from `sandbox.installed`.

- **Where:** the agent list chosen at create time in `runFirstSandbox()` (`src/lib/onboarding/setup.svelte.ts:322`); `skipModelSetup()` (`setup.svelte.ts:151-154`); `piReady` in `startHandoff()` (`src/lib/onboarding/handoff.svelte.ts:40`); the first-sandbox card copy in `src/lib/welcome/SetupProgress.svelte:148`.
- **Defect:** The first-sandbox job decides `agents: model.skipped ? [] : ["pi"]` once, when it starts, which is right after machine and image are ready and usually long before the model download finishes or fails. A later "Go on without it" sets `model.skipped`, but pi is already installed, so `sandbox.installed` is true and the handoff opens the pi chat. pi was provisioned with the fallback model id (no server was running), so the first message fails.
- **Failure scenario:** The model download fails at 60% on a flaky connection. Meanwhile `hello-world` was created with pi. The user clicks "Go on without it". The first-sandbox card now claims "with no agent, as you went on without a Model", the handoff opens the pi chat pane, and the user's first prompt errors. 02_dev.md, OQ-no-local-model, says the sandbox gets no agent in this case and the user is told why.
- **Fix:** In `startHandoff()`, compute `piReady` as `sandbox.installed && modelRun().ready` for a created sandbox too, so the chat opens only when a model is serving; when pi is installed but the model was skipped, open the sandbox without the chat and toast a short tour-voiced note that pi needs a Model (TODO(i18n)). Change the first-sandbox card copy in `SetupProgress.svelte:148` to derive from `sandbox.installed` rather than `model.skipped`. Uninstalling pi is not needed: a later model download plus `reprovisionInstalledAgents` makes it useful.

### R3 (medium): the progress page has dead ends and no way out during a long wait

- **Status:** Fixed. The progress page always offers "Go to my sandbox list"; the model card offers "Go on without it" while downloading or starting (the download keeps running); the first-sandbox error state offers it too.

- **Where:** the actions block in `src/lib/welcome/WelcomeFlow.svelte:146-152`; the first-sandbox card in `src/lib/welcome/SetupProgress.svelte:138-140`; the model card in `SetupProgress.svelte:112-124`.
- **Defect:** On `main`, onboarding could be finished once machine and image were ready. On the branch the only exit is the handoff, which needs `firstSandboxReady()`: machine, image, model (ready or skipped) and the first sandbox. The first-sandbox card offers only "Try again" on failure, with no skip. The model card offers "Go on without it" only after an error, never during a slow download.
- **Failure scenario A:** `create_sandbox` fails deterministically (a provider or name clash the retry cannot fix). The user retries forever; onboarding cannot be completed, and the app chrome stays locked because `onboardingCompleted()` stays false.
- **Failure scenario B:** A multi-gigabyte download on a slow connection. The user finishes the tour and sees "Downloading… 12%" with only "Back to the tour" available, for tens of minutes.
- **Fix:** Always render "Go to my sandbox list" on the progress page in the onboarding variant (not only when `firstSandboxReady()`), wired through the R1 cancel; it completes onboarding and leaves every background job running, which the module-scoped store already supports. On the first-sandbox card's error state, add a secondary "Go on without it" that does the same. On the model card's downloading and starting states, add a secondary "Go on without it" that marks the model skipped for the handoff (the download itself may keep running and is visible in the activity center).

### R4 (medium): a machine retried from its card never chains the image build, and the status line says "Checking your computer…" forever

- **Status:** Fixed. Machine and image retries go through `startBackgroundSetup`, idle copy reads "Not started yet.", and `setupHeadline()` reports `attention` when a step is idle and nothing runs.

- **Where:** `startBackgroundSetup()` in `src/lib/onboarding/setup.svelte.ts:414-428`; the `"Checking your computer…"` fallback in `setupHeadline()` at `setup.svelte.ts:486`; the machine retry at `src/lib/welcome/SetupProgress.svelte:69` and `:72`; the idle image state at `SetupProgress.svelte:95-97`; the comment at `src/lib/welcome/WelcomeFlow.svelte:68-69`.
- **Defect:** The background run stops at the first machine failure. A successful retry from the machine card runs only `runMachineSetup()`; nothing calls `runImageDownload()` afterwards, so the image waits for a click on "Pack it now". The idle copy above that button reads "Packing it. This takes a few minutes the first time.", which describes work that is not happening. `setupHeadline()` falls through to `sandboxWanted && !sandbox.ready` and reports `running` with "Checking your computer…" indefinitely. The same idle-but-busy copy applies to the machine card ("Getting it ready. This takes a few minutes." above "Get it ready"). The WelcomeFlow comment claims the effect "covers a machine or image retried from its card", which is true for the first sandbox only.
- **Failure scenario:** The first machine setup fails (a transient WSL error). The user clicks "Try again" on the machine card and it succeeds. The image card says "Packing it…", the status bar says "Checking your computer…", and nothing happens until the user notices the "Pack it now" button.
- **Fix:** Make the machine card's retry call `startBackgroundSetup(provider)` instead of `runMachineSetup(provider)`, so the retry continues into the image build as the first run did (`startBackgroundSetup` already skips finished steps). Do the same for the image card's "Try again" after a probe error. Change the idle copy of both cards to an invitation ("Not started yet."), keeping the progress copy for the busy state only. In `setupHeadline()`, return the `"Checking your computer…"` state only while `backgroundActive || modelJobActive`, and return an `attention` headline ("Something needs a hand") when a step is idle and nothing runs. Update the WelcomeFlow comment to match.

### R5 (medium): a reused `hello-world` never gets pi, yet the handoff opens its chat

- **Status:** Fixed. A reused `hello-world` gets pi installed when it is missing and the model was not skipped; `sandbox.installed` reflects the result. Not covered by a light spec (the mock install has no model path); a stopped reused sandbox makes the install fail quietly and the chat stays closed.

- **Where:** the reuse branch in `runFirstSandbox()` (`src/lib/onboarding/setup.svelte.ts:314-315`); `piReady` in `src/lib/onboarding/handoff.svelte.ts:40`; the restart switch in `src/routes/settings/+page.svelte:142-160`.
- **Defect:** An existing `hello-world` is reused as it is, and the handoff opens the pi chat whenever the model is ready, without checking that pi is installed in that sandbox. `open_agent_chat` (`src-tauri/src/commands/panes.rs:87`) opens the native chat pane unconditionally, so the user gets a chat pane with no pi behind it. The new settings restart makes this reachable: with "Also delete the hello-world sandbox" switched off, or with a `hello-world` create still in flight at restart time (the page reload kills the webview-driven pre-install loop in `preinstallAgents()`, while the Rust create completes and lists the sandbox without pi).
- **Failure scenario:** A user first onboarded with "Set up without a Model", so `hello-world` has no agent. Later the user restarts onboarding from settings with the delete switch off and confirms the model. The handoff reuses `hello-world`, opens the chat pane, and pi is missing.
- **Fix:** In the reuse branch of `runFirstSandbox()`, read the sandbox's installed agents (the same source `reprovisionInstalledAgents` in `src/lib/agents/reprovision.ts` uses) and, when pi is missing and the model was not skipped, run `installAgentInSandbox()` from `src/lib/agents/install.ts` with the pi descriptor before setting `sandbox.ready`; set `sandbox.installed` from the result. `startHandoff()` then uses `sandbox.installed && modelRun().ready` for both branches (shared with R2).

### R6 (low): running jobs mask a failed job in the status line

- **Status:** Fixed. The error branch of `setupHeadline()` now precedes the running branches.

- **Where:** the branch order in `setupHeadline()`, `src/lib/onboarding/setup.svelte.ts:463-485`.
- **Defect:** Every `running` branch (machine, model, image, sandbox busy) is checked before the error branch, so an error in one job stays invisible while another job runs.
- **Failure scenario:** The model download fails two minutes in while the image build and then the first sandbox run for several more minutes. The status bar keeps saying "Getting things ready" and offers no "Go to setup" action; the user learns of the failure only when everything else has finished.
- **Fix:** Move the error check above the `running` branches in `setupHeadline()`, or keep the running title but switch `state` to `attention` and append "· Something needs a hand" to the detail whenever any job has an error, so the "Go to setup" action in `WelcomeFlow.svelte:91-94` appears.

### R7 (low): the heads-up can be confirmed before the machine probe finishes, bypassing "Try anyway"

- **Status:** Fixed. `specsProbed()` is exposed and the heads-up shows "Checking your computer…" with the primary button disabled until the probe settles.

- **Where:** `modelOffer()` and `startModelSetup()` in `src/lib/onboarding/setup.svelte.ts:124-146`; the default branch of `src/lib/welcome/HeadsUpStep.svelte:189-198`.
- **Defect:** While `detect_machine_specs` is still running, `modelOffer()` returns null and the heads-up renders the normal variant with no size and the "Start the tour and get set up" button. Confirming awaits the probe inside `startModelSetup()` and then downloads the low-RAM offer without the explicit "Try anyway" consent 02_dev.md, Phase F requires.
- **Failure scenario:** On a slow Windows machine the WMI probe takes several seconds; the user clicks through the hero quickly and confirms on a low-memory laptop. The smallest model downloads although the user never saw the low-memory warning or the download size.
- **Fix:** In `HeadsUpStep.svelte`, show a short "Checking your computer…" state and disable the primary button while the probe is pending; expose a `specsProbed()` accessor from `setup.svelte.ts` (true once `probeMachineSpecs()` settled, success or failure) so a failed probe still lets the user continue. Keep "Set up without a Model" enabled throughout.

### R8 (low): `inference_status` silently clears a dead server but leaves its sandbox forwards open

- **Status:** Fixed. `inference_status` spawns `close_all` when it reaps a dead server.

- **Where:** `inference_status` in `src-tauri/src/commands/inference.rs:142-152`.
- **Defect:** A status read now mutates state: it drops the handle when the child exited, but does not call `services::inference_reach::close_all` as `stop_inference` does, so every sandbox's reverse forward keeps pointing at the dead port until the next `start_inference`.
- **Failure scenario:** llama.cpp runs out of memory and exits. The Models page polls `inference_status`, which reports not running; pi inside `hello-world` keeps hitting a forward to nothing and gets connection refused rather than a clear "model stopped" signal. Harmless today, but the side effect in a getter is easy to miss.
- **Fix:** When `exited` is true in `inference_status`, spawn `services::inference_reach::close_all` off the critical path, mirroring `stop_inference`, and add a code comment that the status read reaps a dead server.

### R9 (low): generated docs indexes were out of date

- **Status:** Already fixed before this pass.

- **Where:** `docs/features/fastest-time-to-wow/index.md`, `docs/features/index.md`, `docs/architecture/index.md`.
- **Defect:** `feature.md` moved to `status: in-progress` but the indexes still listed it as `planned`; `./build docs` failed with three "out of date" errors on the branch tip.
- **Fix:** Run `./build docs --fix`. Resolved while adding this review, since adding a document requires regenerating the indexes anyway.

### R10 (low): stale comments and orientation docs

- **Status:** Fixed. Store comment, `src/AGENTS.md`, `src/lib/onboarding/AGENTS.md` (layout row and gotcha) and `readiness.ts` now name the four jobs.

- **Where:** the module doc comment in `src/lib/onboarding/setup.svelte.ts:25-31` ("machine, then base image", "the two setup cards"); the store entry in `src/AGENTS.md:24` (same wording); the Common Gotchas row in `src/lib/onboarding/AGENTS.md:58` (names only `machineRun()` and `imageRun()`); the doc comment of `fetchMachineSetupSteps()` in `src/lib/machine/readiness.ts:5-7` (names the deleted download-necessities step).
- **Defect:** The root `AGENTS.md` comment rules require updating an adjacent comment a change makes false. All four still describe the two-card, two-job flow.
- **Fix:** Rewrite each to name the four jobs (machine, image, model, first sandbox) and `SetupProgress.svelte`; in the gotchas row, list `modelRun()` and `sandboxRun()` too.

### R11 (low): e2e coverage misses the new branches

- **Status:** Partly fixed. Light specs now assert `hello-world` exists with no packages after the decline, the first-sandbox retry, the cancel (no auto-open) and the settings restart with the switch on and off. Not covered: the model-failure skip path and the Phase D pre-install on an ordinary create, because both need a downloaded model and a running inference server, which the mock backend cannot fake; they belong in a `standard` or `heavy` spec.

- **Where:** `e2e/test/specs/light/onboarding-story-ui.spec.ts`; `e2e/test/specs/light/settings-debugging-ui.spec.ts` (unchanged).
- **Defect:** The only updated spec declines the model and asserts that the nav chrome appears; it never asserts that `hello-world` was created, that `install_agent` was not called with the model skipped, or that the sandbox opened. No spec covers the settings restart, the cancel (R1), the first-sandbox retry, a model failure followed by "Go on without it" (R2), or Phase D pre-install on an ordinary create.
- **Fix:** In the light tier, using the mock control helpers in `e2e/lib/mock.ts`: assert `hello-world` is listed and `mock_calls` holds no `install_agent` call after the decline path; add a case that fails `create_sandbox` once through `configureMock`, checks the first-sandbox card's error, and retries; add a settings-debugging case that clicks `[data-settings-restart-onboarding]` and checks the heads-up renders and `hello-world` is gone; add a create-flow case that seeds `downloaded_models` and asserts `install_agent` runs for pi. The model path itself (download, serve, pi reply) stays a `standard` or `heavy` spec, as the light spec's header already says; record the plan in `03_e2e.md`.

### R12 (low): hard-coded English outside onboarding

- **Status:** Fixed. `projects_preinstall_installing`, `projects_preinstall_failed` and `settings_debugging_restart_onboarding_force_confirm` exist in en, de and ru; 02_dev.md Phase I notes the scope.

- **Where:** the pre-install step text and failure toast in `preinstallAgents()` (`src/lib/sandbox/lifecycle.svelte.ts:198` and `:203`); the force-destroy confirm text in `src/routes/settings/+page.svelte:151`.
- **Defect:** Phase I of 02_dev.md plans translations for onboarding literals in `messages/onboarding/{locale}.json`, but these strings show on every sandbox create and in settings, for all users, in German and Russian too.
- **Fix:** Add `projects_preinstall_installing` and `projects_preinstall_failed` (with `{agent}`, `{name}`, `{message}` parameters) and a `settings_debugging_restart_onboarding_force_confirm` key to `messages/en.json`, `de.json` and `ru.json`, and call them through `m`. These belong to the main message files, not the onboarding ones; add them to Phase I's scope in 02_dev.md.

### R13 (nit): the restart switch defaults to deleting `hello-world` with no confirmation

- **Status:** Fixed. The delete switch defaults to off.

- **Where:** `deleteFirstSandbox = $state(true)` in `src/routes/settings/+page.svelte:142`.
- **Defect:** One click on "Restart onboarding" destroys the user's `hello-world` sandbox; a confirmation appears only for unsynced remote changes. The section is a debugging tool, so the risk is small, but a user may have put work in `hello-world`.
- **Fix:** Default the switch to off, or confirm before destroying with the sandbox name in the prompt.

### R14 (nit): the cancel button swallows a failed write

- **Status:** Fixed. A rejected `completeOnboarding()` is toasted through `parseError`.

- **Where:** `cancelHandoffToList()` in `src/lib/welcome/WelcomeFlow.svelte:41-49`.
- **Defect:** A rejected `completeOnboarding()` becomes an unhandled rejection; the button stops spinning and nothing tells the user.
- **Fix:** Add a `catch` that toasts `parseError(e).message`, as the root `AGENTS.md` error-handling rules require for `invoke()` failures.

### R15 (nit): progress updates are not announced

- **Status:** Fixed. Each card's status block is an `aria-live="polite"` region and the download `Progress` is labelled "Model download".

- **Where:** the card status lines and the download `Progress` in `src/lib/welcome/SetupProgress.svelte:58-152`.
- **Defect:** Status changes ("Ready ✓", errors, download percent) are not in a live region, and the `Progress` bar has no accessible name, so a screen reader user gets no feedback on the page that exists to report progress.
- **Fix:** Wrap each card's status line in an element with `aria-live="polite"`, and give the `Progress` an `aria-label` ("Model download") (TODO(i18n)).

### R16 (nit): 02_dev.md Phase H order drifted from the code

- **Status:** Fixed. 02_dev.md Phase H states the implemented order and its reason.

- **Where:** [02_dev.md](02_dev.md), Phase H, versus `startHandoff()` in `src/lib/onboarding/handoff.svelte.ts:45-48`.
- **Defect:** Phase H says onboarding is completed and then the sandbox opened; the code opens the sandbox first so an open failure keeps the retry UI mounted.
- **Fix:** Update the Phase H bullet to the implemented order and its reason.

## Not fixing / out of scope

- **A different running model is replaced:** `runModelSetup()` (`src/lib/onboarding/setup.svelte.ts:197-203`) stops a server serving another model and starts the offered one, and downloads the offer even when another model is already on disk. Reachable only through the debugging restart, where the user expects a fresh first run.
- **Milestone origin after `resetOnboarding()`:** `resetOnboarding()` in `src/lib/settings/onboarding.svelte.ts` keeps `firstOpenedAtMs`, so a replay's milestones would measure from the original first launch; a replay starts no job and records no milestone, so nothing is mis-measured today.
- **Settings read-modify-write races:** `firstOpenTime()` in `src/lib/onboarding/milestones.ts` and `writeCompleted()` both round-trip the whole settings object; a concurrent write could drop a field. No concurrent writer exists at the points where they run, and the pattern predates this feature.
- **Review file name:** `docs/features/README.md` names the optional review document `05_dev_review.md`; this file is `04_review.md` as requested. Rename it if the convention should win.
