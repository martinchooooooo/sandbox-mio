---
type: dev-plan
title: "Workspace UX: Sidebar as the Only App Chrome"
feature: workspace-ux
---
# Workspace UX: Sidebar as the Only App Chrome

Build record for `workspace-ux`, written after implementation (no `01_ideas.md` exists; the goals came from user reports). The feature removes the 64px sandbox header, nests the in-view sandbox's apps in the global sidebar, replaces the Manage dashboard's vertical section nav with horizontal tabs, adds a dashboard title bar, and makes Open folder open the sandbox folder itself. Status: Phases 1–5 done and approved.

## Problems Solved

1. The Manage dashboard's vertical section nav collapsed to icons only whenever the global sidebar was expanded (container narrower than 48rem).
2. With the global sidebar collapsed, some text looked cramped: the header's sandbox name used the `font-head` face (Archivo Black) with `truncate` and tight leading, clipping glyphs.
3. A hosted app (Terminal, the Paseo Agents web UI, Chat) had app chrome on the left (sidebar) and on top (sandbox header); users want the hosted app to feel native.
4. Start and Stop were too prominent next to Folder, and Folder opened the parent directory with the sandbox selected instead of the sandbox folder.

## The Concrete Model

- No sandbox header. Pane webviews fill the full height right of the sidebar: `workspace_layout()` in `src-tauri/src/workspace/mod.rs` returns y = 0 and the full window height. The header-height constant is gone.
- `src/lib/workspace/AppSidebar.svelte` renders the sandbox in view as a tree: Dashboard, the apps from `src/lib/workspace/apps.ts` that have an open pane, and Open folder. In the icon rail the same rows appear as grouped icons under the sandbox's letter badge (the "hosted app" mode). Other sandboxes stay single rows.
- Apps are launched from large cards at the top of the Overview (`src/lib/workspace/DashboardOverview.svelte`): icon, title, plain-language description, Open or Switch to, and Close when open. Unavailable apps render as disabled cards with a hint; descriptions and hints live in the catalogue. `launchApp()` in `src/lib/workspace/navigation.svelte.ts` is the one launch-or-switch path.
- The dashboard (`src/lib/workspace/WorkspaceDashboard.svelte`) opens with `src/lib/workspace/SandboxTitleBar.svelte`, then the section tab strip. `src/lib/workspace/SandboxView.svelte` shows dashboard or chat and carries no header.
- Sections render in `src/lib/components/ui/section-nav/` (converted in place from vertical to a horizontal tab strip; labels always visible, icons dropped below a 52rem container width so the strip fits one row at the default window size, wrapping only as a last resort). Shared item classes live in `section-nav/styles.ts`; `src/lib/components/ui/nav-list/nav-list-item.svelte` gained `muted`/`contains` states and a sibling action button for the sidebar rows.
- `open_sandbox_folder(name)` in `src-tauri/src/commands/sandbox.rs` (registered in `src-tauri/src/commands/mod.rs`) resolves the workspace path from the backend's own sandbox list, checks it is a directory and opens it with the opener plugin.

## Resolved Decisions

1. **No sandbox header.** The header is removed entirely and panes fill the full height. Alternatives considered: a slimmer header and an auto-hiding header; both were rejected because any top strip still reads as the app wrapping the hosted app.
2. **Sidebar tree for the sandbox in view.** Dashboard, the apps the user has opened, and Open folder nest under the sandbox row. Apps appear in the sidebar only once opened (revised after user review): the user meets the Dashboard first and launches apps from its cards (Resolved Decision 11). Only the sandbox in view expands, to keep the list calm. Why: the sidebar is the only chrome left, so it must carry app switching.
3. **No popups from the sidebar.** A Paseo child webview covers DOM content right of the sidebar, so a popover or menu anchored in the sidebar would render beneath it. Per-app close is an inline hover x (expanded mode only); sandbox actions that need a menu live in the dashboard.
4. **Manage tab renamed "Dashboard" in the UI.** The message key is unchanged. It gets an icon distinct from the global Home icon.
5. **Dashboard title bar.** Status dot plus name (normal leading, no clipping truncation), status row, health chip and a visible Open folder button. A small Stop (outline) or Start (filled) button sits beside the status text: quiet while running, prominent when stopped because nothing works until it starts. Restart, Close and Delete sandbox sit in a "more" dropdown. Revised after user review: hiding Stop in the menu made it undiscoverable.
6. **Horizontal, always-labelled section tabs. Supersedes `docs/features/manage-dashboard-redesign/02_dev.md` Resolved Decision 10** (the `@min-[48rem]` icon-collapse breakpoint and the `10rem` list). `section-nav` had one consumer, so it was converted in place. Below a 52rem container the tab icons are dropped so all labels fit one row; wrapping is only a fallback, never a scrollbar.
7. **`open_sandbox_folder(name)` in Rust.** The path comes from the backend's sandbox list, never from the frontend. It must be a directory. There is no workspace-root containment check, because sandboxes created on an existing path live outside the root by design. Under the mock backend (`mock_backend_enabled()` in `src-tauri/src/backends/`) it validates but does not launch Explorer.
8. **Start and Stop are reachable only from the Dashboard**, not while a pane shows. Accepted trade-off; the sidebar status dot still shows state.
9. **Sidebar is not auto-collapsed on entering a hosted app.** Users like the global sidebar; collapsing stays manual. Possible follow-up.
10. **Delete sandbox moves from a Danger zone tab into the "more" menu, behind a confirmation dialog** (`src/lib/workspace/SandboxTitleBar.svelte`). The long tab overflowed the strip, and a menu item needs a confirm step a dedicated tab did not. The dialog states that the workspace folder files stay on the computer.
11. **Large app cards on the Overview.** A non-technical user sees what each app is for before opening it, and unavailable apps explain what to install instead of being hidden.

