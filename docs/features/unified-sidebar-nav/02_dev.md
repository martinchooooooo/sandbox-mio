---
type: dev-plan
title: "Unified Sidebar Navigation: Build Plan"
feature: unified-sidebar-nav
---
# Unified Sidebar Navigation: Build Plan

This document is the build plan for replacing the top page tabs, the multi-sandbox switcher bar and the docked navigation column with one persistent app sidebar, a full-width content area and a single-sandbox header. It continues `docs/features/unified-sidebar-nav/01_ideas.md` and supersedes it wherever the two differ. Status: Phases A to I are implemented and pass `pnpm check` and the `light` and `standard` tiers; the `heavy` tier has not yet run.

## Concrete Model

### Shell layout

`src/routes/+layout.svelte` renders, once onboarding is complete, a horizontal flex of the app sidebar and the content area:

- **App sidebar**, `src/lib/workspace/AppSidebar.svelte`: `w-60` (240px) expanded or `w-12` (48px) collapsed, matching `NAVIGATION_PANEL_EXPANDED_W` and `NAVIGATION_PANEL_RAIL_W` in `src-tauri/src/workspace/mod.rs`. The collapse flag is Rust-owned (`set_panel_collapsed`, read through `isCollapsed()` in `src/lib/workspace/state.svelte.ts`) because the pane overlay rectangle depends on it. The collapse toggle is always shown, not only while a sandbox is open.
- **Content area**: when a sandbox is in view, `src/lib/workspace/SandboxView.svelte` (the sandbox header, then the Manage dashboard, the native chat, or nothing under a webview pane); otherwise the routed page inside the `data-route-outlet` element, which carries `@container` so pages size against the content area. The routed page is unmounted while a sandbox is in view (Resolved Decision 9).

During onboarding (`onboardingCompleted() === false`) the layout stays chromeless, as today.

### View state

`src/lib/workspace/state.svelte.ts` stays the single owner of the `workspace://state` subscription and gains:

- `viewSandboxName()`: the sandbox shown in the content area, the Rust-reported active pane's sandbox, else the client-side `activeSandbox`, else null (a routed page shows).
- `activeSandbox` follows Rust: when `active_app` changes to a pane label, `applyState` sets `activeSandbox` to that pane's sandbox (Resolved Decision 10).
- `sandbox_last_pane` mirrored from the Rust state.
- A per-sandbox "Manage was the last view" memory, set when Manage is shown and cleared when one of the sandbox's panes becomes active. `restoreTarget(name)` reads both and returns the remembered pane only while it is still open, else null (Manage).

`src/lib/workspace/navigation.svelte.ts` holds every navigation action so no component re-implements them: `showPage(href)` (`goto`, then clear `activeSandbox` and `hide_active_pane`, so the previous page never flashes), `openSandbox(name, workspacePath, { restore })` (start `watch_workspace_ports`, then either `switch_pane` to the remembered pane or `hide_active_pane` to show Manage), `showManage(name)`, `activatePane(pane)`, `closePane(pane)`, `closeSandbox(name)`, and the "New sandbox" focus request read by `src/routes/projects/CreateSandboxItem.svelte`.

### Nav-list primitive

`src/lib/components/ui/nav-list/` is a hand-built, sandbox-agnostic primitive in the retro style: `Root` (a `<nav>` with a required `aria-label` and a `collapsed` prop, wrapping a bits-ui `Tooltip.Provider`), `Group` (an optional heading hidden in the rail), and `Item` (a button with an `icon` snippet, a `label`, an `active` flag, an optional `trailing` snippet, and a tooltip shown only while collapsed). Items share their base classes with `src/lib/components/ui/section-nav/section-nav-item.svelte` through `src/lib/components/ui/section-nav/styles.ts`, so both lists look like one family. While a webview pane is shown, `Item` swaps the tooltip for a native `title` (Resolved Decision 8).

### App sidebar contents

