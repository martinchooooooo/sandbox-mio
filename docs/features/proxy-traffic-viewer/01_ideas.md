# Proxy Traffic Viewer: Per-Sandbox Flow Log Panel

This document covers a per-sandbox UI panel showing the HTTP(S) flows the `network-proxy` mitmproxy
sidecar (`docs/features/network-proxy/01_ideas.md`, `02_dev.md`) has observed for a proxy-enabled
sandbox: method, host, path, status, and timing per flow, plus filtering and click-to-detail. This
was scoped out of `network-proxy` explicitly (`docs/features/network-proxy/04_improvements.md`,
"Traffic viewer UI") pending that feature's container/reachability design settling, which it now has —
`network-proxy` is implemented (`src-tauri/src/proxy/mod.rs`, `rules.rs`, `types.rs`,
`src-tauri/resources/mitm_addon.py`, `src-tauri/src/commands/proxy.rs`,
`src/routes/projects/ProxyPanel.svelte` all exist and match `docs/features/network-proxy/02_dev.md`'s
design, though that document's own phase table is stale and still marks them "Remaining" — this
document treats the code, not the phase table, as ground truth, the same convention
`docs/features/proxy-network-policy/01_ideas.md` already adopted). Nothing in this document is
implemented. Relevant existing files: `src-tauri/src/proxy/mod.rs` (`create_proxy`, `state_dir`,
`proxy_container_name`, `status`), `types.rs`, `rules.rs` (the proxy subsystem this feature reads
from and extends), `src-tauri/resources/mitm_addon.py` (the one existing mitmproxy addon script),
`src-tauri/src/provider/types.rs` (`MachineProvider`, `SidecarSpec`, `SidecarMount` — the seam a new
mount goes through), `src-tauri/src/network/mod.rs` (`watch_ports_file`/`poll_ports_file`,
`emit_forwards` — the established poll-loop-plus-event pattern this feature reuses),
`src/lib/network/state.svelte.ts` (the frontend store shape that pattern feeds), and
`src/routes/projects/ProxyPanel.svelte` / `PortsManager.svelte` (the two existing per-sandbox panels
this feature's own panel sits beside in `src/lib/workspace/WorkspaceDashboard.svelte`). A sibling
planning pass, `docs/features/proxy-network-policy/01_ideas.md` and `02_dev.md`, is adding domain
allow/deny policy and interactive per-host approval to the same mitmproxy sidecar; its own
`04_improvements.md` records a deferred "Blocked-request visibility" idea that would consume this
feature's flow log once both exist. This document is aware of that overlap (see Current State and
Open Questions) but does not design for it — `proxy-network-policy` is not implemented yet (every
phase in its `02_dev.md` is `Remaining`), and this feature's own scope is the flow log itself, not a
blocked/allowed indicator on top of it.

## Current State

**No flow is captured anywhere today beyond mitmdump's own unstructured stdout.**
`create_proxy` (`src-tauri/src/proxy/mod.rs`) launches `mitmdump` with `-s /addons/addon.py` and no
other logging flag. `docs/features/network-proxy/02_dev.md`'s Spike Results record that "mitmdump
logs one line per flow to stdout, including the client's bridge IP" — a human-readable line mitmdump
itself formats, not a structured record this project owns or controls, and nothing reads that stdout
today. `MachineProvider` (`src-tauri/src/provider/types.rs`) has no `logs` method, and no code
anywhere in `src-tauri/src` calls `podman logs`.

**`mitm_addon.py`'s only addon, `SecretInjector`, hooks `request()` only.** It matches a request's
host/path against `/addons/rules.json` and rewrites a header; it never observes a response, a status
code, or timing, and writes nothing to disk. It is the one precedent for "project-owned logic mounted
into the proxy container," and this feature follows the same shape: a script this project authors,
embedded via `include_str!` and written to `<state>/addon.py`-equivalent at every `create_proxy`, not
a mitmproxy built-in.

