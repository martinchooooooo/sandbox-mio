---
type: dev-plan
title: "Proxy Traffic Viewer: Build Plan"
feature: proxy-traffic-viewer
---
# Proxy Traffic Viewer: Build Plan

This document is the build plan for the per-sandbox flow-log panel scoped in
`docs/features/proxy-traffic-viewer/01_ideas.md`, and supersedes it wherever the two disagree.
All four phases are `Implemented`; the Paraglide i18n pass is the one deliberate remainder, deferred
until the panel's wording is approved (`TrafficPanel.svelte` carries `TODO(i18n)`-marked English
literals in the interim). Phase A ran 2026-08-18
(`docs/features/proxy-traffic-viewer/spike/`) and cleared every risk it targeted — see Phase A below
for results, and `01_ideas.md`'s Validation section for the full spike record. One finding narrows an
assumption Phase B's design text below still states as an "accepted gap" (flow.kill()-based blocks
are not actually invisible to this feature's addon, as flagged inline in Phase B) — flagged there for
a follow-up pass, not silently corrected.

## The Concrete Model

A proxy-enabled sandbox gets one more mounted file and one more mitmproxy addon script, both
additions to what `docs/features/network-proxy/02_dev.md` already builds in `create_proxy`
(`src-tauri/src/proxy/mod.rs`): `src-tauri/resources/mitm_flowlog_addon.py`, embedded and written to
`<state>/flowlog_addon.py` at every `create_proxy`, mounted read-write at `/addons/flows.ndjson`'s
directory and read-only at `/addons/flowlog_addon.py`, and added as a third `-s` argument to
`mitmdump`'s command vec (order relative to `addon.py`/a future `policy_addon.py` does not matter —
`01_ideas.md`, Proposed Model). The addon hooks `response()` and `error()`, appends one JSON line per
finished flow to `/addons/flows.ndjson` (pre-created empty by `create_proxy`, the same pattern
`rules_path` already follows), and caps the file at `FLOW_LOG_MAX_LINES` (500, Resolved Decision
below) by rewriting it to keep only the most recent lines whenever a new line would exceed the cap.

A new module, `src-tauri/src/proxy/flowlog.rs`, owns the app side: `FlowEntry` (the Rust mirror of the
addon's JSON schema), a small `FlowWatchRegistry` (Tauri-managed state, `Mutex<HashMap<String,
tokio::task::JoinHandle<()>>>`) that makes starting a watch idempotent per sandbox, and
`watch_flows(app, registry, sandbox)` — a detached poll task reading `<state>/flows.ndjson` on a fixed
interval, tracking a byte offset so each tick parses only newly appended lines, and emitting
`proxy://flows-changed` with `{ sandbox, flows: FlowEntry[] }` (new lines only, not a full resnapshot —
Resolved Decision, below) whenever the offset advances. The task self-terminates the first time the
read fails with "file not found" (the proxy container, and therefore its state directory, was
removed), so no explicit cancel-on-teardown wiring is needed — only cancel-on-duplicate-start, which
`FlowWatchRegistry` already covers.

A new command, `watch_proxy_flows(name, app)` (`src-tauri/src/commands/proxy.rs`), calls
`flowlog::watch_flows`, and is invoked once from `src/routes/projects/TrafficPanel.svelte`'s mount
`$effect`, mirroring `watch_workspace_ports`'s call site in `open_terminal_pane`-adjacent code
(`src-tauri/src/commands/panes.rs`) and `watch_workspace_ports`'s own frontend call. A new
module-scoped store, `src/lib/proxy/traffic.svelte.ts` (mirroring `src/lib/network/state.svelte.ts`),
subscribes to `proxy://flows-changed` once, holds a per-sandbox capped ring buffer (`FLOW_LOG_MAX_LINES`
mirrored client-side), and exposes a `flowsFor(sandbox)` reader `TrafficPanel.svelte` filters through,
the same shape `openForwardsFor` already gives `PortsManager.svelte`.

## Resolved Decisions

