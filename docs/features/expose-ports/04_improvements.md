---
type: improvements
title: "Expose Ports — Deferred Improvements"
feature: expose-ports
---
# Expose Ports — Deferred Improvements

Port exposure (`expose_port`/`unexpose_port`/`watch_ports_file`, `src-tauri/src/network/mod.rs`) predates the `docs/features/` pipeline, so this directory carries no `01_ideas.md`–`03_e2e.md`. This file exists to hand off work found while fixing `e2e/test/specs/standard/expose-port.spec.ts` on 2026-07-26. Each section states the problem, the evidence for it, and the shape of a fix.

**Resolved as of 2026-08-17.** Both sections below have since been fixed in the codebase (found stale on review; no separate commit tied to this note). The listening-socket-scan race is closed in `expose_port` (`src-tauri/src/network/mod.rs:241-249`): a scan call (`auto_detected: true`) now re-checks `forwards` and returns the existing host port instead of touching it if the port is already tracked, so it can never demote a manual expose; a manual call (`auto_detected: false`) is not gated by that check at all, so it always runs the full flow and overwrites whatever the scanner left behind, upgrading `Auto:<port>` to `Exposed:<port>` with its ✕ regardless of which path won the race. The empty-state gate is fixed in `PortsManager.svelte:28`, which now filters to `fwd.kind.kind === "exposed"` before deriving `forwards`, exactly as the "Shape of a fix" below proposed — Terminal/Agents rows no longer suppress "No ports open."

Not implemented, and still true today: `unexpose_port` has no re-detection suppression, so closing a port whose process is still listening lets the next scan tick re-add it as `Auto:<port>` (`src-tauri/src/network/mod.rs:72`, `e2e/test/specs/standard/expose-port.spec.ts:166-168`). This was posed as an open question below, not a requirement, and remains a deliberate product decision rather than a bug. **Obsolete (2026-11-07):** every relay-path item in this file is moot. The publish+relay reachability model, its relay pool and the in-container `socat` relay were deleted (`docs/features/techdebt-sweep/02_dev.md`, Phase D, Resolved Decision 1), so the manual-upgrade path that orphaned a relay slot no longer exists, and the "reuse the existing relay-pool slot" remark in the first section's shape of a fix describes code that is gone. Exposing a port is now a single Mutagen forward over the sandbox's SSH alias.

## The listening-socket scan can claim a port before a manual expose does

**Where:** `poll_ports_file` (`src-tauri/src/network/mod.rs`, the scan block at the end of each tick) versus `expose_port` called from the "Expose a port" UI (`src/routes/projects/PortsManager.svelte`).

**What happens:** each tick of `poll_ports_file` scans the sandbox's listening TCP sockets and exposes every port not already tracked, passing `label: None, auto_detected: true`. A port the user is about to expose manually is indistinguishable from a dev server they started themselves, so whichever path runs first wins:

- Manual expose wins → the forward is `auto_detected: false`, `kindLabel` renders `Exposed :<port>`, and the row gets a "Stop exposing" ✕ (`isManaged` returns false).
- Scan wins → the forward is `auto_detected: true`, renders `Auto :<port>`, and gets **no** ✕. The subsequent manual `expose_port` call then finds the port already tracked and no-ops, so the user's explicit action silently does nothing to the UI.

The window is up to `PORTS_FILE_POLL_INTERVAL` (2s) wide, and any service already listening when the user clicks is exposed to it.

**Why it matters beyond tests:** a user who starts a dev server and then exposes its port through the UI can end up with a row they cannot close, because the ✕ is withheld from scanner-owned forwards by design (`isManaged`, `PortsManager.svelte`). The only way back is to stop the process inside the sandbox. The explicit user action loses to a background heuristic, which is the wrong precedence.

**Evidence:** `e2e/.diag/exposes-a-port-via-the-UI-and-reaches-th.html` (2026-07-26 07:52 run) captures the losing case — the port list holds `Auto :4000` and no `Exposed :4000`, after the spec had exposed 4000 through the UI. The spec's fake service (`startFakeService`, socat `TCP-LISTEN:…,fork,reuseaddr`) is listening before the expose click, so the race is reliably reachable there.

**Shape of a fix:** make an explicit expose take ownership of a port the scanner already claimed, rather than no-op. In `expose_port`, when the target port is present in `forwards` with `auto_detected: true`, clear that flag (and apply any `label`) instead of returning early — an in-place upgrade, so the existing relay-pool slot and Mutagen forward are reused and the row simply gains its ✕. Requires deciding whether the reverse demotion should ever happen: today `unexpose_port` on a manual forward removes it, and the next scan tick re-adds it as `Auto :`, which is defensible (the port *is* still listening) but means "Stop exposing" does not visibly stop anything. Consider suppressing re-detection of a port the user explicitly unexposed for as long as the sandbox session lives.

**Test impact:** `expose-port.spec.ts` currently depends on winning this race at `closePortRow(\`Exposed :${MANUAL_PORT}\`)`. Once explicit expose reliably wins, that dependency becomes a guarantee rather than a coin flip, and the spec is worth extending with the upgrade case directly: let the scanner claim a port first, then expose it through the UI, and assert the row changes from `Auto :` to `Exposed :` and gains its ✕.

## "No ports open." cannot render once a workspace pane is open

**Where:** `PortsManager.svelte`'s `{#if forwards.length > 0}` empty-state branch.

**What happens:** the `forwards` list mixes user-exposed ports with the workspace's own `ServiceKind::Terminal` and `ServiceKind::AgentWeb` forwards, so the empty state is gated on *all* forwards being absent. Opening a Terminal or Agents pane makes it permanently unreachable for that sandbox, even with no ports exposed.

**Consequence:** the empty state only ever appears in the window between opening a sandbox's dashboard and opening its first pane. `expose-port.spec.ts` asserts it in exactly that window (the opening test) and used to assert it again after closing the last exposed port, where it could not hold; that second assertion was removed on 2026-07-26 rather than papered over with a longer timeout.

**Shape of a fix:** decide what the section is claiming. If "Ports" means user-facing ports, gate the empty state on the exposed-kind subset (`fwd.kind.kind === "exposed"`) and let Terminal/Agents rows render above it independently — then the message means "you have no ports exposed", which is what a reader expects, and it becomes assertable again at the end of a spec. If the section is meant to list every forward including the app's own, the message should say so instead. Either way the `noPortsText` assertion can come back in `expose-port.spec.ts`'s close-button test.

## Common Gotchas

| Symptom | Cause | Fix |
| --- | --- | --- |
| A port row has no "Stop exposing" ✕ despite being exposed from the UI | The scan claimed the port first, so the forward is `auto_detected` and `isManaged` withholds the ✕; the manual `expose_port` no-op'd | Stop the listening process inside the sandbox, or implement the in-place upgrade above |
| A row reappears as `Auto :<port>` immediately after clicking ✕ | The process is still listening, so the next scan tick re-exposes it — `unexpose_port` removes the forward, not the listener | Expected today; see the re-detection-suppression question above |
| `waitForPortRowGone("Exposed :<port>")` passes but the port list is not empty | The label changed rather than the row disappearing — the same port is now tracked as `Auto :<port>` | Assert on the port's absence from the whole list, not on one label form |
| `noPortsText` never appears mid-spec | Terminal/Agents forwards keep `forwards.length > 0` for the rest of the session | Assert it only before the first pane opens, or fix the empty-state gate above |
