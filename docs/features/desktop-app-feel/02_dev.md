---
type: dev-plan
title: "Desktop App Feel — Build Plan"
feature: desktop-app-feel
---
# Desktop App Feel — Build Plan

## Overview

This is the build plan for `docs/features/desktop-app-feel/01_ideas.md`, which
it continues and supersedes for implementation purposes. All six proposals
from that document are broken into six independent, separately-shippable
phases (A–F below); all six are implemented. No `src-tauri/` changes were
needed by any phase — every phase relocates or restyles existing frontend
surfaces and reuses existing Tauri commands unchanged, so none of the
parallel-worktree constraints (`e2e/AGENTS.md`) apply: no new host path, host
port, host-global external resource, bundled binary, or environment variable
was introduced anywhere in this plan. `pnpm check` is clean end to end;
`./build e2e light` is green (12/12 spec files) after Phases A–D, which is
the only tier with any existing coverage of the touched routes — Phases E and
F touch routes (`credentials`, `agent-auth`, the welcome flow) with no e2e
coverage at all, so those two are verified by `pnpm check` only. Phase B
needed one refinement beyond what this document originally specified — see
its Implementation notes below.

## The Concrete Model

Two new frontend seams underpin the phases below, both following conventions
already established elsewhere in `src/lib/`:

- **`src/lib/components/AdvancedSection.svelte`** — a thin wrapper around
  `Collapsible.Root`/`Trigger`/`Content` (`src/lib/components/ui/collapsible/`),
  closed by default, taking a `label` prop (defaulting to "Advanced") and
  `children`. Phases C, D, and F each wrap existing markup in this component
  instead of building their own disclosure.
- **`src/lib/sandbox/state.svelte.ts`** — a module-scoped rune store for
  deployment/host/capability data, following the pattern of
  `src/lib/workspace/state.svelte.ts` and `src/lib/network/state.svelte.ts`:
  top-level `$state`, exported getter functions, an idempotent
  `ensure*`-style starter. It replaces the local `hosts`/`deployments`/
  `capabilities` state and 3-second poll currently in
  `src/routes/projects/+page.svelte:27-120`, so both the sandbox list and the
  relocated Deploy/Undeploy/Sync controls in `WorkspaceDashboard.svelte` read
  the same live data instead of each fetching it separately.

## Resolved Decisions

Restating every Open Question from `01_ideas.md`:

1. **One feature or several?** Resolved: stays a single feature directory,
   implemented as six independent phases (A–F below), each shippable and
   revertable on its own. If a phase's scope or timeline diverges enough from
   the others during implementation, splitting its remainder into its own
   `docs/features/<name>/` directory remains available — no phase depends on
   another shipping in the same PR, only on shared components landing first
   (see per-phase dependencies below).
2. **`GlobalNav` toolbar vs. tabs restyle.** Stays deferred, per
   `04_improvements.md`. Not part of any phase in this plan.
3. **Start/Stop as a `Switch` vs. two buttons.** Resolved against introducing
   `bits-ui`'s `Switch` (`src/lib/components/ui/switch/switch.svelte`): it has
   no loading/busy visual state, only a boolean `checked`, so replacing
   `SandboxItem.svelte`'s Start/Stop with it would regress the existing
   `loading={action.busy === "start"}` affordance. The existing
   implementation already reads as a single toggle to the user — `start`/
   `stop` (`SandboxItem.svelte:133-151`) render as one `Button` at a time,
   switching on `project.status`, never two buttons side by side. Phase B
   keeps this control as-is; only the surrounding row changes.
4. **Where deployment/host/capability data lives once `SandboxItem` no
   longer drives Deploy/Undeploy.** Resolved: `src/lib/sandbox/state.svelte.ts`
   (introduced in Phase B), a shared store rather than a Dashboard-local
   re-fetch, so the list's status dot/host label and the Dashboard's
   Deploy/Undeploy controls never disagree.

## Implemented: Phase A — Fixed top nav

Files: `src/lib/workspace/NavigationColumn.svelte`.

