---
type: dev-plan
title: "Welcome / Onboarding Screen — Build Plan"
feature: welcome-onboarding
---
# Welcome / Onboarding Screen — Build Plan

This document is the build plan for the welcome/onboarding feature scoped in `docs/features/welcome-onboarding/01_ideas.md`; read that document first for the rejected alternatives behind every choice restated here. It continues every "Resolved 2026-07-08" decision already recorded there and additionally resolves the questions `01_ideas.md` left open, choosing concrete defaults for the mechanical ones rather than leaving them for a later document. **Nothing in this feature is implemented yet** — every phase below is `Remaining`. This document fixes the seams (file paths, command names, trait methods, store shapes) a future agent implementing this feature must create or touch, so `feature-implement` can proceed phase by phase without re-deriving the design.

## The Concrete Model

Six seams, built in the order listed because each later phase's frontend work depends on the backend/store seam before it:

1. **Backend**: one new `AppSettings` field (`onboarding_completed`), one new trait method (`SandboxBackend::base_image_ready`) with a single implementation on `PodmanCliBackend`, one new Tauri command (`build_base_image`) plus one new query command (`base_image_ready`), all wired into `tauri::generate_handler![]` in `src-tauri/src/lib.rs`.
2. **`src/lib/podman/setup.svelte.ts`** — a new module-scoped store, extracted from `src/routes/projects/+page.svelte`'s existing Podman first-run/start/provision state machine, imported by both `/projects` (refactored onto it) and the new welcome flow.
3. **`src/lib/settings/onboarding.svelte.ts`** — a new module-scoped store holding `onboarding_completed`, hydrated once from `+layout.svelte`'s existing `onMount` (alongside its existing `ensureActivity()`/`ensureWorkspaceState()`/`ensureNetworkState()` calls), read by `+layout.svelte` itself (nav-chrome gating) and by `/`.
4. **`src/lib/welcome/`** — new components: `MioHero.svelte` (cycling-translation hero), `WelcomeFlow.svelte` (the three-step container, parameterized by a `variant: "onboarding" | "replay"` prop), and the two action-step components (`CreateVmStep.svelte`, `DownloadNecessitiesStep.svelte`).
5. **`src/routes/+page.svelte`** rewritten around `WelcomeFlow` (onboarding variant) pre-completion, and a small "get started with your first sandbox" screen post-completion; both states render the debug restart button.
6. **`src/routes/intro/+page.svelte`** rewritten to render `WelcomeFlow` in its `"replay"` variant, replacing the current bare `Robot.svelte` stub.

## Resolved Decisions

Restating every Open Question from `01_ideas.md`, in the order it appears there:

- **Where does the shared "create virtual machine" logic live?** Extracted into `src/lib/podman/setup.svelte.ts` (Phase B below); `/projects` is refactored to import it instead of holding its own copy.
- **Does `/` stay the welcome flow's home, or does the flow move to `/intro`?** `/` stays the first-run home; `/intro` is repurposed as the post-onboarding replay entry, both rendering the same `WelcomeFlow.svelte` in different `variant`s.
- **Does the welcome flow show with or without the persistent nav chrome?** Hidden while `onboarding_completed` is `false`, via a new condition in `+layout.svelte` reading `src/lib/settings/onboarding.svelte.ts` (Phase C).
- **Re-entry: can a user replay the welcome flow, and does it show real state?** Yes, via the existing `GlobalNav` `/intro` tab (unchanged href). Both action steps check live state before rendering — `CreateVmStep` reads `src/lib/podman/setup.svelte.ts`'s gate state, `DownloadNecessitiesStep` calls the new `base_image_ready` query command — in **both** `variant`s, not replay-only, so a first-run user who already has a Podman machine from before (e.g. settings reset without touching the host) also sees correct state rather than a hardcoded "not started." This is a property of the action-step components themselves, not something `variant` toggles.
- **What does `/` show once onboarding is complete?** A small standalone screen — headline plus one line of copy plus a button/link to `/projects` — not a redirect. Copy key: `home_ready_title`/`home_ready_description`/`home_ready_button` (final names decided in Phase D/F).
- **Does the debug restart-the-flow button ship in production builds?** Resolved now: it always renders, gated on nothing — `src/routes/+page.svelte` is not reachable from any nav tab once onboarding is complete (the user lands there only via relaunch or the `/intro` tab takes them to the replay route instead), so the exposure surface is small enough that a build-flag gate is unneeded complexity for a stated debugging aid. Revisit if this turns out to be reachable/confusing in practice.
- **Auto-advance vs. explicit dismissal for the hero step.** Resolved now, choosing the simpler of the two: the hero step is dismissed by an explicit "Get started" click, not a timed auto-advance. A timed auto-advance needs a tuned cycle count and risks advancing before a user has read anything; an explicit click has no tuning surface and matches the "bare bones" framing of the feature.
- **Full translation list for the "mio" word.** Resolved now: the fixed list is exactly the four words already named — `{ word: "mio", lang: "it" }`, `{ word: "Mine", lang: "en" }`, `{ word: "Meins", lang: "de" }`, `{ word: "Мой", lang: "ru" }` — a plain array literal in `MioHero.svelte`, not dynamically derived from `messages/`'s locale list. Tying it to `messages/`'s supported-locale list would silently change the hero's visual content every time a locale is added or removed for unrelated reasons, which is a bigger coupling than a four-word decorative animation needs.
- **Does "Download necessities" show progress detail or just busy/done?** Resolved now: a single busy/error state via `createActionRunner` (`src/lib/actions.svelte.ts`), covering both the auto-chained Podman-start/provision step and the image build as one continuous busy period, not per-step detail. `build_base_image` (Phase A) returns a bare `Result<(), AppError>` with no `Channel` — matching this choice; step-level progress would need a signature change if a later revision wants it.

