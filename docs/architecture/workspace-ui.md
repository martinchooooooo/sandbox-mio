---
type: architecture
title: "Workspace UI: Window, Panes and Tabs"
description: "The single app window, the Rust-owned registry of embedded terminal and agent panes, the Manage tab and launchable apps, and terminal auto-login."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-05T23:55:00+11:00
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
---
# Workspace UI: Window, Panes and Tabs

The app runs in one OS window. One webview renders the whole SvelteKit frontend (navigation, the projects list, settings, and each sandbox's Manage dashboard), and a sandbox's terminal and agent web UI are separate child webviews laid over the content area. Rust owns which panes exist and where they sit; the frontend draws the chrome around them.

## Window Layout

- The navigation column sits on the left, expanded or collapsed to a rail, and the sandbox switcher runs across the top. Pane webviews fill the remaining content rectangle.
- Rust computes that rectangle in logical pixels from the window size and the collapsed state, and repositions every pane on resize or collapse, so layout is DPI-independent and deterministic for tests.
- Routed pages (Projects, Models, Settings, Credentials, Intro) render inside the navigation column and must adapt between rail and expanded widths.

## Panes and the Registry

- A Rust registry is the source of truth for the open panes (terminal or agent web UI, per sandbox, each backed by a Mutagen forward), the active app, the collapsed flag, and each sandbox's last active pane. Every change is pushed to the frontend as one state event.
- Opening a pane starts the service inside the sandbox (`zellij web` or the Paseo agent UI), forwards it to a fresh host loopback port ([Transport](transport.md)), and mounts a webview on that port. Opening a pane that already exists switches to it instead.
- Closing a pane removes its webview and ends its forward; closing a sandbox's workspace closes all of its panes.

## Tabs: Manage and Launchable Apps

- Opening a sandbox lands on its Manage tab: a dashboard of package, agent, port and remote controls. Manage is always present and cannot be closed.
- Manage is part of the main webview, not a pane, so showing Manage means hiding the active pane. Which sandbox's dashboard shows is frontend state, but whether the dashboard shows is derived from the active app Rust reports.
- One catalogue declares the apps Manage can launch and when each is available; for example, the agent web UI needs an installed agent.
- The sandbox switcher moves between open sandboxes and restores each sandbox's last active pane.

## Terminal Login

The terminal pane is `zellij web` inside the sandbox. Before the webview opens, the app mints a login token, writes the token into the workspace's `.mio/`, and injects a script that logs the webview in. Writing the token into the synced workspace is a known security workaround ([Technical Debt](../techdebt.md)).

## Invariants

- Panes are positioned only from the Rust layout computation.
- Dashboard visibility follows the active app Rust reports and is never set before a command resolves.
- One frontend store owns the pane-state subscription; components read from that store.
- A new launchable app is added to the catalogue, not special-cased in components.
