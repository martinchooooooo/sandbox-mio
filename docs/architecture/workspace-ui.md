---
type: architecture
title: "Workspace UI: Window, Panes and Tabs"
description: "The single app window, the Rust-owned registry of embedded terminal, agent web UI and native chat panes, the Manage tab and launchable apps, and terminal auto-login."
generated:
  by: claude-code/sonnet-5-5
  at: 2026-11-22T12:00:00+11:00
stale_after: 2027-02-17T00:00:00+11:00
code:
  - src-tauri/src/workspace/
  - src-tauri/src/commands/panes.rs
  - src-tauri/src/services/panes.rs
  - src/lib/workspace/
  - src/lib/agent-chat/
  - src/lib/story-stage/
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
  - resource: docs/features/native-agent-ui/02_dev.md
  - resource: docs/features/chat-agent-anatomy/02_dev.md
  - resource: docs/features/workspace-ux/02_dev.md
---
# Workspace UI: Window, Panes and Tabs

The app runs in one OS window. One webview renders the whole SvelteKit frontend (navigation, the projects list, settings, and each sandbox's Manage dashboard), and a sandbox's terminal and agent web UI are separate child webviews laid over the content area. Rust owns which panes exist and where they sit; the frontend draws the chrome around them.

## Window Layout

- One app sidebar sits on the left, expanded or collapsed to an icon rail. It lists every sandbox, a way to create one, the app's pages (Home, AI models, Accounts and logins, Settings, Guide) and the activity center.
- The content area to its right shows exactly one thing: a routed page at full width, or one sandbox. The sandbox in view nests its apps (Dashboard, the apps opened so far, Open folder) under its sidebar row (grouped icons in the icon rail), which is the only app chrome; the content area is either its dashboard (the Manage tab, labelled "Dashboard"; title bar with status and actions, then section tabs) or a pane.
- Pane webviews fill the rectangle beside the sidebar at full height. Rust computes that rectangle in logical pixels from the window size and the collapsed state, and repositions every pane on resize or collapse.
- Opening a page while a pane shows hides the pane and leaves it running; selecting the sandbox in the sidebar restores its last view. 

## Panes and the Registry

- A Rust registry is the source of truth for the open panes (terminal, agent web UI or native chat, per sandbox), the active app, the collapsed flag, and each sandbox's last active pane. Every change is pushed to the frontend as one state event.
- Opening a web pane starts the service inside the sandbox (Herdr, through the native terminal, or the Paseo agent UI), forwards it to a fresh host loopback port ([Transport](transport.md)), and mounts a webview on that port. Opening a pane that already exists switches to it instead.
- The native chat pane has no webview, forward or URL: the main webview renders it like Manage, and activating it hides any overlay webview. Its connection is in [Agents](agents.md). Learning mode labels parts with Brain or Arms avatars.
- Closing a pane removes its webview and ends its forward, or ends the chat's attach; closing a sandbox's workspace closes all of its panes.

## Tabs: Manage and Launchable Apps

- Opening a sandbox lands on its Manage tab, which is always present and cannot be closed. Manage is an Overview (app cards, coding agents, a status summary) plus horizontal section tabs (tools, ports, network, traffic, remote); deleting the sandbox is a confirmed menu action.
- Only the shown section is mounted, so it loads its data when opened. The last section shown is remembered per sandbox.
- Manage is part of the main webview, not a pane, so showing Manage means hiding the active pane. Whether the dashboard, a page or the chat shows is derived from the active app Rust reports.
- One catalogue declares the apps Manage can launch and when each is available; for example, the agent web UI needs an installed agent.
- The sidebar's nested apps cover only the sandbox in view; its rows switch sandboxes.

## Terminal Login

The terminal pane is Herdr inside the sandbox. Before the webview opens, the app mints a login token and injects a script that logs the webview in; the token is held only in that script, never written to disk. The agent web UI's password is kept in the app-data root, outside the synced workspace.

## Invariants

- Only panes backed by a forward have a webview or URL, and only they are positioned, from the Rust layout computation.
- No pane login secret is written into the workspace.
- Dashboard and page visibility follow the active app Rust reports and are never set before a command resolves.
- The sidebar width and rail width in the frontend equal the Rust layout constants.
- One frontend store owns the pane-state subscription; components read from that store.
- No menu is anchored in the sidebar, because a pane webview covers content beside it; menus live in the dashboard.
- A new launchable app is added to the catalogue, not special-cased in components.
- A Manage section fetches only while it is shown; a new section is added to the section catalogue.
