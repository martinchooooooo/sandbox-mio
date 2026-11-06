---
type: ideas
title: "Desktop App Feel"
feature: desktop-app-feel
---
# Desktop App Feel

## Overview

This document covers a set of frontend-only UI/UX changes across `src/routes/` and
`src/lib/` that make Sandbox-mio read as a native desktop app rather than a web
dashboard, and lower the barrier for non-technical users. No `src-tauri/` seam
changes are required — every change relocates or re-styles existing frontend
surfaces and reuses existing Tauri commands. The changes span six pages
(`src/routes/+layout.svelte` via `src/lib/workspace/NavigationColumn.svelte`,
`src/routes/projects/+page.svelte`, `src/routes/settings/+page.svelte`,
`src/routes/models/+page.svelte`, `src/routes/agent-auth/+page.svelte`,
`src/lib/welcome/WelcomeFlow.svelte`) and are unified by one shared UI
convention proposed below (collapsed-by-default disclosure for
technical/administrative detail).

## Current State

**Top nav scrolls with content.** `src/lib/workspace/NavigationColumn.svelte:29-34`
wraps `GlobalNav` (`src/lib/components/GlobalNav.svelte`) and the routed page
`children` in one `overflow-y-auto` container, so the nav scrolls out of view
with the page.

**Sandbox list is administratively dense.** `src/routes/projects/SandboxItem.svelte:192-267`
renders up to six inline actions per row: Folder reveal (`openFolder`, line 172),
Sync toggle (`toggleSync`, line 119, only for deployed sandboxes), Workspace
(`openWorkspace`, line 158), Deploy/Undeploy (`deploy`/`undeploy`, lines 78-98),
Start/Stop (`start`/`stop`, lines 133-151), and Destroy (`destroyProject`,
line 100). Destroy renders as a `variant="destructive"` button at the same
position and size as every other action.

**The workspace destination already exists.** `src/lib/workspace/WorkspaceDashboard.svelte`
is the per-sandbox surface rendered by `+layout.svelte` once a sandbox is
opened (`docs/features/consolidate-app-container/02_dev.md`). It already hosts
the app launcher (`LAUNCHABLE_APPS`, `src/lib/workspace/apps.ts`),
`src/routes/projects/PackageManager.svelte`, `AgentManager.svelte`, and
`PortsManager.svelte`. It has no folder-reveal, deploy/undeploy, sync, or
destroy controls today — those still live only on `SandboxItem`.

**Project creation shows the provider picker unconditionally.**
`src/routes/projects/CreateSandboxItem.svelte:119-134` renders a "Runs on"
button group (`PROVIDERS`, `src/lib/providers.ts`) inline, always visible. It
is already seeded from the persisted `defaultProvider` setting on mount
(lines 25-32), so the picker only matters when a user wants to override that
default for one sandbox.

**Settings and Models pages mix audiences.** `src/routes/settings/+page.svelte`
uses three `Field.Set` blocks (lines 137-246): Appearance (theme, language —
always relevant), Advanced (default provider, inference engine, GPU layers,
context size — already labelled "Advanced" via `m.settings_advanced()` but
rendered fully expanded), and Debugging (`resetPodmanMachine`, line 126, a
troubleshooting action already isolated in its own `Field.Set` but still
always visible). `src/routes/models/+page.svelte` renders a specs block
(`data-specs`, lines 231-265: CPU cores, RAM, GPU names), a full ranked model
catalogue (`displayModels`, line 84), and a raw chat-completion test box
(`data-chat`, lines 344-360) all in the default view.

**`agent-auth` duplicates rather than reuses the credentials page's flow.**
`src/routes/agent-auth/+page.svelte` opens as a separate native `WebviewWindow`
(constructed in `openAgentAuth()`, `src/routes/projects/AgentManager.svelte:140-161`,
540×460px) and shows raw CLI output (`outputLines`, monospace, `bg-zinc-950`)
as its only UI, with its own local `sessionId`/`outputLines`/`authUrl` state
listening directly to the `agent-auth-output-<id>` event. This duplicates
`src/lib/agents/authSession.svelte.ts`, the shared session store that
`src/routes/credentials/+page.svelte` already uses to drive an equivalent
flow with a friendlier UI: a Sign in/Log out button, connection status text,
and raw output hidden behind a `showDetails` toggle (lines 242-297). The two
flows authenticate different targets — `agent-auth` runs the login command
inside the sandbox's own container (`start_agent_auth` with
`name: sandboxName`), while the credentials page runs it in an ephemeral
`mio-auth-<serviceId>` container and captures a reusable credential bundle
(`capture_agent_credentials`, `src/routes/credentials/+page.svelte:82-89`) —
so the window itself is not redundant, but its UI and state layer are.

