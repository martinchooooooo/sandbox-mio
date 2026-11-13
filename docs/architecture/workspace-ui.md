---
type: architecture
title: "Workspace UI: Window, Panes and Tabs"
description: "The single app window, the Rust-owned registry of embedded terminal and agent panes, the Manage tab and launchable apps, and terminal auto-login."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-13T12:00:00+11:00
stale_after: 2027-02-05T00:00:00+11:00
code:
  - src-tauri/src/workspace/
  - src-tauri/src/commands/panes.rs
  - src-tauri/src/services/panes.rs
  - src/lib/workspace/
  - src/routes/+layout.svelte
sources:
  - resource: docs/features/embedded-windows/02_dev.md
  - resource: docs/features/consolidate-app-container/02_dev.md
  - resource: docs/features/workspace-app-tabs/02_dev.md
  - resource: docs/features/responsive-panel/02_dev.md
  - resource: docs/features/nav-responsive/02_dev.md
  - resource: docs/features/web-terminal/02_dev.md
  - resource: docs/features/manage-dashboard-redesign/02_dev.md
  - resource: docs/features/techdebt-sweep/02_dev.md
  - resource: docs/features/unified-sidebar-nav/02_dev.md
---
# Workspace UI: Window, Panes and Tabs

The app runs in one OS window. One webview renders the whole SvelteKit frontend (navigation, the projects list, settings, and each sandbox's Manage dashboard), and a sandbox's terminal and agent web UI are separate child webviews laid over the content area. Rust owns which panes exist and where they sit; the frontend draws the chrome around them.

## Window Layout

- One app sidebar sits on the left, expanded or collapsed to an icon rail. It lists every sandbox, a way to create one, the app's pages (Home, AI models, Accounts and logins, Settings, Guide) and the activity center.
- The content area to its right shows exactly one thing: a routed page at full width, or one sandbox. A sandbox shows a header (name, status, Start or Stop, Folder, close, and that sandbox's app tabs) above either its Manage dashboard or a pane.
- Pane webviews fill the rectangle beside the sidebar and below the sandbox header. Rust computes that rectangle in logical pixels from the window size and the collapsed state, and repositions every pane on resize or collapse, so layout is DPI-independent and deterministic for tests.
- Opening a page while a pane shows hides the pane and leaves it running; selecting the sandbox in the sidebar restores its last view, pane or Manage. The routed page is unmounted while a sandbox is in view.

## Panes and the Registry

- A Rust registry is the source of truth for the open panes (terminal or agent web UI, per sandbox, each backed by a Mutagen forward), the active app, the collapsed flag, and each sandbox's last active pane. Every change is pushed to the frontend as one state event.
- Opening a pane starts the service inside the sandbox (`zellij web` or the Paseo agent UI), forwards it to a fresh host loopback port ([Transport](transport.md)), and mounts a webview on that port. Opening a pane that already exists switches to it instead.
- Closing a pane removes its webview and ends its forward; closing a sandbox's workspace closes all of its panes.

## Tabs: Manage and Launchable Apps

- Opening a sandbox lands on its Manage tab, which is always present and cannot be closed. Manage is an Overview (app launchers, coding agents and a one-line status summary) plus a sidebar of sections for tools, ports, network policy, traffic, remote deploys and destroying the sandbox.
- Only the shown section is mounted, so a section loads its data when opened. The last section shown is remembered per sandbox, and the summary reads the same shared stores the sections do, so the two never disagree.
- Manage is part of the main webview, not a pane, so showing Manage means hiding the active pane. Which sandbox is in view is frontend state that follows the active pane Rust reports, but whether the dashboard or a page shows is derived from that active app.
- One catalogue declares the apps Manage can launch and when each is available; for example, the agent web UI needs an installed agent.
- The sandbox header's tabs cover only the sandbox in view; switching between sandboxes happens in the sidebar.

## Terminal Login

The terminal pane is `zellij web` inside the sandbox. Before the webview opens, the app mints a login token and injects a script that logs the webview in; the token is held only in that script, never written to disk. The agent web UI's password is kept in the app-data root, outside the synced workspace.

## Invariants

- Panes are positioned only from the Rust layout computation.
- No pane login secret is written into the workspace.
- Dashboard and page visibility follow the active app Rust reports and are never set before a command resolves.
- The sidebar width, rail width and sandbox header height in the frontend equal the Rust layout constants.
- One frontend store owns the pane-state subscription; components read from that store.
- A new launchable app is added to the catalogue, not special-cased in components.
- A Manage section fetches only while it is shown; a new section is added to the section catalogue.