**The proxy's state directory is already a two-way bind mount the app reads from directly.**
`docs/features/network-proxy/02_dev.md`'s Spike Results establish that a Windows host directory
bind-mounted as mitmdump's confdir lets the container (running as uid 1000, not root) write files the
app reads with ordinary `std::fs` calls — `read_ca_pem` (`src-tauri/src/proxy/mod.rs`) is the existing
example, no `podman exec` round trip needed. Every mount `create_proxy` sets up today flows
app-to-container (`addon.py`, `rules.json`) or is container-authored and host-read once at proxy
startup (the CA cert). Nothing today is a container-authored feed the app polls repeatedly while the
proxy runs — this feature is the first.

**A poll-loop-plus-event pattern for per-sandbox near-live frontend data already exists, for a
different subsystem.** `watch_ports_file`/`poll_ports_file` (`src-tauri/src/network/mod.rs`) run one
poll-loop task per watched sandbox, reading `.mio/ports.json` and a listening-socket scan on a fixed
interval (`PORTS_FILE_POLL_INTERVAL`), and call `emit_forwards` (same file) to push a full snapshot
over the `network://forwards-changed` Tauri event. `src/lib/network/state.svelte.ts` is the
module-scoped `$state` store that subscribes once (`ensureNetworkState`) and updates on every event.
`PortsManager.svelte` (`src/routes/projects/PortsManager.svelte`) reads that store filtered to its own
`sandboxName`, needing no per-mount `invoke` call. This is the closest existing precedent for how a
new per-sandbox, frequently-changing dataset should reach the frontend.

**`ProxyPanel.svelte` establishes the per-sandbox proxy-panel convention this feature's panel
follows.** It takes `{ sandboxName, workspacePath, action }` props (`action` unused today, the same
`createActionRunner` handle every panel in `WorkspaceDashboard.svelte` receives), calls
`proxy_status`/`list_proxy_rules` once on mount via an `$effect`, and is mounted directly below
`PortsManager.svelte` in `src/lib/workspace/WorkspaceDashboard.svelte`
(`<PortsManager {sandboxName} {workspacePath} {action} />` then `<ProxyPanel {sandboxName}
{workspacePath} {action} />`). `ProxyPanel.svelte` already states, verbatim, "Traffic that ignores the
proxy (raw sockets, git-over-SSH) is not inspected" — the same caveat this feature's own captured-flow
list inherits for free, since it only ever shows what the proxy actually saw.

## Proposed Model

### A third mitmproxy addon script owns flow capture, independent of `proxy-network-policy`'s addon