## Phase A (Implemented): Backend seams

**`src-tauri/src/settings/types.rs`** — add one field to `AppSettings`:

```rust
pub onboarding_completed: bool,
```

Defaults to `false` via the struct's existing `#[serde(rename_all = "camelCase", default)]`, so existing on-disk `settings.json` files from before this feature deserialize with onboarding treated as incomplete (see Common Gotchas). No change needed to `settings::load`/`settings::save` (`src-tauri/src/settings/mod.rs`) or to the existing `get_settings`/`set_settings` commands (`src-tauri/src/lib.rs:1265-1272`) — both already round-trip the whole `AppSettings` struct, so the new field rides along automatically. The frontend reads/writes it exactly the way `src/routes/settings/+page.svelte` already reads/writes `inferenceEngine` today: load the full object, mutate the one field, save the full object back.

**`src-tauri/src/backends/types.rs`** — add one trait method to `SandboxBackend`, next to `ensure_base_image` (line 98):

```rust
/// Whether the base image already exists on the active connection, without building it.
/// Query-only counterpart to `ensure_base_image` for UI that must distinguish "already done"
/// from "not started yet" (the welcome/onboarding replay screen).
async fn base_image_ready(&self) -> Result<bool, BackendError>;
```

**`src-tauri/src/backends/mod.rs`** — implement it on `PodmanCliBackend` by extracting the existing inline check out of `ensure_image_on` (currently lines 243-245) into a shared private helper both call:

```rust
async fn image_exists_on(&self, connection: &str) -> Result<bool, BackendError> {
    Ok(self.podman_on(connection, &["image", "exists", &Self::image()]).await?.status.success())
}
```

`ensure_image_on` becomes `if self.image_exists_on(connection).await? { return Ok(()); }`, and the new trait method is `async fn base_image_ready(&self) -> Result<bool, BackendError> { self.image_exists_on(&self.connection).await }`. This is a pure refactor of existing behavior (same `podman image exists <tag>` check, same short-circuit semantics) plus one new call site — no change to what `ensure_base_image`/`ensure_image_on` actually do.

**No second `SandboxBackend` implementation is needed.** `PodmanCliBackend` (`src-tauri/src/backends/mod.rs:385`) is the only implementation in the repo; there is no `backends/mock.rs`, and the light tier mocks one layer lower, at the `ProcessRunner` seam (`ScriptedPodmanRunner`, wired in `src-tauri/src/lib.rs:1341`). `base_image_ready` therefore reaches the light tier through the already-registered `podman.image-exists` mockctl key (`src-tauri/src/mockctl/mod.rs:72`) with no new mock code at all: unscripted it takes `default_output`'s exit 0 (`mockctl/mod.rs:255`) and reports the image present, and a spec installs a `FailAlways` behaviour on that key to get the not-yet-built path. Both renderings of `DownloadNecessitiesStep` are consequently reachable from the light tier — there is no mock-side coverage gap for `03_e2e.md` to carry.

