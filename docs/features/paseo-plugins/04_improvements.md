---
type: improvements
title: "Paseo Plugins: Improvements"
feature: paseo-plugins
---
# Paseo Plugins: Improvements

This document lists work deliberately left out of the first `paseo-plugins` build, which ships only app-owned plugins from `paseo-plugins/` in the repository (`docs/features/paseo-plugins/01_ideas.md`).

## User-written plugins

A user could drop their own Paseo plugins into a sandbox's workspace, for example under `.mio/paseo-plugins/<id>/`. The `config.json` merge in `paseo-up.sh` (`src-tauri/resources/agent-web/paseo-up.sh`) would then register them as directory sources pointing at `/root/work/.mio/paseo-plugins/<id>`. Workspace plugins travel with the project and its remote deploys, unlike app-owned plugins. Deferred because the first build serves only the developer's own Agents-pane extensions. User plugins also raise trust questions: Paseo runs plugin server code unsandboxed inside the sandbox, and a cloned repository could ship a plugin the user never inspected.

## App-wide user plugins

A host-side folder under the app-data root could hold plugins the user wants in every sandbox, delivered through the same archive path as app-owned plugins. Deferred until per-workspace user plugins prove the model.

## Plugin settings in the app

A Manage section could list the `mio-*` plugins and any user plugins with their `paseo plugin ls` status, logs and an enable switch per sandbox. Deferred because Paseo's own Settings → Plugins page already shows status and logs inside the Agents pane.

## Third-party dependencies in plugins

App-owned plugins are limited to modules Paseo supplies at runtime. A plugin that needs another npm package would require either pre-bundling on the host with esbuild during the Rust build, or an `npm install` inside the sandbox with registry egress. Deferred until a plugin needs a package.

## Debug-build plugin reload action

A debug-only action (a Manage button or a file watcher on `paseo-plugins/`) could re-deliver and reload plugins without closing and reopening the Agents tab. Deferred because reopening the tab already runs the full delivery path in `prepare_agent_web()` (`src-tauri/src/runtime/mod.rs`), which is enough for the first plugins (`docs/features/paseo-plugins/02_dev.md`, Resolved Decision 2).

## Deployed-sandbox check

Plugin delivery has not been checked by hand on a deployed sandbox, because the deploy feature was broken when paseo-plugins closed. Delivery should work unchanged, since `prepare_agent_web()` (`src-tauri/src/runtime/mod.rs`) runs the same scripts through the deployed machine's provider. Once deploys work again, open the Agents tab of a deployed sandbox and confirm the probe panel appears. The microsandbox check was skipped too, because microsandbox is due to be dropped.