`src-tauri/resources/mitm_flowlog_addon.py` (new), embedded via `include_str!` in
`src-tauri/src/proxy/mod.rs` and written to `<state>/flowlog_addon.py` at every `create_proxy`, mounted
read-only at `/addons/flowlog_addon.py`, added as a third `-s` argument to `mitmdump`'s command vec. A
separate file rather than a new class appended to `mitm_addon.py`'s existing `addons` list, so this
feature's build never needs to touch a file `proxy-network-policy` (`docs/features/proxy-network-policy/02_dev.md`,
Phase B) is concurrently modifying (`SecretInjector.request()`'s planned `flow.response` guard). Load
order relative to the existing `addon.py` and any future `policy_addon.py` does not matter for this
feature: the addon hooks `response()` and `error()` (see below), both of which fire once a flow's
outcome is already decided, regardless of which other addon produced that outcome or in what order the
`-s` flags were given.

### Flow capture: one JSON line per finished flow, appended to a mounted NDJSON file

The addon's `response(self, flow)` hook appends one JSON object to `/addons/flows.ndjson` for every
flow that completes with an HTTP response — from the real upstream, or a synthetic one an earlier
addon set on `flow.response` directly (relevant once `proxy-network-policy` ships, since a `"deny"`
policy verdict resolved by setting `flow.response` rather than calling `flow.kill()` would still reach
this hook; a verdict resolved by `flow.kill()` would not — see Open Questions). Each line carries a
generated flow id, `flow.request.method`, `flow.request.pretty_host`, `flow.request.path`,
`flow.response.status_code`, `flow.request.timestamp_start`, and a duration computed from
`flow.response.timestamp_end - flow.request.timestamp_start` (`mitmproxy.http.HTTPFlow`/`Message`
expose both timestamps; neither mitmproxy nor this feature needs a third-party timing library). The
addon's `error(self, flow)` hook appends a second, narrower line for a flow that never got a response
at all (connection refused, TLS failure, timeout) — status `null`, an `error` string field instead —
so a failed connection still shows up in the panel rather than silently vanishing from it.

### What is deliberately not captured: header values, bodies, or the injected secret

A flow line carries no request/response headers and no body content — only the metadata named above,
plus request/response byte counts (from the `Content-Length` header when present, else the decoded
body's length) and a boolean `secret_injected` flag (true when the flow's host/path matched a
`SecretInjector` rule, false otherwise; the addon does not need to know which header or value, only
whether one of `/addons/rules.json`'s entries matched — `mitm_flowlog_addon.py` loads the same
`rules.json` `mitm_addon.py` does, purely to check for a host/path match, never to read a `value`
field). This mirrors `docs/features/network-proxy/02_dev.md`'s existing secret-exposure boundary: the
whole point of proxy-side injection is that the real credential never has to leave the proxy
container's own filesystem, and a flow-viewer panel that echoed header values back over Tauri IPC into
the frontend would reopen exactly the exposure the base feature closed.

### Transport to the app: a poll loop over the mounted file, not a new streaming channel

`src-tauri/src/proxy/flowlog.rs` (new) follows `network::poll_ports_file`'s shape: one poll-loop task
per sandbox with an open workspace pane, reading `<state>/flows.ndjson` (the same host-readable bind
mount `read_ca_pem` already proves works with plain `std::fs`) on a fixed interval, tracking a byte
offset so each tick only parses newly appended lines, and emitting a `proxy://flows-<sandbox>` (or an
equivalent single app-global event carrying a `sandbox` field, mirroring `network://forwards-changed`'s
one-event-covers-every-sandbox shape — left to `02_dev.md`, see Open Questions) Tauri event with the
new flows. No new TCP server, no reuse of `forward_create_reverse` — unlike
`proxy-network-policy`'s interactive-approval channel, this feature has no need for the proxy container
to reach the app synchronously; it only ever needs the app to notice a file grew, which a poll loop
already does cheaply for `.mio/ports.json`.

### Bounding: the addon caps the file itself, so the app never reads an unbounded log

`mitm_flowlog_addon.py` keeps flow capture bounded at the source rather than relying on the app-side
poll loop to discard old data: on each append, if the file's line count exceeds a fixed cap, the addon
rewrites the file keeping only the most recent lines (the exact cap value is an Open Question below).
This keeps a long-running sandbox's flow log from growing without bound on disk, and keeps the app's
poll-loop parse cost bounded per tick regardless of how long the sandbox has been running. The
frontend store mirrors the same cap as its own in-memory ring buffer, so a browser tab left open for a
long session does not grow unbounded either.

### Frontend: `TrafficPanel.svelte`, mounted beside `ProxyPanel.svelte`

`src/routes/projects/TrafficPanel.svelte` (new), matching `ProxyPanel.svelte`'s prop shape
(`{ sandboxName, workspacePath, action }`) and mounted directly after it in
`src/lib/workspace/WorkspaceDashboard.svelte`. Backed by a new module-scoped store,
`src/lib/proxy/traffic.svelte.ts` (alongside `src/lib/network/state.svelte.ts`'s pattern), that
subscribes to the flow event once and exposes flows filtered by sandbox, the same
`openForwardsFor`-style filter `PortsManager.svelte` already uses. Only rendered when
`proxy_status(sandboxName).running` is true — an unproxied sandbox has no flow log to show, the same
gate `ProxyPanel.svelte` already applies to its own rule editor.

### Per-flow display and extensions: filtering and click-to-detail, both client-side

Each row shows method, host, path (truncated with a title tooltip for the full string), status (or
"error" for an `error()`-hook line), and duration. A `secret_injected` flow gets a small badge, since a
user watching the panel benefits from seeing which requests carried a real credential without the
value itself ever appearing. Filtering (by host substring, method, or status class 2xx/3xx/4xx/5xx/error)
runs entirely client-side against the already-bounded in-memory ring buffer — no backend filter
parameters needed, since the dataset size is capped by design (Bounding, above). Clicking a row expands
it in place to show the fields already captured at full precision (untruncated path, exact timestamp,
exact byte counts) — not a raw header/body dump, consistent with What Is Deliberately Not Captured.

## Technology Choices with Tradeoffs

### Flow export: an addon-authored NDJSON file (chosen) vs. mitmdump's native `-w` flow-file export vs. tailing mitmdump's stdout vs. `mitmweb`

Chosen: a project-owned addon script hooks `response()`/`error()` and appends one JSON line per flow
to a file this project controls the schema of, mounted the same way `rules.json` already is.

- **`mitmdump -w <path>`, mitmproxy's built-in flow-file export** — rejected: the format is
  mitmproxy's own internal flow serialization (read back via `mitmproxy.io.FlowReader` or `mitmdump
  -r`), not JSON or any format a Rust poll loop can parse without either shelling out to mitmproxy's
  own Python tooling on every poll tick or reimplementing that internal format in Rust. An
  addon-authored NDJSON file needs neither: the app reads exactly the schema the addon wrote.
- **Tailing mitmdump's own stdout via `podman logs -f`** — rejected: the default one-line-per-flow
  format is meant for a human watching a terminal, not a stable machine-parseable schema, and
  `docs/features/network-proxy/02_dev.md`'s Spike Results only observed it including host/method/status
  — no confirmed timing, byte-count, or secret-injected fields, and no way to add them without still
  writing an addon (at which point the addon may as well write the structured file directly).
  `MachineProvider::exec_command` (`src-tauri/src/provider/types.rs`) does expose the "live
  bidirectional streaming" primitive `podman logs -f` would need, so this option was technically
  available, not merely unbuilt — it was rejected on schema-control grounds, not on feasibility.
- **`mitmweb`, mitmproxy's own browser-based live flow viewer** — rejected: it is a second HTTP+
  WebSocket server the proxy container would need to publish, which conflicts directly with
  `docs/features/network-proxy/01_ideas.md`'s reachability design — the sane-default requirement that
  no proxy port is published to any interface beyond the sandbox's own bridge network
  (`docs/features/network-proxy/02_dev.md`'s Spike Results already found the bridge itself unreachable
  from the Podman machine's own root netns, a property `mitmweb` would need to punch a hole through).
  It also renders its own UI, not this project's shadcn-svelte/retro-ui-svelte component set
  (`AGENTS.md`, UI Components), so embedding it would be a second, inconsistent UI surface inside a
  per-sandbox panel that otherwise matches every other panel's look.

### Transport to the app: poll loop over a bind-mounted file (chosen) vs. a new TCP push channel vs. a Tauri filesystem watcher

Chosen: extend the existing `poll_ports_file`-style poll loop pattern to a second per-sandbox dataset.

- **A new TCP server the addon pushes flows to, reusing `forward_create_reverse`** — rejected as
  overkill for this feature specifically, unlike `proxy-network-policy`'s interactive-approval channel:
  that feature needs a synchronous round trip (the addon blocks a flow awaiting an answer); this
  feature only needs the app to notice new data eventually, which a poll interval already serves at
  negligible cost, without adding a second `forward_create_reverse` session, a second fixed
  `_VM_PORT` constant, or an in-memory pending-request registry — all machinery this feature has no use
  for.
- **A native filesystem watcher (`notify` crate) on `<state>/flows.ndjson`** — rejected: this project
  already has one polling primitive for "watch a per-sandbox file the container writes"
  (`watch_ports_file`), and a bind-mounted path crossing the Podman machine's virtiofs/9p boundary is
  exactly the kind of path a host-native filesystem-event watcher is least reliable against (events
  from inside a VM's virtual filesystem do not reliably surface as host filesystem-change
  notifications); the existing poll interval sidesteps that reliability question entirely and is
  already proven against a bind mount of the same shape (`.mio/ports.json` is itself workspace-mounted
  into the sandbox).

### Capture scope: response/error metadata only, no headers or bodies (chosen) vs. full request/response capture

Chosen: the addon captures only the fields under Proposed Model's "What is deliberately not captured,"
never a header value or body byte.

- **Full header and body capture, mirroring what mitmproxy itself can see** — rejected: it reopens the
  exact exposure `docs/features/network-proxy/02_dev.md`'s Secret Handling design closed. A flow whose
  `SecretInjector` rule fired has the real credential sitting in its own `Authorization` (or similar)
  header at the exact point `response()` observes it; logging that value into a file the app then
  serializes over Tauri IPC into a Svelte component's DOM would put the secret exactly where the base
  feature's entire design goal was to keep it from ever appearing. A body can carry equally sensitive
  material (request payloads, response content) with no way to distinguish "safe to show" from "not"
  generically. The metadata-only schema is deliberately conservative on this axis.

## Validation

Desk research against mitmproxy's own API documentation (`docs.mitmproxy.org`, fetched 2026-08-18)
confirms `mitmproxy.http.HTTPFlow` and its `Request`/`Response`/`Message` base attributes:
`timestamp_start`/`timestamp_end` (float, inherited from `Message`) exist on both `flow.request` and
`flow.response`, `pretty_host`/`path`/`method` exist on `Request`, `status_code` exists on `Response`,
and mitmproxy provides no built-in duration/elapsed property — a duration is always `timestamp_end -
timestamp_start`, computed by the addon, not read off a single field.

**A gating spike was run on 2026-08-18** (`docs/features/proxy-traffic-viewer/spike/`, throwaway
`mio-spike-*` podman containers/network against `docker.io/mitmproxy/mitmproxy:11.1.3`, the exact tag
pinned as `MITMPROXY_IMAGE_DEFAULT` in `src-tauri/src/proxy/mod.rs`), settling both items desk research
alone could not:

- **`response()` fires when an earlier addon sets `flow.response` directly.** Confirmed. `-s a_synth.py
  -s b_synth.py`: `a_synth.py`'s `request()` hook set a synthetic 403 response (with a distinctive
  `X-Synth-Blocked: true` header) for path `/synth`, and `b_synth.py`'s `response()` hook fired and
  observed `status=403 synth_header=true` for that same flow. A future `proxy-network-policy` `"deny"`
  verdict resolved via `flow.response` (rather than `flow.kill()`) would reach this feature's flow log
  as a captured entry, exactly as `01_ideas.md`'s Proposed Model assumed.
- **`flow.kill()`'s effect on a later addon's hooks.** Confirmed, and more specific than assumed:
  `-s a_kill.py -s b_kill.py`, `a_kill.py`'s `request()` hook calling `flow.kill()` for path `/kill`
  did **not** trigger `b_kill.py`'s `response()` hook, but **did** trigger its `error()` hook, with
  `flow.error` stringifying to exactly `"Connection killed."`. This is not the "or neither" case this
  document and `02_dev.md`'s Phase A left open — a `flow.kill()`-based block is not invisible to a
  `response()`/`error()`-hooking addon; it surfaces via `error()` with a generic, non-null message that
  carries no status code and is indistinguishable from any other addon calling `flow.kill()` for an
  unrelated reason. See `02_dev.md`'s Phase A section for the resulting flag against Phase B's
  "accepted gap" framing.
- **Three `-s` scripts load and run cleanly together.** Confirmed. `-s addon.py -s
  policy_addon_stub.py -s flowlog_addon.py` (the real `mitm_addon.py`'s current logic, a throwaway
  policy-addon stand-in, and a `mitm_flowlog_addon.py` stand-in) all logged a `Loading script` line
  with no error against `mitmproxy/mitmproxy:11.1.3`, and a single test request produced all three
  scripts' own hook output (`addon.request`, `policy_addon_stub.request`, `flowlog.response`) plus a
  correctly appended `flows.ndjson` line.
- **`timestamp_start`/`timestamp_end` populated, duration sane.** Confirmed on an ordinary successful
  flow (client -> mitmdump -> a same-network plain-HTTP target container): `duration_ms` computed as
  `(timestamp_end - timestamp_start) * 1000` came out to values in the 6-17ms range across runs —
  millisecond-scale and plausible for same-bridge-network traffic, not zero/negative/absurd.
- **`error()` fires for an unreachable host, with no `response()` for the same flow.** Confirmed:
  targeting an unused IP on the spike's own bridge subnet (`10.89.0.250` on a `10.89.0.0/24` network)
  produced exactly one `flows.ndjson` line, from `error()`, with `status: null` and
  `error: "[Errno 113] Connect call failed ('10.89.0.250', 80)"` — `response()` never fired for that
  flow (mitmproxy itself synthesizes a 502 back to the client, but that synthetic reply is not exposed
  to the addon's `response()` hook the way a `flow.response`-set reply from another addon is).

No blocker surfaced — every risk this spike targeted cleared. The scripts used are preserved at
`docs/features/proxy-traffic-viewer/spike/` (`a_synth.py`/`b_synth.py`, `a_kill.py`/`b_kill.py`,
`addon.py`, `policy_addon_stub.py`, `flowlog_addon.py`) per this project's spike-artefact convention.

## Open Questions

- **Exact flow-log cap.** Proposed Model's Bounding section commits to capping `flows.ndjson` at a
  fixed line count but does not choose the number. Too small loses history a user might want after
  switching away from the panel briefly; too large costs more disk and more per-tick parse work on
  every poll. Left for `02_dev.md` to pick, informed by `PORTS_FILE_POLL_INTERVAL`'s existing interval
  choice (`src-tauri/src/network/mod.rs`) as a reference point for "cheap enough to poll often."
  Additionally undecided: whether the cap is a fixed constant or grows/shrinks with the poll interval.
- **Response-hook behavior on a synthetic (`flow.response`-set) response, and its interaction with a
  future policy-blocked flow.** The mechanism itself is now confirmed by this feature's own gating
  spike (Validation, 2026-08-18): `response()` fires and sees a `flow.response` set by an earlier
  addon, while `flow.kill()` instead fires `error()` with a generic `"Connection killed."` message (no
  status code). Still open: whether this feature's flow log shows a *`proxy-network-policy`* block as a
  captured `response()` entry or an `error()` entry with that generic message depends on which
  mechanism `proxy-network-policy` itself chooses (`docs/features/proxy-network-policy/01_ideas.md`,
  Open Questions references its own hook-mechanism uncertainty) — this document does not resolve that
  choice, only the two mechanisms' effects on this feature's own addon.
- **Event shape: one app-global event carrying a `sandbox` field vs. one event name per sandbox.**
  `network://forwards-changed` uses the first shape (one event, every sandbox's forwards in the
  payload, client-side filtered). A flow log's payload could instead be per-sandbox
  (`proxy://flows-<sandbox>`) to avoid re-serializing every other open sandbox's unrelated flows on
  every tick. Left open pending a rough sense of typical flow volume per poll interval, which the
  gating spike does not measure.
- **Whether flow capture is itself a separate opt-in from the base proxy toggle, or automatic whenever
  a sandbox is proxy-enabled.** `network-proxy`'s own proxy toggle is opt-in
  (`docs/features/network-proxy/02_dev.md`, Resolved Decision 3) because it changes the sandbox's
  trust/attack surface; flow capture changes no reachability or trust boundary (it is purely
  additive, host-side-only metadata capture from traffic already flowing through the proxy), so the
  case for gating it behind its own separate opt-in is weaker than the base proxy toggle's — but a
  metadata log of every host and path a sandbox's agent contacted is still information a user might
  reasonably want off by default, or might specifically want captured for auditing an agent's
  behavior. `02_dev.md` should decide whether flow capture is unconditional for every proxy-enabled
  sandbox or its own toggle.
- **Retention across a proxy container restart** (e.g. `restart_proxy_container`, called by
  `apply_rules` on a rule change). `flows.ndjson` lives under `<state_dir>` and is not touched by
  `create_proxy`'s own file writes (unlike `rules.json`, which `create_proxy` resets to an empty array
  every time), so a restart should not lose flow history — but this is inferred from the existing
  `create_proxy` code shape, not confirmed against an implementation of this feature. Left for
  `02_dev.md` to confirm explicitly rather than leave implicit.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A flow that should have appeared in the panel is missing entirely, or shows only a generic `"Connection killed."` error | The flow ended via an earlier addon's `flow.kill()` rather than a set `flow.response`; the gating spike (Validation, 2026-08-18) confirmed this reaches `mitm_flowlog_addon.py`'s `error()` hook, not `response()`, with `flow.error` stringifying to exactly `"Connection killed."` — no status code, indistinguishable from any other `flow.kill()` call | Not a missing flow — check the `error()`-hook entry for that host/path/timestamp; a `flow.kill()`-based block from a future policy addon surfaces as this generic error line, not a captured status code (Open Questions) |