Split the single `@container flex flex-col overflow-y-auto` div into an outer
non-scrolling wrapper that keeps `@container` (so `@max-[420px]:` queries in
`SandboxItem.svelte` and elsewhere keep resolving against the 380px column
width) and an inner `overflow-y-auto` region holding only
`{@render children?.()}` and `<ActivityCenter />`. `GlobalNav` and the
collapse-toggle `Button` stayed on the outer wrapper, outside the inner scroll
region. The collapsed-rail branch (`showControls && collapsed`) was
unaffected — it already has no scrolling content.

No new props, no store changes, no e2e selector changes (no `data-*`
attributes on the moved elements). `pnpm check` passes.

## Implemented: Phase B — Sandbox list as an appliance; `WorkspaceDashboard` absorbs administration

Files: `src/lib/sandbox/state.svelte.ts` (new), `src/routes/projects/+page.svelte`,
`src/routes/projects/SandboxItem.svelte`, `src/routes/projects/RemoteManager.svelte`
(new), `src/lib/workspace/WorkspaceDashboard.svelte`.

**`src/lib/sandbox/state.svelte.ts`** exports `ensureSandboxRemoteState()`
(idempotent starter, mirrors `ensureWorkspaceState()`'s `started` guard in
`src/lib/workspace/state.svelte.ts:73-82`), `deploymentFor(name): Deployment | undefined`,
`hostsList(): string[]`, and `capabilitiesFor(provider): ProviderCapabilities | undefined`.
Internally it owns the `invoke("sandbox_deployments")`/`invoke("list_sandbox_hosts")`/
`allCapabilities()` calls currently inline in `+page.svelte:45-60,88-89`, and
the 3-second poll currently started in `startLoadAndPoll()` (`+page.svelte:94-97`)
after `checkPodman()`/`podmanUnavailable()` gating — the store's starter takes
the same gating responsibility so it never floods a down Podman with failing
invocations, matching the existing comment on `+page.svelte:92-93`.

**`+page.svelte`** drops its local `hosts`/`deployments`/`capabilities` state
and `loadDeployments`/`loadHosts`/`reload`'s deployment half, calling
`ensureSandboxRemoteState()` from `startLoadAndPoll()` instead. `SandboxItem`
still receives `deployment={deploymentFor(project.name)}` for its status dot
and "→ host" label (existing markup, `SandboxItem.svelte:194-207`, unchanged).

**`SandboxItem.svelte`** removes `openFolder`, `toggleSync`, `deploy`,
`undeploy`, `destroyProject`, the host-picker state (`picking`/`chosenHost`),
and their buttons (lines 216-260, 286-304). The action rail keeps only the
existing Start/Stop `Button` (lines 237-249, per Resolved Decision 3) and the
existing "Open" `Button` (`openWorkspace`, lines 250-254). The row's status
dot, name, and deployment-target label (lines 194-207) are unchanged.

**`RemoteManager.svelte`** (new) takes `sandboxName`, `workspacePath`,
`action` (`ReturnType<typeof createActionRunner>`), following the prop
convention already used by `PackageManager.svelte`/`AgentManager.svelte`/
`PortsManager.svelte`. It renders the Sync `Switch` and Deploy/Undeploy
`Button`/host-picker UI moved from `SandboxItem.svelte`, reading
`deploymentFor(sandboxName)`/`hostsList()`/`capabilitiesFor(provider)` from
`src/lib/sandbox/state.svelte.ts` instead of props — `provider` comes from the
`Project` record `WorkspaceDashboard.svelte:61`'s existing `list_sandboxes`
call already fetches (that call currently only reads `workspace_path` off the
result; it starts also reading `provider`).

**`WorkspaceDashboard.svelte`** gains three additions: a Folder-reveal
`Button` (moved `openFolder` logic) next to the existing `×` close button in
the header (lines 71-81); a `<RemoteManager>` section alongside
`<PackageManager>`/`<AgentManager>`/`<PortsManager>` (lines 114-116); and a
Destroy `Button` (moved `destroyProject` logic, including its
`UnsyncedChanges` confirm-and-force-retry branch, `SandboxItem.svelte:100-117`)
placed at the bottom of the page, separated from the launcher row rather than
adjacent to Start/Stop, per `01_ideas.md`'s "Destroy demoted" proposal — this
page is reached deliberately, not scanned as a list, so no additional
confirmation UI beyond the existing `confirm()` call is added.