**Onboarding leads with infrastructure vocabulary.** `src/lib/welcome/WelcomeFlow.svelte`
drives `src/lib/welcome/CreateVmStep.svelte` and
`src/lib/welcome/DownloadNecessitiesStep.svelte` as the two "First two stops"
setup steps. Their headings and error copy are infra-first: "Create virtual
machine" (`CreateVmStep.svelte:56`), "Podman started but never answered...
details are in `mio.log`" (lines 68-71), "Windows needs to restart to finish
installing WSL2" (`DownloadNecessitiesStep.svelte:88`).

**Existing disclosure primitive.** `src/lib/components/ui/collapsible/` (bits-ui
backed `Collapsible.Root`/`Trigger`/`Content`) is already in use in
`src/routes/credentials/+page.svelte:201-299` for the per-service auth card,
proving the component works for collapsed-by-default technical detail. No
accordion or disclosure convention exists yet at the page level (Settings and
Models use always-expanded `Field.Set` sections instead).

## Proposed Model

### Shared "Advanced" disclosure convention

Every other proposal below collapses some technical/administrative detail
behind a disclosure that is closed by default. Define this once as a shared
convention rather than four bespoke implementations: a thin wrapper around the
existing `Collapsible.Root`/`Trigger`/`Content` (`src/lib/components/ui/collapsible/`)
that always renders a closed-by-default trigger labelled "Advanced" (or a
context-specific label like "Choose a different model" on the Models page),
consistent chevron/typography, and no persisted open state (each page visit
starts collapsed). This becomes the seam every other section below cites.

### Fixed top nav

`NavigationColumn.svelte` splits its single `overflow-y-auto` div (lines 29-34)
into two: `GlobalNav` (and, when shown, the collapse toggle) renders outside
the scrolling region, and only `{@render children?.()}` plus `ActivityCenter`
scroll beneath it. The `@container` context children rely on for their
`@max-[420px]:` container queries (`src/routes/projects/SandboxItem.svelte`,
`docs/features/nav-responsive/01_ideas.md`) must stay on the outer wrapper so
those queries keep resolving against the 380px column width, not the inner
scroll region.

### Sandbox list as an appliance: power + open only

`SandboxItem.svelte`'s row keeps the status dot, name, and deployment target
(existing lines 194-207), and reduces its action rail to two controls: a
Start/Stop toggle (recommend restyling `start`/`stop`, lines 133-151, as a
single on/off `Switch` rather than two `Button`s — same primitive already
used for the sync toggle) and an "Open" action (`openWorkspace`, line 158)
that takes the user into `WorkspaceDashboard`. Folder reveal (`openFolder`),
Sync toggle (`toggleSync`), Deploy/Undeploy (`deploy`/`undeploy`), and Destroy
(`destroyProject`) move into `WorkspaceDashboard.svelte`, joining
`PackageManager`/`AgentManager`/`PortsManager` as another administrative
section of that page. `WorkspaceDashboard.svelte` already loads `workspacePath`
via `list_sandboxes` (line 61) and already receives `sandboxName`, so it has
what these actions need without new props; `deployment`/`hosts`/`capabilities`
currently computed in `src/routes/projects/+page.svelte:27-60` need an
equivalent load inside the Dashboard (or a shared store) since the Dashboard
does not currently subscribe to that data.

### Destroy demoted out of the primary action set

Because Destroy moves into `WorkspaceDashboard` under this proposal, the
misclick risk of a destructive action sitting at Start's visual weight in the
list is resolved by relocation, not by adding new interaction chrome (no
context menu needed). Within the Dashboard, render Destroy with a lower
visual weight than the launcher buttons — e.g. at the bottom of the page,
still `variant="destructive"` but not adjacent to Start/Stop — since the
Dashboard is already the place a user goes deliberately, not a list they
scan.

### Provider picker deferred behind Advanced

`CreateSandboxItem.svelte`'s "Runs on" `Field.Field` (lines 119-134) moves
inside the shared Advanced disclosure proposed above, collapsed by default.
The `provider` state (line 19) keeps defaulting from `get_settings`'s
`defaultProvider` (lines 25-32) exactly as today; the disclosure only changes
whether the picker is visible before the user opts in.

### Settings split into Preferences / Advanced / Troubleshooting

`settings/+page.svelte`'s existing three `Field.Set` blocks map directly onto
the shared Advanced convention: Appearance (lines 137-155) stays always
visible as "Preferences"; the existing "Advanced" `Field.Set` (lines 157-227,
already labelled via `m.settings_advanced()`) becomes collapsed by default
using the shared component instead of a plain expanded `Field.Set`; and
Debugging (lines 229-246) is relabelled/kept as a separate collapsed
"Troubleshooting" disclosure rather than merged into Advanced, since
`resetPodmanMachine` is diagnostic (recovering a stuck machine), not a
preference a user tunes.

### Models page defaults to the one recommended model