## Implemented: Phase 1 (Rust layout and folder command)

- `workspace_layout()` and its unit tests in `src-tauri/src/workspace/mod.rs`: y = 0, full height, x equal to `NAVIGATION_PANEL_EXPANDED_W` or `NAVIGATION_PANEL_RAIL_W`.
- `open_sandbox_folder` in `src-tauri/src/commands/sandbox.rs`, registered in `src-tauri/src/commands/mod.rs`.

## Implemented: Phase 2 (frontend and i18n)

- `src/lib/workspace/AppSidebar.svelte`: sandbox tree, grouped rail icons, hover close, Open folder row.
- `src/lib/workspace/SandboxTitleBar.svelte` (new) replaces the deleted `src/lib/workspace/SandboxHeader.svelte`.
- `src/lib/workspace/navigation.svelte.ts` (`launchApp`), `src/lib/workspace/state.svelte.ts`, `src/lib/sandbox/state.svelte.ts`, `src/lib/workspace/apps.ts`, `src/routes/+layout.svelte`.
- `src/lib/components/ui/section-nav/*` horizontal conversion and `src/lib/components/ui/nav-list/nav-list-item.svelte`.
- Messages in `messages/en.json`, `messages/de.json`, `messages/ru.json`.

## Implemented: Phase 3 (e2e)

`e2e/test/pageobjects/sandbox-view.page.ts` replaces the header page object; `e2e/test/specs/light/workspace-sidebar-apps-ui.spec.ts` covers the sidebar tree, card launch and close, the tab strip on one row, Start/Stop and the delete dialog. Selectors for Open folder and Close sandbox must be scoped (see Common Gotchas).

## Implemented: Phase 4 (docs)

`docs/architecture/workspace-ui.md`, `docs/features/log.md`, `src/AGENTS.md` and `AGENTS.md` (component list) updated.

## Implemented: Phase 5 (review revisions)

App cards, apps in the sidebar only once opened, the visible Stop/Start beside the status, Delete in the menu with a dialog, and the icon-dropping tab strip (Resolved Decisions 2, 5, 6, 10, 11).

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A popover, dropdown or tooltip opened from the sidebar is invisible while Terminal or Agents is active | A Paseo or Herdr child webview is a native overlay that covers all DOM content right of the sidebar | Keep sidebar interactions inline (hover x, buttons); put menus in the dashboard title bar (`src/lib/workspace/SandboxTitleBar.svelte`) |
| Sandbox name glyphs are clipped | `font-head` (Archivo Black) combined with `truncate` and tight leading cuts ascenders and descenders | Never combine `truncate` with tight leading on `font-head`; the title bar's `truncate` is safe only at `text-xl`'s normal leading |
| Native panes render shifted against the sidebar | Tailwind widths `w-60` and `w-12` in `src/lib/workspace/AppSidebar.svelte` differ from `NAVIGATION_PANEL_EXPANDED_W` and `NAVIGATION_PANEL_RAIL_W` in `src-tauri/src/workspace/mod.rs` | Change both sides together |
| An e2e selector for `[data-open-folder]` or `[data-close-sandbox]` matches two elements | Each attribute exists in both `AppSidebar.svelte` and `SandboxTitleBar.svelte` | Scope the selector to the sidebar or to the dashboard title bar container |
| Open folder is rejected for a sandbox created on an existing path | A workspace-root containment check was added although such sandboxes live outside the root by design | Keep `open_sandbox_folder` free of root containment; rely on the directory check |
| Open folder launches Explorer during mock-backend e2e runs | The `mock_backend_enabled()` early return in `src-tauri/src/commands/sandbox.rs` was removed | Restore the early return so the command validates but does not launch |
| Start and Stop are not visible while Terminal or Agents shows | Lifecycle actions live only in the dashboard title bar (Resolved Decision 8) | Open Dashboard from the sidebar; the status dot shows state meanwhile |
| An app is missing from the sidebar | The sidebar lists only apps with an open pane (Resolved Decision 2) | Launch it from its Overview card |