**E2E impact is larger than a selector tweak.** `e2e/test/pageobjects/project-row.component.ts`
owns `deployButton`/`undeployButton`/`destroyButton`/`syncSwitch` and the
higher-level `deployTo()`/`undeploy()`/`destroy()`/`toggleSyncOffOn()`
helpers, all scoped to the list row (`li[data-sandbox="..."]`). **11 spec
files across all three tiers call into them**: `standard/expose-port.spec.ts`,
`standard/embedded-workspace.spec.ts`, `standard/agent-web.spec.ts`,
`standard/runtime-management.spec.ts`, `standard/opencode-agent.spec.ts`,
`standard/base-image-remote.spec.ts`, `standard/base-image-build.spec.ts`,
`standard/agent-setup.spec.ts`, `standard/deploy-remote.spec.ts`,
`heavy/opencode-forward.spec.ts`, `light/microsandbox-provider-ui.spec.ts`.
None of those specs care where the controls render — they only care that
`ProjectRow`'s methods still work. Phase B therefore keeps `ProjectRow`'s
public method names (`deployTo`/`undeploy`/`destroy`/`toggleSyncOffOn`/
`start`/`stop`) stable and has them delegate to the relocated controls
(driving `WorkspaceDashboard` via `row.openWorkspace()` first, then acting on
`e2e/test/pageobjects/workspace-dashboard.page.ts`'s new deploy/undeploy/
sync/destroy getters — extended there in this phase, mirroring its existing
`packageInput`/`installButton` pattern), rather than requiring the 11
dependent spec files to change at all. `e2e/test/pageobjects/projects.page.ts`
is unaffected — it never referenced these getters directly.

**Implementation notes (two refinements beyond the text above):**

1. **`src/lib/sandbox/state.svelte.ts` also owns the sandbox list itself, not
   only deployment/host/capability data.** The list (`SandboxItem.svelte`, in
   `NavigationColumn`) and the Dashboard (`WorkspaceDashboard.svelte`, in
   `WorkspaceContent`) render as *siblings* under `+layout.svelte`, both
   mounted simultaneously whenever a workspace is open — not
   ancestor/descendant — so a Destroy triggered from the Dashboard had no prop
   path back to the list's `projects` array. Filtering the list by the
   `activity.svelte.ts` container-event overlay (`status === "removed"`) was
   considered and rejected: `ProviderCapabilities.eventStream` is `false` for
   the microsandbox provider (`src-tauri/src/provider/msb.rs`), so a destroyed
   microsandbox sandbox would never receive that status and would stay listed
   forever — this would have broken
   `light/microsandbox-provider-ui.spec.ts`'s destroy step. Instead, the store
   now polls `list_sandboxes` alongside `sandbox_deployments` every 3 s and
   exposes `sandboxesList()`/`refreshSandboxState()`; `+page.svelte` reads
   from it (still overlaying live container status from `activity.svelte.ts`
   for between-poll responsiveness) instead of loading its own copy, and
   every relocated mutating action (deploy/undeploy/sync/destroy in
   `RemoteManager.svelte`/`WorkspaceDashboard.svelte`) calls
   `refreshSandboxState()` on success so both surfaces update immediately
   rather than waiting out the poll.
2. **One of the 11 dependent specs needed a small change.**
   `light/microsandbox-provider-ui.spec.ts` read `row.deployButton` as a
   getter directly (checking capability-gating visibility), rather than going
   through a stable `ProjectRow` method — the one call shape the "11
   dependent specs" analysis above didn't cover. Updated to open the
   Dashboard (`row.openWorkspace()` + `workspaceDashboard.waitForRendered()`)
   and check `workspaceDashboard.deployButton` instead. All other dependent
   specs were unaffected, confirmed by a full `./build e2e light` pass (12
   spec files, all green).

`RemoteManager.svelte` also takes a `provider: ProviderId` prop (not
enumerated in the original file list above, which only named
`sandboxName`/`workspacePath`/`action`) — `capabilitiesFor(provider)` needs a
value only `WorkspaceDashboard.svelte`'s `list_sandboxes` call has.

