---
type: improvements
title: "Unified Sidebar Navigation: Improvements"
feature: unified-sidebar-nav
---
# Unified Sidebar Navigation: Improvements

Work deferred out of the unified sidebar build (`docs/features/unified-sidebar-nav/02_dev.md`) and seam limitations found while building it.

## Activity panel under an active webview pane

The activity panel opens upward from the app sidebar (`src/lib/activity/ActivityCenter.svelte`, `sidebar` placement). While a Terminal or Agents webview pane is shown, the part of the panel past the sidebar edge is drawn under the native pane, because the main webview cannot paint above sibling child webviews. A fix needs Rust help, for example a command that temporarily shrinks the overlay rectangle while the panel is open, or a separate always-on-top popup webview. Deferred because it touches `src-tauri/src/workspace/mod.rs` and the old bell already sat entirely under the overlay.

## Root route as Home

`/` (`src/routes/+page.svelte`) still shows the welcome flow and an "all set" screen after onboarding, so a fresh launch lands on a page no sidebar item highlights. Redirecting `/` to `/projects` once onboarding is complete would make Home the true start page; deferred because the route also hosts the onboarding debug button and the e2e harness's landing assumptions.