1. **Exact flow-log cap** (`01_ideas.md` Open Question) — `FLOW_LOG_MAX_LINES = 500`, a constant in
   both `src-tauri/resources/mitm_flowlog_addon.py` and `src/lib/proxy/traffic.svelte.ts` (the two
   must stay equal, commented on both ends per `AGENTS.md`'s cross-file-constant convention). 500 at a
   ~2-second poll interval (Resolved Decision 3) covers a burst-heavy debugging session without makng
   either the addon's per-append rewrite or the frontend's re-render cost noticeable.
2. **`response()`-hook behavior on a synthetic (`flow.response`-set) response** (`01_ideas.md`
   Validation, first unconfirmed item) — settled by Phase A's gating spike (2026-08-18, below):
   `response()` fires and sees a `flow.response` set by an earlier addon; `flow.kill()` instead fires
   `error()` with a generic `"Connection killed."` message. Phase B's schema (below) is written against
   this confirmed mechanism; whether a future `proxy-network-policy` block actually uses `flow.response`
   or `flow.kill()` is still that feature's own decision, not resolved by this document.
3. **Event shape: one app-global event vs. one per sandbox** (`01_ideas.md` Open Question) — resolved
   as one app-global event, `proxy://flows-changed`, carrying a `sandbox` field and only the flows new
   since the last tick (not a full snapshot), matching `network://forwards-changed`'s single-event-name
   precedent but diverging from its full-snapshot-every-tick payload shape — a full resend of up to 500
   lines on every 2-second tick for every open proxy-enabled sandbox was judged wasteful compared to
   ports' typically-single-digit forward count, so this feature sends only the delta and lets the
   frontend store own accumulation and capping.
4. **Whether flow capture is a separate opt-in or automatic for every proxy-enabled sandbox**
   (`01_ideas.md` Open Question) — automatic, no separate toggle. `mitm_flowlog_addon.py` is mounted
   and loaded unconditionally in `create_proxy` alongside `addon.py`, the same way `addon.py` itself is
   unconditional once a sandbox has any proxy sidecar at all — the base proxy toggle
   (`docs/features/network-proxy/02_dev.md`, Resolved Decision 3) remains the only opt-in gate. A user
   who wants a proxy-enabled sandbox with no flow log has no way to get one in v1; that narrower control
   is deferred (`04_improvements.md`).
5. **Retention across a proxy container restart** (`01_ideas.md` Open Question) — confirmed by this
   plan's own design, not left implicit: `create_proxy` only pre-creates `flows.ndjson` if absent (it
   must not truncate an existing file the way it resets `rules.json` to an empty array every call,
   since `create_proxy` itself is only called once per sandbox lifetime — `restart_proxy_container`
   never calls `create_proxy` again, so this is a non-issue for a rules-change-triggered restart, but
   `create_proxy`'s own `flows.ndjson` initialization (Phase B) is written as create-if-absent
   defensively, so a future code path that does re-run `create_proxy` against an existing state
   directory does not silently wipe history).
6. **Poll interval** (new; not an open question in `01_ideas.md`, settled here) —
   `FLOW_LOG_POLL_INTERVAL = Duration::from_secs(2)` in `src-tauri/src/proxy/flowlog.rs`, matching
   `PORTS_FILE_POLL_INTERVAL` (`src-tauri/src/network/mod.rs`) exactly, for the same reasoning: cheap
   enough to poll this often (a bind-mounted file read, not a `podman exec`), frequent enough that a
   user watching the panel doesn't perceive lag.
7. **`secret_injected` computation** (new; narrows `01_ideas.md`'s Proposed Model) —
   `mitm_flowlog_addon.py` loads `/addons/rules.json` once at startup (the same file, same load-once
   contract `SecretInjector` in `mitm_addon.py` already has) purely to test host/path-prefix matches in
   its own `response()` hook; it does not import or call into `SecretInjector`, keeping the two addon
   classes independent per `01_ideas.md`'s Technology Choices ("no coupling to files
   `proxy-network-policy` or `mitm_addon.py` changes concurrently"). A rules-file change during a proxy
   container's lifetime already forces a restart (`docs/features/network-proxy/02_dev.md`, Resolved
   Decision 11), which reloads both addons' copies together, so the two never drift out of sync between
   restarts.