`e2e/test/pageobjects/workspace-dashboard.page.ts`'s new getters are scoped
under a `root` getter (`$("[data-dashboard]")`, added to
`WorkspaceDashboard.svelte`'s `<main>`) rather than left as bare global `$()`
selectors like the pre-existing `packageInput`/`installButton`: the list and
Dashboard both render `StatusRow`, so an unscoped `errorRow` lookup could
resolve to the list row's error line instead of the Dashboard's.

## Implemented: Phase C — `AdvancedSection` component and deferred provider picker

Files: `src/lib/components/AdvancedSection.svelte` (new),
`src/routes/projects/CreateSandboxItem.svelte`.

`AdvancedSection.svelte` is built here (first consumer) per The Concrete
Model above — a thin wrapper around `Collapsible.Root`/`Trigger`/`Content`,
closed by default, taking `label` (default `"Advanced"`, TODO(i18n)) and
`children`, and forwarding any other props (e.g. a `data-*` e2e hook, layout
classes) onto `Collapsible.Root` so a page with more than one instance can
scope selectors without depending on the (still English-only) trigger label.
`CreateSandboxItem.svelte` wraps the "Runs on" `Field.Field` in
`<AdvancedSection>`; the `data-provider-picker` div and its
`data-provider-option` buttons are unchanged, only their container. `provider`
still defaults from `get_settings`'s `defaultProvider` exactly as today.

E2E: `e2e/test/pageobjects/projects.page.ts`'s `chooseProvider()` now expands
the picker's `AdvancedSection` first (`button=Advanced`, text-based to match
this file's existing selector convention) when the option isn't already
visible.

## Implemented: Phase D — Settings and Models pages adopt `AdvancedSection`

Files: `src/routes/settings/+page.svelte`, `src/routes/models/+page.svelte`.
Depends on Phase C (`AdvancedSection.svelte`).

**`settings/+page.svelte`**: the existing "Advanced" `Field.Set`
(already labelled via `m.settings_advanced()`) moved inside
`<AdvancedSection data-advanced-section="tuning">`, closed by default. The
"Debugging" `Field.Set` moved inside its own
`<AdvancedSection label="Troubleshooting" data-advanced-section="troubleshooting">`
rather than merging into the same disclosure as Advanced, since
`resetPodmanMachine` is a diagnostic action, not a tunable preference.
Appearance is unchanged, always visible; so is the Save button and the
runtime-explainer section at the bottom.

**`models/+page.svelte`**: the default view shows only `displayModels[0]`
(`topEntry`) — always the top-ranked entry, recommended-rated or not, so a
constrained-hardware host where nothing rates `"recommended"` still gets a
non-empty default rather than a special-cased fallback branch — with a single
action that chains `downloadModel` then `startModel` (`downloadAndStart()`)
for that model. The row markup itself is a shared `{#snippet modelRow(entry,
primary)}` so the default row and the full-catalogue rows behind Advanced
don't duplicate markup; `primary` only changes one thing — a not-yet-downloaded
llama model shows one "Download & Start" button (`data-start`, TODO(i18n)
label) instead of a disabled Start plus a separate Download button. The specs
block, the rest of `displayModels`, and the chat test box moved inside
`<AdvancedSection label="Choose a different model" data-model-advanced>`;
`action.error` stays outside it (always visible — a default-row failure must
not require expanding Advanced to see).

E2E: `e2e/test/pageobjects/models.page.ts`'s `open()` now expands the single
AdvancedSection immediately (every current caller needs at least the specs
panel), and `startDownload()`/`startInference()` were made position-agnostic
— `startDownload()` falls back to the combined `[data-start]` button when
`button=Download` isn't there, and `startInference()` no-ops once
`[data-start]` is already gone (replaced by Stop) — so
`heavy/local-llm.spec.ts`'s four-step download→start flow keeps working
whether its target model ends up the default row or a secondary one, without
the spec itself knowing which. `settings.page.ts` gained the equivalent
`tuningTrigger`/`troubleshootingTrigger` + `#expand()` (checks
`data-state==="open"` before clicking) and `open()`/`activeEngine()`/
`selectEngine()`/`resetPodmanMachine()` now expand the relevant section
first — `local-llm-ui.spec.ts`'s "gates Start on a non-downloaded llama
model" test was rewritten (the default model row is never gated on
`isDownloaded` now, by design; that check still applies to secondary rows,
whose code path is unchanged).