`models/+page.svelte`'s default view shows only the top-ranked entry from
`displayModels` (line 84, already sorted recommended-first by
`recommendModels`, `src/lib/models/recommend.ts`) with a single "Download &
Start" action that chains `downloadModel` then `startModel` (lines 101-161).
The full catalogue list, the specs block (`data-specs`, lines 231-265), and
the chat test box (`data-chat`, lines 344-360) move behind an Advanced
disclosure labelled around "Choose a different model" / "Why this model?",
reusing the same convention.

### `agent-auth` window adopts the credentials page's session UI

Extract the friendly-flow markup from `src/routes/credentials/+page.svelte:242-297`
(status text, "Open in browser" button, code-paste input, `showDetails`
toggle over `authOutputLines()`) into a shared component consumed by both
`credentials/+page.svelte` and `src/routes/agent-auth/+page.svelte`.
`agent-auth/+page.svelte` switches from its local `sessionId`/`outputLines`/
`authUrl` state (lines 11-19, 45-76) to the existing
`src/lib/agents/authSession.svelte.ts` store credentials already uses,
removing the duplicate `agent-auth-output-<id>` listener. The window keeps
authenticating inside the sandbox's own container (`start_agent_auth` with
`name: sandboxName`, unchanged) — only the UI layer and state source change.

### Onboarding copy leads with outcomes, not infrastructure