## Implemented: Phase A — gating spike, hook behavior and multi-script composition

Ran 2026-08-18. Throwaway `mio-spike-*` containers/network under
`docs/features/proxy-traffic-viewer/spike/`, all removed after (verified via `podman ps -a --filter
name=mio-spike` and `podman network ls --filter name=mio-spike`, both empty), against
`docker.io/mitmproxy/mitmproxy:11.1.3` — confirmed as the exact tag `MITMPROXY_IMAGE_DEFAULT` resolves
to (`src-tauri/src/proxy/mod.rs`). Every risk below cleared; no blocker. Full evidence recorded in
`01_ideas.md`'s Validation section; scripts kept at
`docs/features/proxy-traffic-viewer/spike/{a_synth,b_synth,a_kill,b_kill,addon,policy_addon_stub,flowlog_addon}.py`.

- **Confirmed**: `mitmdump -s a.py -s b.py`, where `a.py`'s `request()` hook sets `flow.response`
  directly (not `flow.kill()`), causes `b.py`'s `response()` hook to fire and see that synthetic
  response (`b_synth.py` observed `status=403` and the distinctive `X-Synth-Blocked: true` header
  `a_synth.py` set) — the mechanism a future `proxy-network-policy` block would need for this feature's
  flow log to capture a blocked request as a captured (not missing) entry.
- **Confirmed, more specific than the question posed**: `flow.kill()` in `a.py`'s `request()` hook
  does **not** trigger `b.py`'s `response()` hook. It **does** trigger `b.py`'s `error()` hook — not
  "or neither" — with `flow.error` stringifying to exactly `"Connection killed."` (`b_kill.py`'s
  output, verbatim). **This narrows a downstream assumption**: see the flag immediately below and in
  Phase B.
- `flow.request.timestamp_start` and `flow.response.timestamp_end` are populated as expected on an
  ordinary successful flow, and their difference is a sane millisecond-scale duration — measured
  6-17ms across runs against a same-bridge-network plain-HTTP target container.
- An `error()` hook fires for a connection to a deliberately unreachable host (an unused IP,
  `10.89.0.250`, on the spike's own `10.89.0.0/24` bridge subnet), with no `response()` call for the
  same flow — confirmed by exactly one `flows.ndjson` line (`error`, `status: null`) for that flow.
- Three `-s` scripts (`addon.py`, `policy_addon_stub.py` as a throwaway stand-in for a future
  `policy_addon.py`, and `flowlog_addon.py`) all loaded without error against
  `mitmproxy/mitmproxy:11.1.3` and each script's own hooks fired on a single test request (`addon.py`'s
  `request()`, `policy_addon_stub.py`'s `request()`, `flowlog_addon.py`'s `response()` all logged, plus
  a correctly written `flows.ndjson` line) — a narrower claim than full hook-ordering composition
  (which `docs/features/proxy-network-policy/01_ideas.md`'s own gating spike owns), but confirms this
  feature's addon is not itself the thing that breaks three-script loading.

**Flag for a follow-up pass — does not silently rewrite Phase B**: Phase A's original text (before this
spike ran) assumed that if `flow.kill()` never reaches `response()` or `error()`, a blocked-and-invisible
flow would be an accepted, undesigned-for gap. The spike found the opposite of that premise: `flow.kill()`
**does** reach `error()`, every time, with a stable, generic message (`"Connection killed."`, no status
code). This means a future `proxy-network-policy` block resolved via `flow.kill()` is *not* invisible to
this feature's flow log — it will appear as an `error()`-hook entry — but Phase B's current schema
(below) does not special-case this message, so a `flow.kill()`-based block would render in the panel
identically to any other `error()`-hook flow (connection refused, TLS failure, timeout, or a genuinely
unrelated `flow.kill()` call from some other addon), with no way for a user to tell "this was a policy
block" apart from "this was a network failure." Whether that ambiguity matters is a `proxy-network-policy`
design question (which hook mechanism it picks, and whether it wants blocked requests visually
distinguishable from failures) — this document does not resolve it, only surfaces that the "accepted
gap" framing in `01_ideas.md`'s Open Questions and this file's original Phase A text was inaccurate: the
gap is not invisibility, it is under-specificity of an `error()` entry that could mean several different
things.