Verified: `./build e2e light` — 12/12 spec files green after both phases.

## Implemented: Phase E — `agent-auth` window adopts the credentials page's session UI

Files: `src/lib/agents/AuthSessionCard.svelte` (new),
`src/routes/credentials/+page.svelte`, `src/routes/agent-auth/+page.svelte`.

`AuthSessionCard.svelte` is a presentational component extracted from
`credentials/+page.svelte`'s inline `Collapsible.Content` markup: status text
per `authPhase()`, the "Open in browser" button over `authUrlDetected()`, the
code-paste input for `authPhase() === "awaiting-code"`, and a `showDetails`
toggle over `authOutputLines()`. It takes no session props — it reads
`src/lib/agents/authSession.svelte.ts`'s exported getters directly — and
takes callback props `onCancel: () => void` and
`onSubmitCode: (code: string) => void`. The code input's own text state now
lives inside the card (it owns the whole input+submit control), so
`credentials/+page.svelte`'s `submitCode()` changed shape from a no-arg
function reading module-local `codeInput` to `submitCode(code: string)`
reading the argument the card passes it; its own now-redundant
`codeInput`/`submittingCode`/`showDetails` state was removed.
`cancelOrDismiss()` is unchanged, passed through as `onCancel`.

`agent-auth/+page.svelte` drops its local `outputLines`/`authUrl`/
`showCodeInput`/`exitCode` state and its own `listen("agent-auth-output-<id>")`
call, calling `start(sessionId)` from `src/lib/agents/authSession.svelte.ts`
in `onMount` instead — that store's `start()` already attaches the listener
given an existing session id, which is exactly what this page has from its
`?session=` query param (already populated by `AgentManager.svelte`'s
`start_agent_auth` invoke before the window opens). Unlike
`credentials/+page.svelte`'s `logIn()`, it does not call `beginSigningIn()`
first: that covers the gap before a session id exists (during
`ensure_auth_container`/`install_agent`), which does not apply here since
`start_agent_auth` already resolved before this window opened. It renders
`<AuthSessionCard>` and keeps its own window-close-on-success
`setTimeout(() => getCurrentWindow().close(), 2000)` plus an `onDestroy` that
calls `cancel_agent_auth` if the window closes (native × button) before a
terminal exit code — both page-specific glue on top of the shared card,
since closing the window and cancelling an abandoned session are this page's
responsibility, not the card's. The `start_agent_auth` target (in-sandbox
container, `name: sandboxName`) is unchanged — only the UI layer and state
source change, matching `01_ideas.md`'s rejection of retiring the window
outright.

Each `WebviewWindow` (`AgentManager.svelte:154-159`) loads its own JS bundle
in a separate webview, so `authSession.svelte.ts`'s module-scoped `$state` in
the `agent-auth` window is a distinct instance from the one `credentials/+page.svelte`
uses in `main-navigation-panel` — no cross-window state collision.

No e2e coverage exists for either route (`e2e/test/` has zero references to
`credentials` or `agent-auth`), so this phase carries no page-object/spec
follow-up; verified with `pnpm check` only.

## Implemented: Phase F — Onboarding copy leads with outcomes

Files: `src/lib/welcome/CreateVmStep.svelte`, `src/lib/welcome/DownloadNecessitiesStep.svelte`.
Depends on Phase C (`AdvancedSection.svelte`) for the "technical details" disclosure.

Headings reframe around outcome rather than mechanism: "Create virtual
machine" → "Setting up your workspace" (`CreateVmStep.svelte`), "Download
necessities" → "Getting everything ready" (`DownloadNecessitiesStep.svelte`).
The routine explainer paragraph in each moved inside
`<AdvancedSection label="Technical details">`, right after the heading and
before the status/action content, same position it held before. Blocking
error copy — the `PodmanUnresponsive` branches and the WSL2 reboot message —
was untouched, stays outside any disclosure, always visible, since it is the
actual actionable instruction when a user hits it. The action button labels
(`actionLabel()` in `CreateVmStep.svelte`, "Download necessities" in
`DownloadNecessitiesStep.svelte`) were left as-is — only the two headings and
the routine explainer text were in scope.

