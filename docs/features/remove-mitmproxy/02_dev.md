---
type: dev-plan
title: "Remove mitmproxy: Build Plan"
feature: remove-mitmproxy
---
# Remove mitmproxy: Build Plan

Every sandbox gets an iron-proxy sidecar and nothing creates or runs a mitmproxy sidecar, so this feature deletes the compatibility path for a pre-boundary mitmproxy sidecar. Status: implemented.

## Resolved Decisions

1. **Keep the seam.** `ProxyEngine` (`src-tauri/src/proxy/types.rs`) and the `engine` field of `ProxyStatus`, `meta.json` and the frontend `ProxyStatus` type stay, with one variant. This is the same one-arm-match extension point `ProviderId` keeps (`docs/features/remove-microsandbox/02_dev.md`).
2. **A missing engine means iron-proxy.** `meta_engine` and `sidecar_engine` (`src-tauri/src/proxy/mod.rs`) default to iron-proxy when `meta.json` has no parseable `engine`.
3. **No support, no cleanup for pre-boundary sidecars.** Same stance as microsandbox leftovers: they are neither migrated nor swept.
4. **Legacy UI is gone.** `ProxyPanel.svelte` loses the legacy notice and its `proxy_unmonitored` message; the `iron` gate stays so a future engine has a place to branch. The e2e legacy-notice test and assertions are deleted.
5. **Fixture left alone.** `"mode":"mitm"` in `src-tauri/src/proxy/fixtures/podman-logs-timestamps.txt` is real iron-proxy log content.

## Touched Files

- `src-tauri/src/proxy/types.rs`, `mod.rs`, `flowlog.rs`: drop the variant, default and wording; `watch_flows` matches on the engine.
- `src/lib/proxy/status.svelte.ts`, `src/routes/projects/ProxyPanel.svelte`, `messages/{en,de,ru}.json`.
- `e2e/test/specs/light/podman-egress-boundary-ui.spec.ts`, `e2e/test/specs/standard/podman-egress-boundary.spec.ts`, `e2e/test/pageobjects/workspace-dashboard.page.ts`.
- `docs/architecture/network.md`: the pre-boundary sidecar sentence is removed.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| An agent deletes `ProxyEngine` or the `engine` field as redundant | One variant looks pointless | Keep them: they are the extension seam, like the one-arm `ProviderId` matches |
| `git grep mitm` hits the flow-log fixture | iron-proxy audit lines carry `"mode":"mitm"` | Leave the fixture; it is real iron-proxy output |