## Implemented: Phase B — addon script and materialised state

- `src-tauri/resources/mitm_flowlog_addon.py` (new): `FlowLogger` addon class. `__init__` loads
  `/addons/rules.json` once (Resolved Decision 7) and counts the existing lines of
  `/addons/flows.ndjson` (each append re-opens the file rather than holding a handle, so the
  cap-triggered rewrite cannot race its own writer).
  `response(self, flow)` builds one JSON object (`id` — a per-process monotonic counter is sufficient,
  no UUID needed since the log is process-lifetime-scoped and reset on every `create_proxy`; `method`;
  `host` from `pretty_host`; `path`; `status` from `status_code`; `ts` from `timestamp_start`;
  `duration_ms` from `(timestamp_end - timestamp_start) * 1000`; `req_bytes`/`resp_bytes` from each
  message's `Content-Length` header, falling back to `len(content)` when absent; `secret_injected`,
  Resolved Decision 7) and appends it as one line, then truncates to `FLOW_LOG_MAX_LINES` (Resolved
  Decision 1) if the append pushed the file over the cap. `error(self, flow)` appends a narrower object
  (`id`, `method`, `host`, `path`, `status: null`, `ts`, `error` — `str(flow.error)` — no duration/byte
  fields, since no response ever arrived). JSON keys are camelCase (`durationMs`, `reqBytes`,
  `respBytes`, `secretInjected`), since `FlowEntry` (Phase C) deserialises these same lines under one
  `serde(rename_all = "camelCase")` that governs both directions.
- `src-tauri/src/proxy/mod.rs`: embed `mitm_flowlog_addon.py` via `include_str!` (mirrors
  `ADDON_SCRIPT`), write it to `<state>/flowlog_addon.py` at every `create_proxy`. `create_proxy`
  pre-creates `<state>/flows.ndjson` only if absent (Resolved Decision 5) rather than unconditionally
  truncating it, unlike `rules_path`'s unconditional empty-array write. `create_proxy`'s
  `SidecarSpec.mounts` gains a `SidecarMount { host_path: host_path(&flowlog_addon_path), dest:
  "/addons/flowlog_addon.py", read_only: true }` and a `SidecarMount` for `flows.ndjson` itself at
  `/addons/flows.ndjson`, `read_only: false` (the addon appends to it). `command` gains `-s
  /addons/flowlog_addon.py` (position among the `-s` flags is a no-op per the Concrete Model, appended
  last for minimal diff against the existing two-flag command vec).
- `src-tauri/src/proxy/types.rs`: no new `ProxyError` variant expected — `flowlog.rs` reuses
  `ProxyError::Io`/`CommandFailed` the way `rules.rs` does.

## Implemented: Phase C — poll loop, event, and command