No e2e coverage exists for the welcome/onboarding flow (`e2e/test/` has no
`WelcomeFlow`/`CreateVmStep`/`DownloadNecessitiesStep`/`data-welcome-*`
reference), so this phase carries no page-object/spec follow-up; verified
with `pnpm check` only.

## Lifecycle / Cross-Cutting Concerns

**i18n deferred.** Every phase above ships with the same `<!-- TODO(i18n) -->`
English-first convention already used throughout these files
(`src/AGENTS.md`, `feature-implement` skill: Paraglide translation happens
after the user approves the working feature, not during initial
implementation). No phase blocks on adding `messages/*.json` keys.

**No backend changes anywhere in this plan.** Every phase reuses existing
`#[tauri::command]`s unchanged (`start_sandbox`, `stop_sandbox`,
`deploy_sandbox`, `undeploy_sandbox`, `destroy_sandbox`, `enable_sync`/
`disable_sync`, `get_settings`, `start_agent_auth`, `base_image_ready`/
`build_base_image`, etc.) — confirmed by reading each seam above before
listing it in a phase.

**`src/lib/sandbox/state.svelte.ts` persists for the life of the webview**,
same as `src/lib/workspace/state.svelte.ts` and `src/lib/activity/activity.svelte.ts`
(`src/routes/+layout.svelte:26-27`'s comment on why activity's subscription is
never torn down) — its poll is not cleared on route change, only gated by the
same Podman-readiness check `+page.svelte` already applies before starting.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `@max-[420px]:` queries in `SandboxItem.svelte` stop firing after Phase A | `@container` ended up on the inner scroll-only region instead of the outer wrapper | Keep `@container` on the outer, non-scrolling wrapper in `NavigationColumn.svelte`; only `overflow-y-auto` moves to the inner region |
| Deploy/Undeploy button in `RemoteManager.svelte` never enables | `capabilitiesFor(provider)` returns `undefined` because `WorkspaceDashboard.svelte`'s `list_sandboxes` call was never updated to read `provider` off the result (Phase B) | Read `provider` from the same `Project` record `WorkspaceDashboard.svelte:61` already fetches for `workspace_path` |
| Sandbox list and `WorkspaceDashboard` show conflicting deploy/sync status after Phase B | Both surfaces read from `src/lib/sandbox/state.svelte.ts`, but a mutating action (`RemoteManager.svelte`'s deploy/undeploy/toggleSync) forgot to trigger the store's own refresh, not just local component state | Ensure `RemoteManager.svelte`'s action handlers call back into `src/lib/sandbox/state.svelte.ts`'s refresh path, not a local `onChanged` prop that only the old `+page.svelte` reload wired up |
| `agent-auth` window shows no live output after Phase E | `agent-auth/+page.svelte`'s `onMount` calls `authSession.svelte.ts`'s `start(sessionId)` before `sessionId` is parsed from `?session=`, or omits the call entirely | Parse `sessionId` from `URLSearchParams` first (existing `+page.svelte:47`), then call `start(sessionId)` from the shared store in the same `onMount` |
| One of the 11 dependent specs (see Phase B) fails with "button not found" after Phase B | `ProjectRow`'s `deployTo`/`undeploy`/`destroy`/`toggleSyncOffOn` were edited to look for buttons still on the list row, which Phase B removes | Have those methods call `row.openWorkspace()` then act through `workspace-dashboard.page.ts`'s deploy/undeploy/sync/destroy getters, keeping the method names and call sites in the 11 spec files unchanged |
| Folder-reveal or Destroy button in `WorkspaceDashboard.svelte` has no e2e getter | `workspace-dashboard.page.ts` was not extended with the new getters `ProjectRow`'s delegated methods need | Add `folderButton`/`deployButton`/`undeployButton`/`destroyButton`/`syncSwitch` getters to `workspace-dashboard.page.ts` in the same phase that moves the Svelte controls, before updating `ProjectRow` to call them |
