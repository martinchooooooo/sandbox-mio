# Paseo Plugins Orientation

`paseo-plugins/` holds the app-owned Paseo plugins that every sandbox's Paseo daemon (the Agents pane) loads. Each `mio-<name>/` subdirectory is one Paseo plugin project. The app packs them into an archive, streams it into the sandbox at `/usr/local/lib/mio/paseo-plugins/`, and registers each plugin in the daemon's `config.json` (`docs/features/paseo-plugins/02_dev.md`). This directory is a pnpm workspace package used only for typechecking: Paseo compiles the TypeScript itself inside the sandbox.

## Rules

- **Ids.** A plugin's directory name and its `paseo-plugin.json` `id` are both `mio-<name>`. The `mio-` prefix is how `paseo-up.sh` (`src-tauri/resources/agent-web/paseo-up.sh`) tells its own `config.json` entries from plugins the user installed. A directory without the prefix or without a manifest is not shipped.
- **Imports.** Import only modules Paseo supplies at runtime: `@getpaseo/plugin`, React, React Native, TanStack Query and Zod (plus `node:` modules from server code). The sandbox copy has no `node_modules`, so any other package fails to resolve there.
- **Version.** `@getpaseo/plugin` here and every manifest's `requirements.paseo` (`>=` followed by the version) must match `PASEO_CLI_TOOL` in `src-tauri/src/runtime/mod.rs`; a `cargo test` in `src-tauri/src/runtime/paseo_plugins.rs` enforces it.
- **Contract.** The `paseo-plugin` skill and https://paseo.sh/docs/plugins/reference.md define the plugin API: entry files, the `client/`, `server/`, `shared/` split, theme colours and the mobile rules (no DOM).

## Verification

- `pnpm --filter paseo-plugins typecheck` (repo root) after any change here.
- `cargo test` (in `src-tauri/`) after adding, renaming or re-versioning a plugin.

## Development Loop

A debug build (`cargo xtask dev`) reads `paseo-plugins/` from its own worktree each time the Agents tab opens; a release build embeds the archive at compile time. To pick up an edit, close the sandbox's Agents tab and open it again: changed contents are applied with `paseo plugin reload` without restarting the daemon, while adding or removing a plugin restarts it. Inside the sandbox, `paseo plugin ls` and `paseo plugin logs <id>` show status and server output.
