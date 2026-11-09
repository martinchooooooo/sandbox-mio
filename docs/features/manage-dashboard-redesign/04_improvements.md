---
type: improvements
title: "Manage Dashboard Redesign: Deferred Improvements"
feature: manage-dashboard-redesign
---
# Manage Dashboard Redesign: Deferred Improvements

Work worth doing that falls outside `docs/features/manage-dashboard-redesign/01_ideas.md`, the redesign of the Manage dashboard (`src/lib/workspace/WorkspaceDashboard.svelte`).

## Host-native mitmdump for microsandbox sidecars

Replace each microsandbox sandbox's sidecar microVM with a Windows-native `mitmdump` process on an authenticated `127.0.0.1` SOCKS5 port, to stop paying one microVM's memory per proxied sandbox. Spike 03 (`docs/features/microsandbox-network-policy/spike/03_chained-proxy.md`, 2026-11-01) proved the chain works, the project addon `src-tauri/resources/mitm_flowlog_addon.py` runs unmodified on Windows, and the sandbox fails closed when the proxy is down. `docs/features/microsandbox-network-policy/01_ideas.md` rejected the option only because of packaging (a shipped Windows binary or a pinned `uv` environment), a host process lifecycle outside the VM tooling, and the build differing from the Podman image. The work would need its own feature that resolves packaging, a host-process lifecycle that survives app restarts or fails closed, and per-sandbox port allocation. Deferred because it changes the proxy architecture, not the dashboard, and the user chose to keep the project's own traffic viewer, which works the same either way. Podman sidecars are excluded: they already run outside the sandbox container, and a host proxy would need the Podman-machine exposure that `docs/features/network-proxy/01_ideas.md` rejected.

## Consistent section headings across the panels

The panels moved into the dashboard sections keep their own small muted labels (Packages, Ports, Remote, Traffic inspection, Network policy, Secrets), written independently over time, so the sections have no shared heading style and a section's list entry is the only common title. A pass that gives every section one heading component would remove the inconsistency. Deferred because the redesign is a re-housing of the panels, not a restyle of each.

## Drop the provider gate inside RemoteManager

`src/routes/projects/RemoteManager.svelte` still checks `capabilitiesFor(provider)?.remoteTargets` itself, although `DASHBOARD_SECTIONS` in `src/lib/workspace/sections.ts` now hides the whole Remote section for a provider that cannot deploy. The inner check is redundant but harmless; removing it leaves one place that owns the gate. Deferred to keep the panel unchanged in this feature.

## Light spec `activity-center-ui` fails on a clean checkout

`e2e/test/specs/light/activity-center-ui.spec.ts` fails all six tests when run alone against the commit before this feature (the app window stays at `about:blank`, so the bell toggle never renders). Observed on 2026-11-08 with the feature's changes stashed, so it is not caused by the dashboard redesign. Deferred because it is a harness or startup problem unrelated to this feature.