| The traffic panel shows nothing even though the sandbox is proxy-enabled and traffic is flowing | `TrafficPanel.svelte` gates rendering on `proxy_status(sandboxName).running`, or the poll loop (`src-tauri/src/proxy/flowlog.rs`) was never started for this sandbox's pane | Confirm `proxy_status` reports `running: true`, and that the flow poll loop was started the same way `watch_ports_file` starts on workspace-pane open (`src-tauri/src/commands/panes.rs`) |
| Flow rows stop updating after the sandbox has been open a long time | `flows.ndjson`'s line-count cap (Open Questions) truncated older lines out from under a poll tick that had not yet read them, or the frontend ring buffer's own cap silently dropped the oldest rows | Not a bug in isolation — both caps are deliberate (Proposed Model, Bounding); increase the cap in `02_dev.md`'s implementation if history loss is a real problem for a given workflow |
| A secret value appears in a flow row's expanded detail view | A future change added header or body capture to `mitm_flowlog_addon.py`, reopening the exposure Technology Choices' "Capture scope" deliberately closed | Revert to metadata-only capture; if header/body visibility is genuinely wanted, redesign with the same secret-exposure care `docs/features/network-proxy/02_dev.md`'s Secret Handling section already gives the injection path, not as an incidental addition here |
| The panel's flow count and the proxy container's actual traffic volume disagree noticeably | `mitm_flowlog_addon.py` only logs on `response()`/`error()`; a flow that is still in flight when the panel is viewed (e.g. a long-lived streaming response) has not yet completed and so has not yet been written | Expected and not a bug; a flow appears once mitmproxy considers it finished, the same point at which `docs/features/network-proxy/02_dev.md`'s own Spike Results observed mitmdump's default stdout logging a line |