- `src-tauri/src/proxy/flowlog.rs` (new): `FlowEntry { id: u64, method: String, host: String, path:
  String, status: Option<u16>, ts: f64, duration_ms: Option<f64>, req_bytes: Option<u64>, resp_bytes:
  Option<u64>, secret_injected: bool, error: Option<String> }`, deriving `serde::Deserialize` (reading
  the addon's JSON lines) and `serde::Serialize` (the event payload) with `#[serde(rename_all =
  "camelCase")]` matching `ProxyStatus`'s existing convention.
- `src-tauri/src/proxy/flowlog.rs`: `FLOW_LOG_MAX_LINES: usize = 500` (Resolved Decision 1,
  commented as mirroring the same-named constant in `src/lib/proxy/traffic.svelte.ts`) and
  `FLOW_LOG_POLL_INTERVAL: Duration` (Resolved Decision 6).
- `src-tauri/src/proxy/flowlog.rs`: `FlowWatchRegistry(Mutex<HashMap<String,
  tauri::async_runtime::JoinHandle<()>>>)` — `tauri::async_runtime::spawn`, not `tokio::spawn` as
  this plan originally said, and therefore that handle type (`is_finished` reached through
  `.inner()`). `tokio::spawn` panics when the caller has no Tokio runtime entered, which aborts the
  process rather than unwinding when the caller is the IPC thread; see the Common Gotchas row below.
  `Default`-derived, registered via `app.manage(...)` in
  `src-tauri/src/bootstrap.rs` alongside the existing `TaskRegistry`/`WorkspaceRegistry` calls — managed
  bare, not behind an `Arc`, matching those two (the spawned task needs no handle on the registry).
  `start_watch(app, registry, sandbox) -> bool` returns `false` (no-op) if a live task is already registered
  for `sandbox` (checked with `JoinHandle::is_finished`), otherwise spawns and registers a new one —
  the same idempotency `watch_ports_file` gives implicitly through `MutagenNetworkManager.ports_watch`.
- `src-tauri/src/proxy/flowlog.rs`: `async fn watch_flows(app: AppHandle, sandbox: String)` — the
  spawned task body. Each tick: read `<state>/flows.ndjson` from the tracked byte offset onward (a
  plain `std::fs::File` seek-and-read, no `podman exec`, mirroring `read_ca_pem`'s direct-host-read
  precedent); on `NotFound`, break the loop (proxy was torn down); on any new lines, parse each as a
  `FlowEntry` and `app.emit("proxy://flows-changed", &json!({ "sandbox": sandbox, "flows": new_entries
  }))`; sleep `FLOW_LOG_POLL_INTERVAL`. Two details the plan left implicit and the implementation
  settled: a read stops at the last newline in the chunk, so a partial trailing line is left for the
  next tick (the Common Gotchas row below); and a file *shorter* than the tracked offset means the
  addon's cap-triggered rewrite ran, so the task re-reads from zero and drops entries whose `id` it
  already emitted (ids are monotonic within one proxy process, so a container restart plus a rewrite
  can still cost a few rows — lossy by design, Resolved Decision 1).
- `src-tauri/src/commands/proxy.rs`: `async fn watch_proxy_flows(name: String, registry:
  State<'_, FlowWatchRegistry>, app: tauri::AppHandle) -> Result<(), AppError>` — `async` like
  `watch_workspace_ports`, so it runs on the async runtime rather than the IPC thread — calls
  `flowlog::start_watch`, ignoring the `bool` (a no-op second call is not an error, matching
  `watch_workspace_ports`'s own idempotent framing).
- `src-tauri/src/commands/mod.rs`: register `proxy::watch_proxy_flows` in `tauri::generate_handler![]`.

## Implemented: Phase D — frontend

- `src/lib/proxy/traffic.svelte.ts` (new): `FLOW_LOG_MAX_LINES = 500` (Resolved Decision 1, comment
  pointing at the Rust-side constant), `FlowEntry` type mirroring the Rust struct (camelCase per its
  `serde(rename_all)`), a module-scoped `$state` map of `sandbox -> FlowEntry[]`, a single
  `listen("proxy://flows-changed", ...)` subscription (started idempotently, mirroring
  `ensureNetworkState`) that appends the payload's `flows` to the named sandbox's array and slices to
  the last `FLOW_LOG_MAX_LINES`, and `flowsFor(sandbox): FlowEntry[]`.
- `src/routes/projects/TrafficPanel.svelte` (new): props `{ sandboxName, workspacePath, action }`
  matching `ProxyPanel.svelte`. An `$effect` calls `proxy_status(sandboxName)` (already exists,
  `src-tauri/src/commands/proxy.rs`) and, when `running` is true, calls `watch_proxy_flows(sandboxName)`
  once. Renders `flowsFor(sandboxName)` from `traffic.svelte.ts` as a row list (method, host, truncated
  path with a title tooltip, status or "error", duration) with a `secret_injected` badge, a text filter
  input (host/path substring, client-side `Array.prototype.filter`), and method/status-class filter
  chips (client-side). Clicking a row toggles an expanded state showing the untruncated path, exact
  `ts`, and byte counts — no header/body content, per `01_ideas.md`'s Capture Scope decision. Rows
  render newest first, since the store appends and this is a live log. Not rendered at all when
  `proxy_status` reports `running: false`, matching `ProxyPanel.svelte`'s own gate on its rule editor.
  `workspacePath` and `action` are declared as props (the dashboard's call site passes them, matching
  `ProxyPanel.svelte`) but unused — this panel needs neither.
- `src/lib/workspace/WorkspaceDashboard.svelte`: add `import TrafficPanel from
  "../../routes/projects/TrafficPanel.svelte";` and mount `<TrafficPanel {sandboxName} {workspacePath}
  {action} />` directly after the existing `<ProxyPanel {sandboxName} {workspacePath} {action} />` line.
- `messages/en.json`, `de.json`, `ru.json`: keys for the panel's static copy (section label, empty
  state, filter placeholders). Per the `feature-implement` skill, the i18n pass runs after the user has
  approved the working feature, not inside Phase D — `TrafficPanel.svelte` ships with the same
  `<!-- TODO(i18n) -->`-marked literal strings `ProxyPanel.svelte` already uses in the interim.

## Cross-Cutting Concerns

**Parallel-worktree state.** No new host-global resource. `<state>/flows.ndjson` and
`flowlog_addon.py` live under `<app_data_root>/proxy/<sandbox>/`, already routed through
`crate::paths::app_data_root` and therefore `MIO_APP_DATA_DIR`-relocatable — the same category
`docs/features/network-proxy/02_dev.md`'s own Cross-Cutting Concerns already established for that
directory's other contents. `FlowWatchRegistry` is in-process Tauri state, not a file or port, so it
carries no cross-worktree collision risk at all. No new host port, no new bundled binary, no new
`build.rs` asset group (the addon script is `include_str!`-embedded, matching `ADDON_SCRIPT`'s
existing mechanism).

**Teardown.** No new teardown path. `remove_proxy` (`src-tauri/src/proxy/mod.rs`) already deletes the
whole `<app_data_root>/proxy/<sandbox>/` tree, which now additionally contains `flows.ndjson` and
`flowlog_addon.py` — no code change needed there. The poll task in `watch_flows` self-terminates on its
next tick after that deletion (Phase C), so no explicit stop-on-destroy call is added to
`close_panes_and_forwards`/`workspace::close_app` (`src-tauri/src/services/sandbox.rs`) — the task
notices its own file is gone within one `FLOW_LOG_POLL_INTERVAL` and exits.

**State derivation.** `TrafficPanel.svelte`'s visibility is derived the same way `ProxyPanel.svelte`'s
rule editor already is — from `proxy_status(sandboxName).running`, never persisted. The flow log's
*content* is not re-derivable the way rules/policy are: `flows.ndjson` is the one-and-only record of
what the proxy observed, capped and lossy by design (Resolved Decision 1), not a materialised copy of
some workspace-side source of truth.

**Testability.** `create_proxy`'s file writes (`flowlog_addon.py`, `flows.ndjson`'s initial empty file)
run as plain `std::fs` calls regardless of `MIO_MOCK_BACKEND`, unlike the container-launching calls
that go through the mocked `ProcessRunner` (`docs/features/scriptable-mock-backend/02_dev.md`). A
future `light`-tier spec can therefore exercise Phase C's poll-loop-to-event-to-frontend-store path
end to end by pre-seeding `<state>/flows.ndjson` with fake JSON lines directly (bypassing the need for
a real `mitmdump` process) the way `e2e/AGENTS.md`'s `light` tier already substitutes file state for
real container behaviour elsewhere in this feature line. Capturing *real* traffic and confirming the
addon's own `response()`/`error()` output is only provable against a real Podman machine — `standard`
tier, the same split `docs/features/network-proxy/02_dev.md`'s own Common Gotchas table already
documents for this feature line ("Feature works with `./build e2e light` but fails against real
Podman").

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The whole app aborts (`panic in a function that cannot unwind`, `thread caused non-unwinding panic`) right after `proxy_status` returns `running: true` for a proxy-enabled sandbox | `start_watch` spawned the poll task with `tokio::spawn` from a *synchronous* `#[tauri::command]`. A sync command runs on the IPC thread with no Tokio runtime entered, so `tokio::spawn` panics — and that panic cannot unwind across the webview's FFI boundary, so it aborts the process instead of surfacing as a rejected `invoke()` | Spawn detached tasks with `tauri::async_runtime::spawn`, which carries its own runtime handle and is safe from any thread; keep `watch_proxy_flows` `async` as well, matching `watch_workspace_ports` (`src-tauri/src/commands/panes.rs`) |
| The traffic panel never shows any flows even though the sandbox is proxy-enabled and generating traffic | `watch_proxy_flows` was never called (the panel's mount `$effect` did not fire, or `proxy_status` reported `running: false` at the time it checked), so no poll task exists for this sandbox | Confirm `TrafficPanel.svelte`'s `$effect` calls `watch_proxy_flows(sandboxName)` after `proxy_status` reports `running: true`, and that `FlowWatchRegistry` (`src-tauri/src/proxy/flowlog.rs`) shows a live task for the sandbox |
| Flow rows appear then abruptly stop updating while the sandbox is still proxy-enabled | The poll task self-terminated because `<state>/flows.ndjson` briefly returned `NotFound` — a proxy container restart (`restart_proxy_container`) does not delete the file, so this should not happen from a rules/policy change alone; check whether the proxy was actually removed and recreated | Call `watch_proxy_flows(name)` again — it is idempotent (`FlowWatchRegistry::start_watch`) and starts a fresh task if the old one exited |
| A flow that clearly happened (confirmed by the tool succeeding) shows only a generic `"Connection killed."` error row, with no status code | The flow ended via `flow.kill()` in an earlier addon rather than a set `flow.response`; Phase A's gating spike (2026-08-18) found `flow.kill()` reaches `FlowLogger.error()` (not `response()`), every time, with `flow.error` stringifying to exactly `"Connection killed."` | Not a missing flow — check for the `error()`-hook entry at that host/path/timestamp; the row is expected to carry no status code and to be indistinguishable from any other `error()`-hook cause, per Phase A's flagged finding and `01_ideas.md`, Open Questions, Resolved Decision 2 |
| Two duplicate-looking poll tasks are both emitting `proxy://flows-changed` for the same sandbox, doubling every row | `watch_proxy_flows` was called twice before `FlowWatchRegistry::start_watch`'s `is_finished` check ran, in a narrow race between two rapid calls | Confirm `start_watch`'s registry check-and-insert happens under the same lock, not as two separate operations — if the race is real, serialize `start_watch` the way `MutagenNetworkManager.expose_lock` serializes `expose_port` |
| The frontend flow list grows without bound and the tab becomes sluggish over a long session | `traffic.svelte.ts`'s ring buffer cap was not applied after appending a `proxy://flows-changed` payload | Confirm the store slices to `FLOW_LOG_MAX_LINES` (`src/lib/proxy/traffic.svelte.ts`) on every append, matching the addon-side cap in `mitm_flowlog_addon.py` |
| `flows.ndjson` on disk is smaller than `FLOW_LOG_MAX_LINES` would suggest, or a poll tick parses a partial/corrupt final line | The addon's truncate-on-cap rewrite and an in-flight poll-loop read raced, or the poll loop read mid-append (the addon does not fsync/atomically replace the file on every append, only on a cap-triggered rewrite) | Treat a JSON-parse failure on the last line of a read chunk as "not yet complete, retry next tick" rather than an error — do not drop the whole tick's batch over one partial trailing line |
| A secret value or full header appears anywhere in a flow row, expanded detail, or the `flows.ndjson` file itself | A change to `mitm_flowlog_addon.py` started logging header/body content, reopening the exposure `01_ideas.md`'s Capture Scope decision deliberately closed | Revert to the metadata-only `FlowEntry` schema (Phase B); if header/body visibility is genuinely wanted, it needs its own secret-exposure design pass, not an incidental addition to this addon |