Top to bottom: the app name and the collapse toggle; Home (`/projects`); a "Sandboxes" group listing every sandbox from `sandboxesList()` (`src/lib/sandbox/state.svelte.ts`) with a status dot from `statusDotClass()` (`src/lib/sandbox.ts`, shared with `src/routes/projects/SandboxItem.svelte`) and the sandbox in view highlighted; a "New sandbox" item; then a bottom group of AI models (`/models`), Accounts and logins (`/credentials`), Settings (`/settings`), Guide (`/intro`) and the activity center. Page items highlight only while no sandbox is in view and the URL matches. The sidebar calls `ensureSandboxRemoteState()` on mount so the list fills on any route.

### Sandbox header

`src/lib/workspace/SandboxHeader.svelte` is `h-16` (64px, matching `SWITCHER_H`) and one row: a status dot, the sandbox name and its translated status; the app tabs (Manage plus each open pane from `LAUNCHABLE_APPS` in `src/lib/workspace/apps.ts`, each pane tab closable); then Start or Stop, Folder and close. The header root carries `data-sandbox`, tabs keep `data-app` and `data-close-tab`, close keeps `data-close-sandbox`. A sandbox missing from `sandboxesList()` (a chat opened by command, or a sandbox created moments ago) renders without status or Start/Stop. Button labels collapse to icons under a container query on the header.

### Manage dashboard

`src/lib/workspace/WorkspaceDashboard.svelte` loses its own title row (name, Folder, close), now in the header; the `StatusRow`, the section list and every section are unchanged in structure. Destroy finishes with `closeSandbox()`.

### Home page

`src/routes/projects/+page.svelte` becomes Home: a heading and short intro, the Podman setup and start states unchanged, then sandbox cards (`SandboxItem.svelte`, restyled as a card, still `li[data-sandbox]`) in a responsive grid and the create card (`CreateSandboxItem.svelte`). Container queries key to the content area (`data-route-outlet`), never to a 380px column. The card's workspace button opens Manage (Resolved Decision 2).

## Resolved Decisions

The Open Questions of `01_ideas.md`, restated with their resolution:

- **Restore target from the sidebar**: the sandbox's last view, pane or Manage. Rust's `sandbox_last_pane` names the pane; a frontend per-sandbox memory in `src/lib/workspace/state.svelte.ts` records when Manage was the last view, because `hide_active_pane` does not touch `sandbox_last_pane`.
- **Home card primary action**: always Manage, preserving today's projects-list semantics and every flow that opens the dashboard through the card (Resolved Decision 2).
- **Activity center placement**: in the sidebar's bottom group, opening upward from the sidebar (Resolved Decision 3).
- **Header height**: 64px, unchanged, so `SWITCHER_H` keeps its value and name (Resolved Decision 4).
- **Root route**: unchanged; `/` still hosts the welcome flow and the "all set" screen (Resolved Decision 5).
- **Closing a sandbox**: `closeSandbox()` closes the workspace, clears `activeSandbox` and goes to Home; when Rust activates another sandbox's pane as the neighbour, that sandbox comes into view instead (Resolved Decision 6).

Decisions taken on 2026-11-13, consistent with the approved design:

1. **Rust and label scope.** The only Rust change is `NAVIGATION_PANEL_EXPANDED_W` (380.0 to 240.0) with its unit tests; `NAVIGATION_PANEL_RAIL_W` stays 48.0 and `SWITCHER_H` stays 64.0. Plain-language labels change the values of existing message keys; new UI strings get new keys.
2. **Home card opens Manage, the sidebar restores.** The card is the "look after this sandbox" entry point, so it lands on Manage; the sidebar is the "go back to what I was doing" entry point, so it restores.
3. **Activity popover limitation.** The activity panel opens upward, anchored to the sidebar's bottom edge. While a Terminal or Agents webview pane is shown, the part of the panel past the sidebar edge sits under the native pane; the old bell sat entirely under the overlay, so the sidebar placement is strictly better. A proper fix is deferred in `docs/features/unified-sidebar-nav/04_improvements.md`.
4. **One-row header at 64px.** Name and status stack on the left, tabs sit in the middle, actions on the right; the existing height keeps the overlay's top edge unchanged.
5. **Root route untouched.** Redirecting `/` to Home would also move the onboarding debug button and the `onboardingCompleted` gate; out of scope.
6. **Close lands on Home.** The routed page only matters once no sandbox is in view, so `goto("/projects")` after close is harmless when another sandbox takes over.
7. **Constant names kept.** `NAVIGATION_PANEL_EXPANDED_W`, `NAVIGATION_PANEL_RAIL_W` and `SWITCHER_H` keep their names; their doc comments describe the sidebar and the sandbox header.
8. **Rail tooltips.** The `nav-list` item uses the retro `Tooltip` while no webview pane is shown, and a native `title` attribute while one is shown, because WebView2 draws native tooltips as OS popups above sibling webviews while DOM tooltips past the rail fall under the pane.
9. **Unmount the page while a sandbox is in view.** Home cards and the header both carry `data-sandbox`; a hidden-but-mounted page would make e2e selectors resolve to the hidden card.
10. **`activeSandbox` follows `active_app` transitions only.** Setting it on every state event would race a page click: an unrelated event (a collapse echo) arriving between `clearActiveSandbox()` and the `hide_active_pane` echo would put the sandbox back in view.
11. **Dashboard visibility still derives from Rust.** The Manage dashboard shows only when `active_app` is null and a sandbox is in view; a routed page shows only when `active_app` is null and no sandbox is in view. Neither is set before a command resolves.
12. **No new host-global state.** The feature adds no path, host port, external resource, bundled binary, teardown or harness variable; every new store is webview memory. Specs keep their tiers and run in parallel worktrees as today.

## Implemented: Phase A — View state and navigation actions

- `src/lib/workspace/state.svelte.ts`: `viewSandboxName()`, `panesOf()`, `restoreTarget()`, `markManageView()`, `forgetManageView()`, the `active_app` transition rule in `applyState`, `sandbox_last_pane` in the mirrored state type. Removed the unused `isSandboxOpen()` and `hasTabs()`.
- `src/lib/workspace/navigation.svelte.ts`: `showPage`, `openSandbox`, `showManage`, `activatePane`, `closePane`, `closeSandbox`, `newSandbox` (with the `takeCreateFocus`/`createFocusRequested` pair the create card reads).
- `src/lib/sandbox.ts`: `statusDotClass()`.

## Implemented: Phase B — Nav-list primitive, app sidebar, layout

- `src/lib/components/ui/nav-list/` (`index.ts`, `nav-list.svelte`, `nav-list-group.svelte`, `nav-list-item.svelte`, `context.ts`) and `src/lib/components/ui/section-nav/styles.ts`.
- `src/lib/workspace/AppSidebar.svelte`.
- `src/lib/activity/ActivityCenter.svelte`: a `sidebar` placement (panel opens upward, toggle renders as a nav item).
- `src/routes/+layout.svelte`: sidebar plus content area.

## Implemented: Phase C — Sandbox header and view

- `src/lib/workspace/SandboxHeader.svelte`, `src/lib/workspace/SandboxView.svelte`.
- `src/lib/workspace/WorkspaceDashboard.svelte`: title row removed, destroy ends with `closeSandbox()`.
- Removed: `src/lib/components/GlobalNav.svelte`, `src/lib/workspace/NavigationColumn.svelte`, `src/lib/workspace/WorkspaceSwitcher.svelte`, `src/lib/workspace/SandboxSwitcherGroup.svelte`, `src/lib/workspace/WorkspaceContent.svelte`, and the `src/lib/components/ui/tabs/` primitive, whose only user was `GlobalNav.svelte`.

## Implemented: Phase D — Home and full-width pages

- `src/routes/projects/+page.svelte`, `src/routes/projects/SandboxItem.svelte`, `src/routes/projects/CreateSandboxItem.svelte` (focus request).
- `src/routes/models/+page.svelte`, `src/routes/settings/+page.svelte`, `src/routes/credentials/+page.svelte`: centred column, container queries keyed to the content area.

