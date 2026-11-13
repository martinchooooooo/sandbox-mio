---
type: ideas
title: "Unified Sidebar Navigation: Ideas"
feature: unified-sidebar-nav
---
# Unified Sidebar Navigation: Ideas

This document scopes replacing the app's four navigation surfaces with one persistent left sidebar, so the whole app reads like the sandbox workspace the user already likes. The app targets non-technical users and should not look like a typical agent harness. Today the shell lives in `src/routes/+layout.svelte`, `src/lib/workspace/` and `src/lib/components/GlobalNav.svelte`; the Rust side that positions native pane webviews is `workspace_layout()` in `src-tauri/src/workspace/mod.rs`. The design below was approved by the user on 2026-11-13.

## Current State

Navigation is split across four surfaces that do not share a visual language:

| Surface | File | Role |
|---|---|---|
| Top line-tabs (Intro, Projects, Models, Settings, Credentials) | `src/lib/components/GlobalNav.svelte` | Route switching, rendered above every routed page |
| Multi-sandbox switcher bar | `src/lib/workspace/WorkspaceSwitcher.svelte`, `src/lib/workspace/SandboxSwitcherGroup.svelte` | One tab group per sandbox with an open pane; shown only once a pane exists |
| Manage section list | `src/lib/workspace/WorkspaceDashboard.svelte` on `src/lib/components/ui/section-nav/` | Left list of Overview, Tools, Ports, Network, Traffic, Remote, Danger zone |
| Navigation column | `src/lib/workspace/NavigationColumn.svelte` | Wraps the routed page; squeezed to 380px (or a 48px rail) whenever a sandbox is open |

Once any sandbox is open, `src/routes/+layout.svelte` docks the whole routed page (the projects list, Models, Settings) into the 380px column and renders the switcher plus the dashboard or a pane to its right. Pages therefore carry container queries keyed to that column (`@max-[420px]` in `src/routes/projects/SandboxItem.svelte`, `src/routes/projects/+page.svelte`, `src/routes/models/+page.svelte`, `src/routes/settings/+page.svelte`, `src/routes/credentials/+page.svelte`).

The pane geometry seam is `workspace_layout()` in `src-tauri/src/workspace/mod.rs`: the overlay rectangle starts at `NAVIGATION_PANEL_EXPANDED_W` (380) or `NAVIGATION_PANEL_RAIL_W` (48) from the left and `SWITCHER_H` (64) from the top. The frontend state seam is `src/lib/workspace/state.svelte.ts`, the single owner of the `workspace://state` subscription, which also holds the client-side "which sandbox's dashboard" value (`activeSandbox`).

## Proposed Model

### One app sidebar

A persistent left sidebar replaces `GlobalNav.svelte` and `NavigationColumn.svelte`. Expanded it is about 240px wide; collapsed it is the existing 48px icon rail with tooltips. From top to bottom: the app name (a way back to Home), a "Sandboxes" group listing every sandbox from `sandboxesList()` (`src/lib/sandbox/state.svelte.ts`) with a status dot and the active one highlighted, a "New sandbox" entry, and a bottom group of AI models, Accounts and logins, Settings, Guide (the `/intro` route) and the activity center. The collapse flag stays Rust-owned (`set_panel_collapsed`), because the overlay rectangle depends on it.

### Content area: a page or a sandbox

To the right of the sidebar the content area shows exactly one of two things: a routed page at full width (Home, AI models, Settings, Accounts and logins, Guide), or one sandbox's workspace. Opening a page while a sandbox pane is active hides the native pane (`hide_active_pane`) and leaves every pane running; selecting the sandbox in the sidebar restores the sandbox's last view. The `/projects` route becomes a friendly Home: sandbox cards, the create flow, and the existing Podman setup and start states.

### One sandbox header

When a sandbox is in view, a header across the top of the content area shows that sandbox only: name, status, Start or Stop, Folder, close, and one row of app tabs (Manage plus each open launchable app from `src/lib/workspace/apps.ts`, closable as today). The header keeps the switcher's 64px height, so the pane overlay's top edge does not move. Switching sandboxes happens in the sidebar. The Manage dashboard keeps its section list unchanged in structure.

### Plain-language labels

