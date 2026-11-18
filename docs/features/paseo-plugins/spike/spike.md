---
type: spike
title: "Spike: delivering a directory plugin to the in-sandbox Paseo daemon"
feature: paseo-plugins
---
# Spike: delivering a directory plugin to the in-sandbox Paseo daemon

This record covers the 2026-11-18 spike that proved the delivery model in `docs/features/paseo-plugins/01_ideas.md`. The spike ran against Paseo `npm:@getpaseo/cli@0.16.1` (the version pinned as `PASEO_CLI_TOOL` in `src-tauri/src/runtime/mod.rs`) in a throwaway container from `localhost/sandbox-mio/base:10` on the `podman-machine-default` machine. Artefacts beside this file: `run-spike.sh` (the driver, reproducible), `probe-plugin/` (the `mio-probe` plugin used) and `catalog-probe.mjs` (a Paseo protocol client that lists client bundles).

## Source reading (before the run)

The `@getpaseo/server@0.16.1` and `@getpaseo/protocol@0.16.1` npm tarballs were unpacked and read, not run.

- `config.json` under the Paseo home carries a root `pluginsEnabled` boolean and a `plugins` record keyed by plugin id (`persisted-config.js`). Each value is a directory source with `source: "directory"`, `path` and an optional `enabled` (`plugin-config.js`); no other source kind exists in 0.16.1.
- No `PASEO_*` environment variable sets `pluginsEnabled` or `plugins` (`config.js`).
- At daemon start the plugin manager starts every configured plugin whose `enabled` is not `false`, provided `pluginsEnabled` is `true` (`plugins/index.js`, `start()`).
- The daemon compiles each plugin with esbuild in-process (`plugins/compiler-build.js`). Client bundles mark the plugin SDK, React, React Native, TanStack Query and Zod external. Any other import is bundled from the plugin folder's own `node_modules`.

## Risks and results

| Risk | Result |
|---|---|
| A minimal pre-written `config.json` (only `pluginsEnabled` and `plugins`) is accepted on the daemon's first start | Cleared. `paseo plugin ls` reported `mio-probe` `running` about 3 s after launch. The plugin folder held only TypeScript sources and a manifest, with no `node_modules` |
| A plugin folder can be delivered as a tar stream over exec stdin, the same channel `exec_piped` uses | Cleared, with a gotcha: a tar created on the Windows host carries uid 197609. Plain `tar -xf` failed with `Cannot change ownership to uid 197609, gid 197609: Invalid argument`; `tar --no-same-owner -xf` succeeded |
| `paseo plugin reload <id>` picks up changed files without a daemon restart | Cleared. Logs showed `server stopped: probe-v1` then `server started: probe-v2` within 0.8 s; the daemon pid stayed `122` |
| `paseo plugin install <dir>` can register plugins idempotently | Failed. A second install of the same id errors with `Plugin ID "mio-probe-two" is already configured; choose another ID with --id`. A first install while running works and writes `"enabled": true` into `config.json` |
| A configured plugin whose folder is missing does not block the daemon | Cleared. The daemon started; the missing plugin showed `failed` with `Plugin manifest is missing: <path>/paseo-plugin.json` while `mio-probe` ran |
| The client bundle reaches a browser-type client (the kind the Agents pane's web UI is) | Cleared at the protocol level. `getPluginCatalog()` over `ws://127.0.0.1:6767/ws` returned `mio-probe` with a 1038-byte client bundle containing the post-reload title `Sandbox-mio probe v2`. Rendering inside the Agents pane webview was not observed |

## Other observations

- The daemon rewrites `config.json` itself (pretty-printed, adding `enabled` on install and dropping entries on remove), so an external writer must merge into the file, not own it.
- `paseo plugin remove <id>` deletes the `config.json` entry and keeps the source folder.
- Paseo 0.16.1 ships built-in plugins (`hoplite-provider`, `worktree-include` and others) that appear in the catalog regardless of `pluginsEnabled`.
- Every `paseo plugin install` prints a trust banner (`Trusting plugin code: ...`) to stdout before the result table.