## Implemented: Phase E — New message keys

New keys in `messages/en.json`, `messages/de.json`, `messages/ru.json`: `app_name`, `sidebar_label`, `sidebar_sandboxes`, `sidebar_new_sandbox`, `sidebar_no_sandboxes`, `sidebar_activity`, `sandbox_header_apps_label`, `home_intro`, `workspace_tab_chat`.

## Implemented: Phase F — Documentation

`docs/architecture/workspace-ui.md` (window layout, content area, header), `src/AGENTS.md` (route map, layout line).

## Implemented: Phase G — Rust constant

`NAVIGATION_PANEL_EXPANDED_W` to 240.0 in `src-tauri/src/workspace/mod.rs`, its doc comment, and the unit tests; `cargo check` and `cargo test`.

## Implemented: Phase H — Plain-language label values

Values of `nav_*`, `dashboard_section_*`, `dashboard_summary_network`, `traffic_title` and related keys in all three locales.

## Implemented: Phase I — E2E

- Page objects: `e2e/test/pageobjects/app.page.ts` (sidebar page navigation, `showHome`), new `sidebar.page.ts` and `sandbox-header.page.ts` (replacing `workspace-switcher.page.ts`), and `projects.page.ts`, `project-row.component.ts`, `models.page.ts`, `settings.page.ts`, `workspace-dashboard.page.ts`, `agent-chat.page.ts`.
- New light spec `e2e/test/specs/light/sidebar-navigation-ui.spec.ts`; `e2e/test/specs/standard/embedded-workspace.spec.ts` on the 240/48/64 constants with a Home-over-pane step; smaller edits listed in `docs/features/unified-sidebar-nav/03_e2e.md`.
- `./build e2e light` and `./build e2e` (`standard`) pass, 17 spec files each. Remaining: the `heavy` tier has not been run against the new shell.

## Lifecycle

- **Pane survival.** Leaving a sandbox for a page only hides the active pane; panes, forwards and the chat attach stream keep running until the sandbox is closed or destroyed.
- **Destroy cleanup.** Destroy from the Danger section still forgets section memory, proxy status and agent auth (`docs/features/manage-dashboard-redesign/02_dev.md`, Resolved Decision 16) and then calls `closeSandbox()`, which also drops the per-sandbox Manage memory.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A Terminal or Agents pane renders over the sidebar or leaves a gap | The sidebar's `w-60`/`w-12` in `src/lib/workspace/AppSidebar.svelte` drifted from `NAVIGATION_PANEL_EXPANDED_W`/`NAVIGATION_PANEL_RAIL_W` in `src-tauri/src/workspace/mod.rs` | Change both ends together; each end's comment names the other |
| The pane's top edge cuts into or leaves a strip under the sandbox header | `h-16` in `src/lib/workspace/SandboxHeader.svelte` drifted from `SWITCHER_H` in `src-tauri/src/workspace/mod.rs` | Keep the header exactly 64px with `shrink-0`; change `SWITCHER_H` with it |
| Clicking a page in the sidebar flashes the page and then the sandbox returns | `activeSandbox` was re-set from a `workspace://state` event that did not change `active_app` | Set `activeSandbox` in `applyState` (`src/lib/workspace/state.svelte.ts`) only on an `active_app` transition |
| Clicking a sandbox in the sidebar shows another sandbox's terminal | `switch_app` is a no-op when `sandbox_last_pane` names a closed pane, leaving the previous `active_app` in place | Route through `openSandbox` in `src/lib/workspace/navigation.svelte.ts`, which checks `restoreTarget()` and falls back to `hide_active_pane` |
| A sidebar tooltip is invisible in the rail while a terminal is open | DOM tooltips past the 48px rail fall under the native pane webview | Keep the native `title` fallback in `src/lib/components/ui/nav-list/nav-list-item.svelte` |