`CreateVmStep.svelte` and `DownloadNecessitiesStep.svelte` keep their existing
logic (`prepareMachine`, `base_image_ready`/`build_base_image`) but reframe
headings and default-state copy around what the user gets ("Setting up your
workspace" / "Getting everything ready") rather than how ("Create virtual
machine" / "Download necessities"). Infra-specific detail — the `mio.log`
pointer (`CreateVmStep.svelte:70`), the WSL2 reboot message
(`DownloadNecessitiesStep.svelte:88`) — stays surfaced when it is the actual
blocking error (a user who hits a WSL2 reboot needs that exact instruction),
but the routine "why this step exists" explainer text
(`CreateVmStep.svelte:58-60`, `DownloadNecessitiesStep.svelte:67-69`) moves
behind a small expandable "technical details" line using the same Advanced
convention.

## Technology Choices with Tradeoffs

### Shared Advanced-disclosure component vs. per-page `Collapsible` usage

**Chosen**: a single shared wrapper component around the existing
`Collapsible.Root`/`Trigger`/`Content` (`src/lib/components/ui/collapsible/`),
consumed by `CreateSandboxItem.svelte`, `settings/+page.svelte`,
`models/+page.svelte`, and (optionally) the onboarding steps.

**Alternative: reuse `Collapsible` directly in each page, styled ad hoc.**
Rejected — this is the option already partially in place (credentials page
uses `Collapsible` directly) and it is exactly what produces inconsistency:
four pages each choosing their own trigger label, default state, and spacing
undermines the "unified feel" goal this whole feature is scoped around. A
shared component makes the default-collapsed behavior and visual treatment
a single seam to change later.

### Sandbox-list admin actions move into `WorkspaceDashboard` vs. a right-click context menu on the list

**Chosen**: relocate Folder reveal, Sync, Deploy/Undeploy, and Destroy into
`WorkspaceDashboard.svelte`.

**Alternative: keep the actions on `SandboxItem` but hide them behind a
right-click context menu.** Rejected as the primary mechanism — the app has
no context-menu pattern anywhere today, so this would be new interaction
surface to design (positioning, keyboard/touch fallback, empty states) for a
problem the app already has an answer to. It also does not fully satisfy the
user's "administrative tasks happen in the workspace" framing, since the
actions would still be reachable, just occluded, from the list itself.
Relocating into the already-built `WorkspaceDashboard` is strictly less new
surface area.

### Sync toggle: `Switch` stays inline on the list vs. moves to the Dashboard

**Chosen**: moves to `WorkspaceDashboard`, alongside Deploy/Undeploy.

**Alternative: keep the sync `Switch` (`SandboxItem.svelte:222-225`) on the
list row since it is a lightweight, frequent toggle, not a rare
administrative action like Destroy.** Considered but rejected for
consistency — sync only exists for deployed sandboxes, so keeping it on the
list means the row's action count still varies by deployment state, which is
the same "row density varies" problem being removed. Moving it to the
Dashboard keeps the list's row shape identical for every sandbox regardless
of deployment status.

### `agent-auth` window: reform vs. retire in favor of the credentials page

**Chosen**: reform the window's UI and state layer to match the credentials
page's pattern, keep the window.

**Alternative: retire the `agent-auth` route/window entirely and route all
agent authentication through `credentials/+page.svelte`.** Rejected — the two
flows authenticate different targets (in-sandbox container vs. ephemeral
shared-credential container, see Current State) and `AgentManager.svelte`'s
"Use saved login" path (`useSavedLogin`, lines 163-174) already depends on a
credential bundle existing from the credentials-page flow separately from a
fresh in-sandbox login. Collapsing them into one flow would change which
container ends up authenticated, not just which UI presents the login —
out of scope for a UI-feel feature.

## Validation

No spike was performed. Every change here relocates or restyles an existing,
already-shipped surface (`WorkspaceDashboard.svelte`'s manager sections,
`Collapsible` in `credentials/+page.svelte`, `authSession.svelte.ts`) rather
than introducing new backend behavior or new frontend infrastructure, so the
technical risk is low in the same way `docs/features/nav-responsive/01_ideas.md`
treated its `SandboxItem` container-query pattern as "production-proven;
extending it... is a replication task, not a new experiment." The build plan
(`02_dev.md`) should still confirm each relocation against
`docs/features/consolidate-app-container/02_dev.md`'s Dashboard-visibility
invariants before moving actions into `WorkspaceDashboard.svelte`.

## Open Questions

1. **One feature or several?** This document intentionally covers all six
   areas under one ideation doc because the unifying decision — the shared
   Advanced-disclosure convention — only stays coherent if it is designed
   once. Recommendation: keep a single `02_dev.md`, but implement in
   independent, separately-shippable phases (fixed nav; sandbox-list +
   Dashboard relocation; create-flow Advanced; Settings/Models Advanced;
   `agent-auth` reform; onboarding copy), matching the `feature-implement`
   skill's phase-by-phase convention. Precedent in this repo leans the other
   way (`nav-responsive`, `sandbox-list-create`, `credentials-inline-auth`,
   `welcome-onboarding` are separate feature directories) — if a future
   agent finds the phases diverging in scope or timeline, splitting this
   directory's remaining phases into their own feature directories remains
   an option `02_dev.md` can decide.
2. **`GlobalNav` toolbar vs. tabs restyle.** `GlobalNav.svelte` currently
   renders `Tabs.List` styled as browser-style tabs, which reads more "web
   app" than "desktop app." Recommendation: out of scope for this feature —
   it is a visual restyle independent of the disclosure/relocation work above
   and touches a component every page depends on, so it deserves its own
   scoped feature rather than riding along here. Recorded in
   `04_improvements.md`.
3. **Should the Start/Stop control become a single `Switch` or stay two
   buttons?** The Proposed Model recommends a `Switch` for symmetry with the
   sync toggle, but `start`/`stop` (`SandboxItem.svelte:133-151`) currently
   surface distinct loading/error states per action via `action.busy`
   (`src/lib/actions.svelte`). `02_dev.md` should confirm a single `Switch`
   can still express "starting…"/"stopping…" busy states clearly before
   committing to it over two buttons.
4. **Where does deployment/host/capability data live once `SandboxItem` no
   longer needs it?** `src/routes/projects/+page.svelte:27-60` currently
   loads `hosts`/`deployments`/`capabilities` once and passes them down to
   every `SandboxItem`. If Deploy/Undeploy moves to `WorkspaceDashboard`,
   `02_dev.md` must decide whether the Dashboard re-fetches this itself per
   sandbox or whether it becomes a shared module-scoped store
   (`src/AGENTS.md`'s rune-store convention, e.g. alongside
   `src/lib/workspace/state.svelte.ts`).

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `@max-[420px]:` container queries in `SandboxItem.svelte` stop firing after the nav-fix split | The `@container` context moved to a different element than the one children actually measure against | Keep `@container` on the outer wrapper in `NavigationColumn.svelte`, not on the inner scroll-only region, so the 380px column width (`NAVIGATION_PANEL_EXPANDED_W`, `src-tauri/src/workspace/mod.rs`) still resolves the query |
| Deploy/Undeploy moved into `WorkspaceDashboard.svelte` but the button never enables | `hosts`/`capabilities` are loaded once in `src/routes/projects/+page.svelte` and never reached the Dashboard | Resolve Open Question 4 before implementing: either fetch `list_sandbox_hosts`/`allCapabilities` inside the Dashboard or lift them into a shared store |
| Sandbox list and `WorkspaceDashboard` show stale/conflicting status after an action moves | Both surfaces poll or react to `src/lib/activity/activity.svelte.ts` independently; relocating an action does not relocate its reload trigger | Ensure the relocated action's `onChanged`/reload call updates whichever store the list still reads from, not just local Dashboard state |
| `agent-auth` window shows no live output after switching to `authSession.svelte.ts` | The shared store's `start()` (`src/lib/agents/authSession.svelte.ts`) expects to be initiated with the session id it manages, not attached mid-stream the way the old local listener was | Call the shared store's `start(sessionId)` from `agent-auth/+page.svelte`'s `onMount` in place of the local `listen()` call, mirroring `credentials/+page.svelte`'s `logIn()` |
