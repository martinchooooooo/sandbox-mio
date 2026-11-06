---
type: ideas
title: "Proxy Non-HTTP(S) Capture: Bypass Visibility"
feature: proxy-non-http-capture
---
# Proxy Non-HTTP(S) Capture: Bypass Visibility

This document covers the deferred idea recorded in `docs/features/network-proxy/04_improvements.md`
under "Non-HTTP(S) traffic capture": traffic from a proxy-enabled sandbox that ignores
`http_proxy`/`https_proxy` — raw TCP clients, git-over-SSH, any tool with its own transport — bypasses
the `network-proxy` mitmproxy sidecar silently. That deferred idea itself names two distinct
directions: (a) a lightweight "N connections were not inspected" visibility signal, and (b) a
mitmproxy SOCKS5/generic-TCP capture mode for tools that support a SOCKS proxy setting even when they
don't support an HTTP one. This document investigates both concretely and recommends building only
(a) as v1; the (b) investigation and its findings are recorded here but the work itself is deferred to
this feature's own `docs/features/proxy-non-http-capture/04_improvements.md`. Nothing in this
document is implemented. This feature applies to Podman sandboxes only: a microsandbox sandbox
routes all guest TCP through microsandbox's host-side stack and, under
`docs/features/microsandbox-network-policy/01_ideas.md`, through a chained mitmproxy, so no traffic
bypasses inspection and there is nothing to count. Relevant existing files: `src-tauri/src/proxy/mod.rs`, `types.rs`, `rules.rs`
(the `network-proxy` subsystem this feature extends — implemented, despite
`docs/features/network-proxy/02_dev.md`'s phase headings still reading "Remaining"),
`src-tauri/src/network/mod.rs` (`scan_listening_ports`/`parse_listening_ports`, `poll_ports_file` — the
established pattern for reading a sandbox's live socket table from the host), `src-tauri/src/provider/
types.rs` (`MachineProvider::exec`, `ExecOpts`), `src-tauri/src/commands/panes.rs` (where
`watch_ports_file` is started, the lifecycle hook this feature's own poll loop mirrors), and
`src/routes/projects/ProxyPanel.svelte` (the panel that already states the bypass caveat this feature
makes concrete). Two sibling planning passes touch the same subsystem and are aware of, but not
designed around, by this document: `docs/features/proxy-traffic-viewer/01_ideas.md` (a flow-log panel
sourced from what mitmdump's addon *does* see — see Current State for why that feature structurally
cannot see what this one is about) and `docs/features/proxy-network-policy/01_ideas.md` (domain
allow/deny enforcement, which acts on the same proxied traffic the base feature already sees, not on
bypass traffic).

## Current State

**A proxy-enabled sandbox has no network path through the proxy container for non-proxy-aware
traffic — the mitmproxy sidecar is not in-line.** `docs/features/network-proxy/02_dev.md`'s Spike
Results and Resolved Decision 1 establish that a proxy-enabled sandbox joins `mio-proxy-<sandbox>`
*instead of* `pasta`, not alongside it (Podman rejects combining `pasta` with a second `--network`).
Both the sandbox and the `<sandbox>-proxy` container sit on the same private bridge and share its
outbound NAT egress. A tool inside the sandbox that honors `http_proxy`/`https_proxy` (set by
`proxy_env` in `src-tauri/src/proxy/mod.rs`) dials the proxy's pinned bridge IP on `PROXY_PORT` (8080);
a tool that ignores those variables dials its target directly, going out over the *same* bridge's NAT
path the proxy itself uses for its own onward connections, but never transiting the proxy process at
all. This is a structural consequence of the chosen "explicit env var" interception mechanism
(`docs/features/network-proxy/01_ideas.md`, Technology Choices — transparent `iptables` redirection was
rejected specifically because it needs `NET_ADMIN`, which the project has rejected granting to a
sandbox on isolation grounds in three prior documents now), not a bug: nothing observes bypass traffic
today, and nothing in the mitmproxy addon (`src-tauri/resources/mitm_addon.py`) or its planned sibling
scripts (`proxy-traffic-viewer`'s `mitm_flowlog_addon.py`, `proxy-network-policy`'s
`mitm_policy_addon.py`) ever will, because none of those addons' hooks fire for a flow the proxy
process never received.

**`ProxyPanel.svelte` already states the bypass caveat as a static, non-actionable sentence.**
`src/routes/projects/ProxyPanel.svelte` renders, verbatim, "Traffic that ignores the proxy (raw
sockets, git-over-SSH) is not inspected" whenever a sandbox's proxy is running. This is the exact
sentence the deferred idea's "N connections were not inspected" framing is meant to make concrete —
the panel already has the caveat's *location*, just not a live number.

**An established, no-privilege pattern already reads a sandbox's live socket table from the host.**
`scan_listening_ports` and `parse_listening_ports` (`src-tauri/src/network/mod.rs`) run `podman exec
<sandbox> sh -c "cat /proc/net/tcp /proc/net/tcp6"` every `PORTS_FILE_POLL_INTERVAL` (2 seconds) for
every exec-capable sandbox with an open pane, and parse the kernel's `/proc/net/<proto>` text format —
a fixed-width table of hex `local_address:port`, hex `rem_address:port`, and a hex connection-state
byte — to find locally listening ports. `/proc/net/tcp{,6}` is readable by any process inside that
network namespace; no `CAP_NET_ADMIN` or elevated container privilege is involved, unlike every
transparent-interception option this project's proxy features have already rejected. The existing
parser only reads column 1 (`local_address`, for the port) and column 3 (`st`, filtering to hex `0A` =
`TCP_LISTEN`); it never decodes column 2 (`rem_address`) or the `01` = `TCP_ESTABLISHED` state, which
is exactly the pair of fields this feature needs to add.

**The proxy's own address is already known and queryable.** `proxy::status` (`src-tauri/src/proxy/
mod.rs`) returns `ProxyStatus { running, proxy_ip, network }` (`src-tauri/src/proxy/types.rs`) per
sandbox, backed by `read_meta_proxy_ip` reading the `meta.json` `create_proxy` writes at proxy-create
time. `PROXY_PORT` is a fixed constant (8080). Together these give an exact `(ip, port)` pair to
compare an observed `rem_address` against: a match is the proxy's own inbound leg (expected, proxied
traffic); anything else in `TCP_ESTABLISHED` state is, by construction, traffic that did not go through
the proxy.

**No git-over-SSH proxy configuration exists anywhere in the crate, and the SSH client's own proxy
support has no native "just point it at a SOCKS proxy" setting.** `src-tauri/resources/Containerfile`
installs `openssh-server` (for the inbound `mio-sshd` exec-transport listener,
`docs/features/network-proxy/01_ideas.md`'s Current State: "does not use the network stack at all") and
`git`, `curl`, `socat`; it does not explicitly install `openssh-client`, and nothing in the crate sets
`GIT_SSH_COMMAND`, `core.sshCommand`, or an SSH `ProxyCommand`. "git-over-SSH" in the deferred idea's
own wording names an *outbound* workload — an agent inside the sandbox running `git clone
git@host:...` — unrelated to this project's own inbound `mio-sshd`. OpenSSH's client has no
`socks_proxy` configuration directive; routing an SSH session through a SOCKS proxy requires a
`ProxyCommand` shim (commonly `nc -X 5 -x <host>:<port> %h %p`, or `connect-proxy`). `socat` (already
installed, used by `network::expose_port`'s relay-pool branch) supports SOCKS4/SOCKS4A as a client
address type but not SOCKS5; nothing installs the usual SOCKS5-capable helpers (`nc`/`ncat` built with
`--enable-nsock-socks` or `connect-proxy`).

## Proposed Model

### Scope: build only the visibility signal (a); defer SOCKS5/generic-TCP capture (b) in full

The recommendation, informed by the investigation below and the deferred idea's own framing ("better
scoped once real usage shows how often it matters"), is v1 = visibility only. The reasoning for
deferring (b) is under Technology Choices; the deferral itself is recorded in this feature's own
`04_improvements.md`, not built here.

### Bypass detection: classify each sandbox's `TCP_ESTABLISHED` rows against the proxy's known address

`src-tauri/src/proxy/bypass.rs` (new) extends the same `/proc/net/tcp`/`/proc/net/tcp6` read
`scan_listening_ports` already performs (Current State) with a second parse pass: decode `rem_address`
(hex, byte-reversed per RFC-native kernel formatting — e.g. `0100007F:1F90` is `127.0.0.1:8080`) for
every row whose state byte is `01` (`TCP_ESTABLISHED`), and classify each as *proxied* (its decoded
address:port equals the sandbox's own `(proxy_ip, PROXY_PORT)`, from `proxy::status`) or *bypassed*
(anything else). This needs no new container privilege and no new mount — it is the same `podman exec
... cat /proc/net/tcp /proc/net/tcp6` call, read a second way.

### Counting granularity: distinct bypassed remote endpoints, not raw connection count or a point-in-time gauge

A 2-second-scale poll interval cannot see a connection that opens and closes between two ticks — a
single `git clone` over SSH, for instance, may hold one connection open for the whole clone and be
caught easily, but a short-lived raw TCP probe might not be. Rather than report "connections currently
open" (a point-in-time gauge that misses everything already closed) or "total connections observed"
(noisy — ordinary HTTP keep-alive/reconnect churn from a *proxied* tool would never show here since it
matches `proxy_ip`, but a bypassing tool making many short requests to the same host would inflate a raw
count without adding new information), the poll loop accumulates a bounded, in-memory
`HashSet<(IpAddr, u16)>` of **distinct** bypassed remote endpoints observed across all ticks since the
sandbox's pane was opened. The set is capped (exact cap an Open Question below, following
`proxy-traffic-viewer/01_ideas.md`'s own precedent of capping a per-sandbox in-memory collection rather
than picking a number here) — once capped, further *distinct* endpoints stop being added but a separate
running total-sightings counter keeps incrementing, so the UI can still distinguish "a handful of hosts,
seen often" from "many different hosts."

### Lifecycle: a poll loop owned by `proxy/`, started only for proxy-enabled sandboxes

Unlike `network::watch_ports_file`, which starts for every exec-capable sandbox regardless of proxy
status, this feature's poll loop only has a signal to produce when a sandbox actually has a proxy
sidecar. `src-tauri/src/commands/panes.rs` — the existing call site for `watch_ports_file` (line 58) —
gains a paired call, gated on `proxy::exists`, starting a `proxy::bypass` poll task for that sandbox;
the task is cancelled the same way `close_sandbox` aborts `ports_watch` entries
(`src-tauri/src/network/mod.rs`). The in-memory set lives only as long as the task runs — no
persistence, no new file under `app_data_root`, no new Podman resource. This keeps the feature free of
the parallel-worktree/host-global-state costs every other proxy-subsystem addition has had to reason
about (`docs/features/network-proxy/02_dev.md`, Cross-Cutting Concerns): nothing here needs
`MIO_APP_DATA_DIR`-awareness or a new allocated subnet/port.

### Surfacing: a plain polled command, not a new Tauri event or module-scoped store

`proxy_bypass_status(name) -> ProxyBypassStatus { distinct_count: u32, total_sightings: u32, sample:
Vec<String> }` (new command, `src-tauri/src/commands/proxy.rs`), read by `ProxyPanel.svelte` the same
way it already reads `proxy_status`/`list_proxy_rules` — once per `$effect` firing, with a small
`setInterval` added to the panel's own script block so the count refreshes while the panel stays open
(mirroring nothing else in the codebase exactly, since no other panel currently self-refreshes on a
timer, but a minimal enough addition that it doesn't need a new module-scoped store or Tauri event to
justify itself — see Technology Choices).

### UI: the existing static caveat sentence becomes a live count

`ProxyPanel.svelte`'s existing line ("Traffic that ignores the proxy ... is not inspected") gains a
trailing clause sourced from `proxy_bypass_status`, in the deferred idea's own vocabulary: e.g. "3
connections were not inspected" when `distinct_count > 0`, otherwise the sentence stands unchanged.
`sample` (a handful of `"host:port"` strings, capped independently of `distinct_count`'s own cap) backs
an optional expand-for-detail affordance — left to `02_dev.md`'s frontend phase to decide whether it
ships in v1 or is itself a follow-on.

### Investigated and not recommended for v1: SOCKS5/generic-TCP capture mode (b)

mitmproxy/mitmdump does support a SOCKS5 listen mode (`--mode socks5`), and multiple `--mode` flags can
run concurrently on one `mitmdump` process (confirmed against `docs.mitmproxy.org`, fetched
2026-08-18 — see Validation), so technically `create_proxy`
(`src-tauri/src/proxy/mod.rs`) could add a second `--mode socks5@<port>` alongside the existing
`--listen-port 8080` regular mode with no new container or image. The blocker is not mitmproxy's own
capability; it is that nothing in this project's sandboxed toolchain has a wired path to *use* a SOCKS
proxy setting today:

- Curl, npm, pip, and mise's own downloader — every HTTP(S)-capable tool this project already routes
  through the proxy — already work via `http_proxy`/`https_proxy`, so a SOCKS mode adds nothing for
  them; SOCKS mode's marginal value is specifically for non-HTTP protocols.
- The flagship non-HTTP example the deferred idea itself names, git-over-SSH, has no native SOCKS
  setting in OpenSSH's client — it needs a `ProxyCommand` shim, a SOCKS5-capable helper binary not
  currently installed (Current State), and new per-sandbox SSH/git configuration (`GIT_SSH_COMMAND` or
  `core.sshCommand`) this project has never shipped for any purpose.
- No other tool in the sandbox's current toolchain (mise-installed runtimes, the agent CLIs this
  project installs) is known to expose a SOCKS-proxy-aware setting distinct from its HTTP proxy
  handling.

Building (b) for v1 would mean: a new package in `src-tauri/resources/Containerfile`, new per-sandbox
SSH/git configuration plumbing with no existing precedent, a second `--mode` and a second static
port/IP consideration in `create_proxy`, and a gating spike (mitmproxy's SOCKS mode against the pinned
`MITMPROXY_IMAGE_DEFAULT` tag has not been run) — all to serve a workload (git-over-SSH from inside a
sandbox) this investigation found no evidence is common in practice, and which the visibility signal
above will make measurable once it ships. That measurement is precisely the "real usage" signal the
deferred idea's own framing says should gate scoping this further. Recorded as a deferred improvement
in this feature's own `04_improvements.md`, not designed further here.

## Technology Choices with Tradeoffs

### Detection mechanism: poll `/proc/net/tcp{,6}` via `podman exec` (chosen) vs. `iptables`/`nftables` counters vs. promiscuous capture on the proxy container's bridge interface vs. host-side `conntrack`

Chosen: extend the existing `podman exec ... cat /proc/net/tcp /proc/net/tcp6` read
(`scan_listening_ports`) to also classify `TCP_ESTABLISHED` rows.

- **`iptables`/`nftables` connection/byte counters inside the sandbox** — rejected: needs
  `NET_ADMIN` inside the sandbox container, a capability `docs/features/tailscale-access/01_ideas.md`
  rejected first, `docs/features/network-proxy/01_ideas.md` rejected again for the interception
  mechanism itself, and `docs/features/proxy-network-policy/01_ideas.md` rejected a third time for
  enforcement — granting it here, for a feature that only wants to *count* rather than *enforce* or
  *inspect*, would be the first exception to a now well-established project-wide line.
- **Promiscuous packet capture on the `<sandbox>-proxy` container's own bridge interface** — rejected:
  a Linux bridge behaves as a learning switch, not a hub. A sibling container on the same
  `mio-proxy-<sandbox>` bridge does not see another container's unicast traffic by default, promiscuous
  mode or not, without an active man-in-the-middle mechanism (ARP spoofing, explicit port mirroring)
  this project has no reason to add for a visibility-only feature — that mechanism is a materially
  larger and shadier lift than reading a file inside the sandbox's own netns.
- **Host-side `conntrack` table inspection** — rejected: the rootless bridge's outbound NAT lives
  inside the Podman machine's own (WSL) network stack, a host-global resource this crate's process-
  runner seam (`ProcessRunner`, referenced throughout `src-tauri/AGENTS.md`) has no existing path into,
  and reading it would need either `NET_ADMIN` inside a container (rejected above) or host-side tooling
  entirely outside the crate's current `podman exec`-based seam — a much larger lift for a signal
  `/proc/net/tcp{,6}` already gives per-sandbox, with no new tooling, for free.

### Counting granularity: distinct bypassed endpoints, capped (chosen) vs. raw per-connection counter vs. point-in-time-only gauge

Chosen: an in-memory, capped `HashSet` of distinct `(remote_ip, remote_port)` pairs seen in
`TCP_ESTABLISHED` state and not matching the proxy's own address, plus a separate uncapped
total-sightings counter.

- **Raw per-connection counter (increment on every poll tick a bypassed socket is observed)** —
  rejected: over-counts a single long-lived connection once per poll tick it happens to still be open,
  and under-counts many short connections to the same host that never straddle a tick boundary; neither
  failure mode produces a number a user can reason about as "how much traffic bypassed the proxy."
- **Point-in-time gauge only (report what's `ESTABLISHED` right now, discard between polls)** —
  rejected: defeats the purpose for exactly the workloads most likely to bypass silently — a `git
  clone` over SSH that finishes in under one poll interval would never be caught, and the whole feature
  exists to catch exactly that case.

### Surfacing to the frontend: a plain polled command (chosen) vs. a poll-loop-plus-Tauri-event architecture matching `network://forwards-changed`/`proxy-traffic-viewer`'s planned flow-log event

Chosen: `proxy_bypass_status` as an ordinary `invoke`-able command, refreshed by a small `setInterval`
inside `ProxyPanel.svelte` rather than a push event and a new module-scoped store.

- **A dedicated event (`proxy://bypass-<sandbox>` or a `network://forwards-changed`-style single
  app-global event) plus a `src/lib/proxy/bypass.svelte.ts` store** — rejected for v1: this is
  proportionate machinery for a per-request live feed (what `network`'s forwards genuinely are, and
  what `proxy-traffic-viewer`'s flow log will be), not for a single slow-changing count. The deferred
  idea's own framing calls for a "lightweight" signal; a full poll-loop-plus-event stack is the kind of
  scope creep the "better scoped once real usage shows how often it matters" line is warning against.
  If `proxy-traffic-viewer`'s event pattern lands first, this feature's command could be folded into
  that event's payload later — a natural follow-up, not a blocker for either feature.

### Scope: visibility signal only (chosen) vs. building the SOCKS5 capture mode now

Chosen and reasoned in Proposed Model's "Investigated and not recommended for v1" section above; not
repeated here. The short version: mitmproxy's own SOCKS5 support is confirmed and cheap to enable, but
no sandboxed workload in this project currently has a wired path to use it, and the visibility signal
this document does build is the mechanism that will produce the "real usage" evidence the deferred
idea's own scoping advice asks for before building further.

## Validation

No spike was run for the chosen (a) design — its mechanism (`podman exec ... cat /proc/net/tcp
/proc/net/tcp6`) is not new; it is the same call `scan_listening_ports` (`src-tauri/src/network/
mod.rs`) already makes in production, reading two additional columns of an already-parsed kernel format,
which is why no gating spike is asked for before `02_dev.md`. One piece of desk research went into the
(b) investigation and its resulting recommendation to defer:

- **mitmproxy's SOCKS5 mode and simultaneous multi-mode support** (`docs.mitmproxy.org`, fetched
  2026-08-18): confirmed `mitmdump --mode socks5` is a supported mode, that repeated `--mode` flags run
  multiple proxy servers concurrently on one process (e.g. `mitmdump --mode regular --mode socks5`,
  each optionally pinned to its own port via `@<port>`), and that mitmproxy's SOCKS support is SOCKS5
  only — no SOCKS4/SOCKS4A. This confirms the technical feasibility side of (b) is not the blocker; the
  investigation's finding that no sandboxed tool has a wired consumer for it is.

## Open Questions

- **Poll interval for bypass detection.** Whether to reuse `PORTS_FILE_POLL_INTERVAL` (2 seconds,
  `src-tauri/src/network/mod.rs`) verbatim or use a longer, dedicated interval — bypass visibility is a
  slower-changing, lower-urgency signal than port exposure, so a longer interval (reducing the added
  `podman exec` load per proxy-enabled sandbox) may be preferable. Left for `02_dev.md` to pick a
  constant.
- **Cap size for the distinct-bypassed-endpoint set.** Proposed Model commits to capping the
  in-memory `HashSet` but not to a number, following `proxy-traffic-viewer/01_ideas.md`'s own precedent
  of leaving its analogous cap (`flows.ndjson`'s line count) to `02_dev.md`. Left open for the same
  reason: no data yet on typical bypass-endpoint cardinality for a real sandboxed workload.
- **`sample` field detail-view scope.** Whether the capped list of `"host:port"` strings backing a
  future expand-for-detail UI ships in v1 alongside the count, or is deferred as its own follow-on once
  the count itself proves useful. Leaning toward shipping it (the marginal backend cost is near zero —
  the set already exists in memory) but left for `02_dev.md`'s frontend phase to decide definitively.
- **IPv6 rows in practice.** `allocate_network` (`src-tauri/src/proxy/mod.rs`) only ever hands out
  IPv4 `10.90.<index>.0/24` addresses, and the bridge network has no DNS resolver
  (`docs/features/network-proxy/02_dev.md`, Spike Results), so a sandboxed tool would need a hardcoded
  IPv6 literal target to ever produce a `TCP_ESTABLISHED` row in `/proc/net/tcp6` that isn't the proxy's
  own (IPv4) address. The parser should still read `tcp6` for symmetry with `parse_listening_ports`'
  existing behavior, but whether this case is worth dedicated handling versus simply counting any
  `tcp6 ESTABLISHED` row as bypassed (since the proxy is never reached via IPv6) is a minor detail left
  for implementation.
- **Default-on for every proxy-enabled sandbox vs. its own toggle.** Unlike
  `proxy-traffic-viewer/01_ideas.md`'s equivalent open question (which stays genuinely open there
  because a flow log records host/path metadata a user might want off by default), this document leans
  toward resolving it rather than leaving it open: bypass counting exposes only a count and a bounded
  `host:port` sample of traffic that was *never* inspected — no header, no body, no secret-adjacent
  data — so the case for gating it behind a separate opt-in is weak. `02_dev.md` should confirm this
  reasoning rather than re-derive it, but is free to overturn it if a concrete reason surfaces.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The bypass count stays at zero even though a sandboxed tool is known to be making raw/non-proxy connections | The connection closed between two poll ticks and the interval (Open Questions) is too coarse to catch it, or the tool's traffic matches the proxy's own `(proxy_ip, PROXY_PORT)` by coincidence | Not necessarily a bug — this is a best-effort, poll-interval-bounded signal, not an exhaustive audit (Proposed Model, Counting granularity); shorten the poll interval in `02_dev.md`'s implementation if a specific workload needs tighter coverage |
| The bypass count includes an endpoint that is actually the proxy itself | `proxy::status`'s cached `proxy_ip` (`meta.json`, `src-tauri/src/proxy/mod.rs`) is stale relative to the running container's actual bridge IP | Confirm `read_meta_proxy_ip` reflects the current `create_proxy` run; the meta file is written once at proxy creation and never expected to drift within one sandbox's lifetime |
| The distinct-endpoint count stops growing even though new hosts are clearly being contacted | The in-memory `HashSet` hit its cap (Open Questions) — by design, further distinct endpoints stop being tracked once capped, though `total_sightings` keeps incrementing | Not a bug; raise the cap in `02_dev.md`'s implementation if a workload legitimately needs a larger working set |
| A `git clone` over SSH from inside a proxy-enabled sandbox is not inspected and still succeeds | Expected — SSH has no native proxy-env-var support and this feature does not add SOCKS5 capture (Proposed Model, "Investigated and not recommended for v1") | Not a bug; the connection should now show up in the bypass count once this feature ships, which is the visibility this document scopes, not prevention |
| `proxy_bypass_status` returns an error or empty status for a sandbox the panel shows as proxy-enabled | The bypass poll loop was never started for this sandbox's pane, e.g. it was started before `proxy::exists` began returning true, or the pane-open call site (`src-tauri/src/commands/panes.rs`) was not updated alongside `watch_ports_file`'s existing call | Confirm the poll loop start is gated on and re-checked against `proxy::exists`, mirroring how `watch_ports_file` is started for exec-capable sandboxes |