Navigation and section labels move to plain phrases in `messages/en.json`, mirrored in `messages/de.json` and `messages/ru.json`: Projects becomes Home, Models becomes AI models, Credentials becomes Accounts and logins, Intro becomes Guide, the Network section becomes Internet access, Danger zone becomes Delete sandbox, and the Tools, Ports, Traffic and Remote labels are softened where a clearer phrase exists. Existing keys keep their names and change only their values.

## Technology Choices with Tradeoffs

### Sidebar primitive

- **Chosen: a small hand-built nav primitive in `src/lib/components/ui/` that reuses the `section-nav` item styling.** The app sidebar and the Manage section list then read as one family: same item height, border, active fill and tooltip behaviour.
- **Rejected: `SectionNav` itself.** `SectionNav` is a bits-ui `Tabs` with content panels and manual activation; the sidebar is navigation across routes and sandboxes with no panel of its own, so `Tabs` semantics (and `aria-controls` pointing at panels) would be wrong.
- **Rejected: the shadcn-svelte `sidebar` registry item.** retro-ui-svelte has no `sidebar` item, the plain shadcn-svelte item brings the default look plus a provider, cookie persistence and mobile sheet the app does not need, and the user asked to hand-build in the existing style rather than pull new registry items.

### Page versus sandbox in the content area

- **Chosen: frontend view state in `src/lib/workspace/state.svelte.ts`.** The sandbox in view is the Rust-reported active pane's sandbox, else the client-side `activeSandbox`; with neither, the routed page shows. Page visibility therefore follows Rust's `active_app` exactly as dashboard visibility already does.
- **Rejected: a `/sandbox/[name]` route.** The static adapter would need a fallback or prerender entries for a dynamic segment, the route would still have to agree with Rust's `active_app` (a pane can become active from Rust with no navigation), and the e2e page objects key heavily on `/projects` URLs.
- **Rejected: keeping the page mounted but hidden behind the sandbox view.** Home cards and the sandbox header would both carry `data-sandbox`, so e2e selectors would resolve to a hidden element; unmounting the page also matches how every other route change behaves.

### Sidebar width source of truth

- **Chosen: keep the Rust constants authoritative** (`NAVIGATION_PANEL_EXPANDED_W`, `NAVIGATION_PANEL_RAIL_W`, and the header height) and mirror them in Tailwind classes with cross-reference comments, as today.
- **Rejected: measuring the sidebar in the DOM and sending the width to Rust.** The overlay would then depend on a frontend round-trip and on DPI-sensitive measurement, breaking the invariant that panes are positioned only from the Rust layout computation.

## Validation

No spike was needed: every mechanism (Rust-owned overlay rectangle, `hide_active_pane`, `switch_app`, `switch_pane`, `sandbox_last_pane`) already exists and is covered by `e2e/test/specs/standard/embedded-workspace.spec.ts`. The change is a reshuffle of frontend chrome plus two constant values.

## Open Questions

- **Restore target from the sidebar**: whether selecting a sandbox in the sidebar restores its last pane, its last view (pane or Manage), or always Manage.
- **Home card primary action**: whether the Home card's Open button restores the last view like the sidebar or always lands on Manage.
- **Activity center placement**: how the activity center sits in the sidebar's bottom group, given that native pane webviews draw above anything the main webview renders over the content area.
- **Header height**: whether the single-sandbox header keeps 64px or shrinks now that it carries one sandbox instead of a tab group per sandbox.
- **Root route**: whether `/` (welcome flow, then an "all set" screen) changes now that Home is a sidebar destination.
- **Closing a sandbox**: where the content area lands after the header's close control closes the sandbox in view.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A Terminal or Agents pane renders shifted or overlaps the sidebar | `NAVIGATION_PANEL_EXPANDED_W` in `src-tauri/src/workspace/mod.rs` and the sidebar's Tailwind width drifted apart | Change both ends together; the comments at each end name the other file |
| A tooltip or popover opened from the sidebar is invisible over an open terminal | Native pane webviews are separate OS surfaces drawn above the main webview, so DOM content past the sidebar edge sits underneath them | Keep sidebar overlays inside the sidebar, or use a native tooltip (`title`) while a webview pane is shown |
| An e2e selector for a sandbox resolves to the wrong element | Home cards, the sidebar list and the sandbox header all render per-sandbox elements | Use distinct hooks per surface (`li[data-sandbox]` on Home, `div[data-sandbox]` on the header, a separate attribute on sidebar entries) |