**`src-tauri/src/lib.rs`** — two new commands, near `create_sandbox` (around line 70-90) and `base_image_version`:

```rust
#[tauri::command]
async fn build_base_image(store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), AppError> {
    Ok(store.ensure_base_image().await?)
}

#[tauri::command]
async fn base_image_ready(store: State<'_, Arc<dyn SandboxBackend>>) -> Result<bool, AppError> {
    Ok(store.base_image_ready().await?)
}
```

Both added to the `tauri::generate_handler![]` list (alongside `get_settings`/`set_settings`, `src-tauri/src/lib.rs:1470-1471`). `build_base_image` deliberately does not call `runtime.init()` the way `create_sandbox` does (line 78) — it only stages the image, exactly matching the "Download necessities" scope in `01_ideas.md`'s Technology Choices (rejecting the "reuse `create_sandbox`" alternative).

## Phase B (Implemented): Shared Podman-setup store

**New file `src/lib/podman/setup.svelte.ts`.** Module-scoped `$state`, following the `src/lib/registry.svelte.ts` pattern (the smallest existing reference, per `src/AGENTS.md`):

- State: `podmanUnavailable`, `podmanNeedsSetup`, `podmanRebootRequired`, `setupBusy`, `setupError` — the same five pieces of state currently local to `src/routes/projects/+page.svelte` (lines 26-42). `setupBusy`/`setupError` stay hand-rolled rather than becoming a `createActionRunner`: that helper keeps only the message, and this flow branches on `parseError(e).kind === "PodmanRebootRequired"` to show a reboot prompt instead of a bare retry (the constraint recorded at `projects/+page.svelte:39-42`). Moving them into the store is what lets `CreateVmStep` reuse the reboot branch instead of re-deriving it.
- `async function checkPodman(): Promise<void>` — the probe currently inlined in `projects/+page.svelte`'s `onMount` (lines 98-119): calls `check_podman_ready`, and on a `PodmanUnavailable` error calls `machine_needs_init` to set `podmanNeedsSetup`. Exported so every caller (both `/projects`'s `onMount` and the welcome flow's `CreateVmStep`) re-probes fresh on its own mount rather than trusting a value some other route computed earlier — Podman can be stopped or started outside the app between mounts, so "fresh per mount, shared implementation" is the model, not "compute once, cache across routes."
- `async function startPodman(): Promise<boolean>` and `async function setupPodman(): Promise<boolean>` — lifted verbatim from `projects/+page.svelte` (lines 124-155), returning whether the action succeeded so callers can chain their own follow-up (`loadAll()` for `/projects`, "proceed to build the image" for `CreateVmStep`) without the store needing to know about either caller's next step. `startPodman`'s `createActionRunner` (`startAction`, `projects/+page.svelte:37`) moves into the store with it — unlike `setupPodman`, starting a machine has no `kind`-dependent branch, so the helper fits there.
- Accessor functions (`podmanUnavailable()`, etc.) exported per the module-store convention, not raw exported `let` bindings.

**`src/routes/projects/+page.svelte`** refactored to import from this module instead of declaring the five `$state` fields and three functions locally (lines 26-42, 98-156 collapse to calls into the store plus the existing `loadAll()`/poll wiring, which stays local to `/projects` since it is that route's own concern, not the Podman gate's).

## Phase C (Implemented): Onboarding-settings store and nav-chrome gating

**New file `src/lib/settings/onboarding.svelte.ts`.** Same module-store shape as Phase B:

- State: `onboardingCompleted = $state<boolean | undefined>(undefined)` (`undefined` = not yet loaded, distinct from `false` = loaded-and-incomplete, so `+layout.svelte` can avoid a flash of nav chrome before the first `get_settings` resolves).
- `async function ensureOnboardingState(): Promise<void>` — idempotent load (mirroring `ensureRegistry`'s `fetched` guard in `src/lib/registry.svelte.ts:6,10-17`), calling `get_settings` once and setting `onboardingCompleted`.
- `async function completeOnboarding(): Promise<void>` and `async function resetOnboarding(): Promise<void>` — each calls `get_settings`, flips the one field, calls `set_settings` with the full object back, then updates local state. Two separate functions (not one `setOnboardingCompleted(bool)`) because the two call sites read differently in the code that uses them (`WelcomeFlow`'s completion vs. the debug restart button).

**`src/routes/+layout.svelte`** — call `ensureOnboardingState()` inside the existing `onMount`'s async IIFE (alongside `ensureWorkspaceState()`/`ensureNetworkState()`, lines 95-96).

`NavigationColumn` is not a sibling of the routed page — it *wraps* it (`+layout.svelte:106-108`; `{@render children?.()}` sits between `GlobalNav` and `ActivityCenter` inside `NavigationColumn.svelte:49`). Gating the `<NavigationColumn>` element itself would therefore hide the welcome flow along with the chrome. The gate is an `{#if}`/`{:else}` around the whole markup block instead, with the chromeless branch rendering `children` in a bare full-bleed container:

```svelte
{#if onboardingCompleted() !== false}
  <NavigationColumn {collapsed} showControls={showWorkspaceChrome} onToggleCollapsed={toggleCollapsed}>
    {@render children?.()}
  </NavigationColumn>
  {#if showWorkspaceChrome}
    <WorkspaceContent {hasTabs} {dashboardSandbox} />
  {/if}
{:else}
  <div class="@container flex-1 overflow-y-auto">
    {@render children?.()}
  </div>
{/if}
```

Chrome shows once `true`, and also while still `undefined`/loading, to avoid a chrome-then-hide-then-show flash — the welcome route itself is what's briefly blank during that window, not the reverse. The chromeless branch keeps `@container` because components below it (`SandboxItem.svelte`, and any welcome component using the same idiom) resolve container queries against that ancestor. `WorkspaceContent` is suppressed alongside the chrome: no embedded pane can be open during first-run onboarding. The `/intro` replay route is unaffected: it is only linked to from `GlobalNav`, which is only visible once `onboardingCompleted()` is already `true`, so nav chrome is already showing whenever `/intro` is reachable.

## Phase D (Implemented): Welcome flow components

**`src/lib/welcome/MioHero.svelte`** — new. A `gsap.timeline({ repeat: -1 })` cross-fading through the fixed four-entry word list (Resolved Decisions above) on a stack of absolutely-positioned `<span>`s, mounted in `onMount` per the `Robot.svelte` idiom (`src/lib/Robot.svelte:5-51`). Takes no props; purely decorative. Whether it embeds `<Robot />` alongside the cycling headline as one composed scene is left to implementation-time visual judgment, not re-litigated here — `01_ideas.md` already flagged this as "likely but not decided" and no further product input changes that.

**`src/lib/welcome/WelcomeFlow.svelte`** — new. Props: `variant: "onboarding" | "replay"`. Local `$state<"hero" | "welcome" | "actions">` step machine (per `01_ideas.md`'s Proposed Model), rendering:
1. `<MioHero />` plus a "Get started" button that advances to `"welcome"` (per the resolved explicit-dismissal decision above).
2. Welcome copy — app name, "brief setup," guided-journey framing (`01_ideas.md`'s "Guided-journey framing" section) — plus a "Continue" button advancing to `"actions"`.
3. `<CreateVmStep />` and `<DownloadNecessitiesStep />` side by side or stacked (visual detail, not a build-plan concern). In `variant="onboarding"`, once both steps report done (each step component exposes its own done state upward, e.g. via an `ondone` callback prop), a "Finish" button becomes enabled and calls `completeOnboarding()` from `src/lib/settings/onboarding.svelte.ts`. In `variant="replay"`, there is no "Finish" button and no call to `completeOnboarding()` — the replay is purely a walkthrough that reflects current state, matching the resolved decision that replaying must not re-trigger or re-flag anything.

**`src/lib/welcome/CreateVmStep.svelte`** — new. On mount, calls `checkPodman()` from `src/lib/podman/setup.svelte.ts`, then renders: a completed/disabled state if `!podmanUnavailable()`, otherwise the same "Start Podman"/"Setup Podman"/`PodmanRebootRequired` branches `projects/+page.svelte` already renders, now reading the shared store instead of local state. Busy/error display comes from the store's `setupBusy`/`setupError`/`podmanRebootRequired` (Phase B), **not** from a `createActionRunner` this component owns — wrapping the setup call in that helper would drop the `kind` the reboot branch needs.

**`src/lib/welcome/DownloadNecessitiesStep.svelte`** — new. On mount, calls the new `base_image_ready` query command; renders a completed/disabled state if it returns `true`. Otherwise, its click handler auto-chains (Resolved Decisions in `01_ideas.md`): call `checkPodman()`, and if `podmanUnavailable()`, call `setupPodman()` (or `startPodman()` if `!podmanNeedsSetup()`) from the shared store first; once that resolves, call `build_base_image`. One `createActionRunner` busy state spans the whole chain, per the resolved "no step-level progress" decision above — but the chained `setupPodman()` can still fail with `PodmanRebootRequired`, so this step renders the store's `podmanRebootRequired` reboot prompt when set, in preference to the runner's generic error text. The runner covers the build; the store covers the provisioning branch it delegated to.

## Phase E (Implemented): Route rewrites

**`src/routes/+page.svelte`** rewritten: on mount, call `ensureOnboardingState()` (Phase C); render `<WelcomeFlow variant="onboarding" />` while `onboardingCompleted()` is `false`, or the small "get started with your first sandbox" screen (headline, one line of copy, a link/button to `/projects`) once it is `true`. In both states, render the debug restart button fixed to the bottom-left corner, calling `resetOnboarding()` on click (Resolved Decisions: always present, not gated). The entire existing scaffold body — the `<Input>`/"Greet" form and its `greet`/`greetMsg` logic (current `src/routes/+page.svelte:9-16,22-38`) — is deleted, not kept behind a flag; nothing in the app calls the `greet` Tauri command from anywhere else, so `greet` in `src-tauri/src/lib.rs:41` and its `generate_handler![]` entry (`lib.rs:1402`) can also be deleted in this same phase (dead scaffold code, not a seam anything else depends on).

**`src/routes/intro/+page.svelte`** rewritten to render `<WelcomeFlow variant="replay" />`, replacing the current bare `<Robot bind:this={robot} />` and its `.robot` sizing `<style>` block. `GlobalNav.svelte`'s existing `{ href: "/intro", label: m.nav_intro }` entry (`GlobalNav.svelte:8`) needs no change — same href, new content behind it.

## Phase F (Remaining): Locale keys

Deferred by the `feature-implement` skill's i18n rule until the user has approved the wording: every
new string in Phases D/E is currently a hard-coded English literal carrying a `TODO(i18n)` comment,
and `messages/*.json` is untouched (the dead `home_title`/`greet_*` keys are therefore still present
and are removed as part of this phase).


New Paraglide keys needed across `messages/en.json`, `messages/de.json`, `messages/ru.json` (mirrored in every file per the root `AGENTS.md` i18n rule): welcome-step copy (hero "Get started" button, welcome-panel body text, guided-journey framing), the two action steps' labels and completed-state text, the post-onboarding "get started with your first sandbox" screen's headline/description/button, and removal of the now-dead `greet_label`/`greet_placeholder`/`greet_button`/`home_title` keys (Phase E deletes their only call sites). `nav_intro` (`messages/en.json:4`) is unchanged — same tab, new destination content.

## Lifecycle / Cross-Cutting Concerns

**State freshness across the shared Podman store.** `src/lib/podman/setup.svelte.ts` (Phase B) is shared code, not a shared cached value: every mount point (`/projects`, `CreateVmStep`) calls `checkPodman()` itself and gets a fresh probe, so a Podman machine stopped or started outside the app between visits to different routes is always reflected correctly, matching `01_ideas.md`'s gotcha about the two screens disagreeing.

**Onboarding flag is the only new persisted state.** `onboarding_completed` on `AppSettings` is set exactly twice in the app: `completeOnboarding()` (Phase D's "Finish" button) and `resetOnboarding()` (the debug button). No other code path should write it — but `set_settings` replaces the whole file, so any writer that rebuilds `AppSettings` from a subset of fields resets the flag by omission. `src/routes/settings/+page.svelte` did exactly that and was corrected during implementation to spread the object it loaded; a future settings writer must do the same.

**The e2e harness seeds the flag.** A run starts from an empty settings directory, which the Phase C gate reads as "not onboarded" and hides the nav chrome every tab-navigating spec depends on. `onPrepare` in `e2e/wdio.conf.ts` writes `{"onboardingCompleted": true}` immediately after wiping `E2E_SETTINGS_DIR`; a welcome-flow spec resets the flag itself to reach the first-run path.

**Deletion, not deprecation, for scaffold code.** The `greet` command and its four associated locale keys are deleted outright in Phase E — per the root `AGENTS.md`'s comment-and-dead-code conventions, nothing in this feature needs a backwards-compatibility shim for a template leftover with no other callers.

**Both action-step renderings are light-tier reachable.** `base_image_ready` resolves through the existing `podman.image-exists` mockctl key, so `03_e2e.md` covers the not-yet-built path by scripting `FailAlways` on it (plus `podman.build` for the click) and the already-built path by leaving it unscripted. Likewise `CreateVmStep`: `podman.machine-list`'s `default_output` reports the default connection already running (`mockctl/mod.rs:250-252`), so the unscripted light tier lands on the completed rendering, and the first-run/setup branches need `podman.info` or `podman.machine-list` scripted to fail. No new mock code is required for either step.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Existing users (settings file predates this feature) see the full onboarding flow again after upgrading | `onboarding_completed` is absent from their on-disk `settings.json` and deserializes to `false` via `#[serde(default)]` | Expected per Resolved Decisions in `01_ideas.md`; if this proves confusing in practice, a migration that sets the field `true` for any settings file that already has sandboxes would need a new one-time check in `settings::load`, not attempted in this phase |
| `/projects` and the welcome flow disagree about whether the Podman machine is set up | One of the two call sites still holds local `$state` instead of importing `src/lib/podman/setup.svelte.ts` | Confirm both read the same module-scoped state from Phase B, not independent copies |
| `DownloadNecessitiesStep` shows "not started" even though the image already exists | `base_image_ready` wasn't called on mount, or its result was ignored | Call `base_image_ready` before rendering the action button, per Phase D |
| Debug restart button flips `onboarding_completed` but `/projects` still shows sandboxes/Podman as fully set up | Expected: the restart button only resets the onboarding flag, not the underlying Podman machine or base image, per `01_ideas.md`'s resolved scope for that button | Not a bug; the replayed flow's own live-state checks (`checkPodman`, `base_image_ready`) are what show "already done" correctly |
| Nav chrome flashes visible then hides on a fresh launch | `+layout.svelte` gated on `onboardingCompleted() === true` instead of `!== false`, so the `undefined` (loading) state was treated as "show chrome" | Gate on `!== false` per Phase C, so the loading state matches the eventual `false` case (hidden) rather than flashing the `true` case |
| The welcome flow itself disappears when onboarding is incomplete, leaving a blank window | `NavigationColumn` wraps `{@render children?.()}`, so gating the `<NavigationColumn>` element hides the routed page along with the chrome | Gate with the `{#if}`/`{:else}` in Phase C, rendering `children` in a bare `@container flex-1` div on the chromeless branch |
| A fresh WSL2 install needs a Windows restart but the VM step shows a generic "setup failed" | `createActionRunner` keeps only the error message; `PodmanRebootRequired` is only visible on `parseError(e).kind` | Read `podmanRebootRequired` from `src/lib/podman/setup.svelte.ts` (Phase B) rather than wrapping `setupPodman` in a runner — the constraint is recorded at `projects/+page.svelte:39-42` |
| `cargo check` fails on a missing `base_image_ready` for a second backend impl | There is no `backends/mock.rs`; `PodmanCliBackend` is the only `SandboxBackend` and the light tier mocks at the `ProcessRunner` seam | Implement the trait method once, on `PodmanCliBackend`; script `podman.image-exists` for light-tier behaviour |
| Saving anything on `/settings` throws the user back into the welcome flow on next launch | `set_settings` overwrites the whole file, and a page that rebuilds `AppSettings` from only the fields it edits lets `onboarding_completed` fall back to its serde default | Spread the object returned by `get_settings` into the payload, as `src/routes/settings/+page.svelte` and `src/lib/settings/onboarding.svelte.ts` both now do |
| Every light spec fails with `[role='tab'][data-value='/projects'] still not displayed` | A run starts from an empty settings directory, so the Phase C gate hides the nav chrome the specs navigate by | Keep `onPrepare`'s `{"onboardingCompleted": true}` seed in `e2e/wdio.conf.ts`; a welcome-flow spec resets the flag itself |
| Locale file shows English text on the new welcome or post-onboarding screen in `de`/`ru` | A new Phase F key was added to `messages/en.json` only | Mirror every new key into `messages/de.json` and `messages/ru.json` in the same change |
