---
type: ideas
title: "Proxy Network Policy: Domain Allow/Deny and Interactive Approval"
feature: proxy-network-policy
---
# Proxy Network Policy: Domain Allow/Deny and Interactive Approval

This document covers adding two capabilities to the per-sandbox mitmproxy sidecar shipped by
`docs/features/network-proxy/` (`01_ideas.md`, `02_dev.md`): rejecting outbound requests that do
not match a host policy, and — built on the same rule storage the static policy uses — prompting
the user the first time a sandbox contacts a new host, with the answer becoming a new static rule.
Both were scoped out of `network-proxy` and recorded in
`docs/features/network-proxy/04_improvements.md` under "Domain/host allow-deny policy enforcement"
and "Per-request approval / interactive policy"; that document is not edited by this one. Nothing in
this feature is implemented. This feature covers Podman sandboxes; microsandbox sandboxes get
static egress policy from microsandbox's own rules instead
(`docs/features/microsandbox-network-policy/01_ideas.md`). When microsandbox gains interactive
approval (`docs/features/microsandbox-network-policy/04_improvements.md`), it reuses this feature's
approval channel (Phases D–F of `02_dev.md`) rather than building a second one. Relevant existing files: `src-tauri/src/proxy/mod.rs`, `rules.rs`,
`types.rs` (the proxy subsystem this feature extends), `src-tauri/resources/mitm_addon.py` (the
mitmproxy addon this feature adds a sibling script to), `src/routes/projects/ProxyPanel.svelte` (the
panel this feature adds a section to), `src-tauri/src/sync/mod.rs` (`forward_create_reverse`, the
reverse-tunnel primitive the interactive-approval channel reuses), and `src-tauri/src/provider/mod.rs`
(`PodmanProvider::list_instances`, `crate::backends::types::Sandbox`, the seam that resolves a
sandbox name to its workspace path). Read `docs/features/network-proxy/01_ideas.md` and `02_dev.md`
in full before continuing — this document assumes and does not re-explain the container/network
lifecycle, the CA injection flow, and the `.mio/proxy-rules.json`/`secrets.json` split they establish.

## Current State

**`network-proxy` is implemented**, despite `docs/features/network-proxy/02_dev.md` still marking
Phases B through E "Remaining": `src-tauri/src/proxy/mod.rs`, `rules.rs`, `types.rs`,
`src-tauri/resources/mitm_addon.py`, `src-tauri/src/commands/proxy.rs`, and
`src/routes/projects/ProxyPanel.svelte` all exist and match that document's design. This document
treats the code, not the phase table, as ground truth, and does not attempt to correct that table —
out of scope here.

**Every proxy-enabled sandbox is currently unrestricted.** `mitm_addon.py`'s only addon,
`SecretInjector`, rewrites a header on a request matching a rule; it never rejects one. A
proxy-enabled sandbox can reach any host the mitmproxy container can reach, including RFC 1918
ranges, loopback, link-local addresses, and the cloud metadata endpoint `169.254.169.254` (itself
inside the link-local range `169.254.0.0/16`) — nothing in the addon, the sandbox's `--env`
proxy configuration (`proxy_env` in `src-tauri/src/proxy/mod.rs`), or the bridge network
(`allocate_network`, same file) restricts the destination.

**The sandbox does no DNS resolution of its own.** `mio-proxy-<sandbox>` networks are created with
`--disable-dns` (`docs/features/network-proxy/02_dev.md`, Spike Results and Resolved Decision 2)
because `aardvark-dns` cannot start on this project's podman machine. A proxy-enabled sandbox has no
resolver at all; every hostname it ever resolves is resolved by mitmproxy itself, lazily, when it
opens the upstream connection for a proxied request. This means the DNS-rebinding attack surface
`01_ideas.md`'s Validation section describes for microsandbox — a sandbox's own DNS query returning
one IP, a later connection landing on another — does not have the same shape here: there is no
sandbox-side DNS query to rebind. The trust boundary is entirely inside the proxy container, which is
this project's own code, not a third party's.

**No IPC path exists for a proxy container to reach the app synchronously.** Every existing seam that
crosses the container boundary is app-initiated (`podman exec`, `MachineProvider::exec_piped`,
`provider.pull_image`) or file-based (mitmdump's confdir bind mount). The one prior-art pattern for
"a container reaches a process outside itself" runs in the opposite direction, host-to-container is not
what is needed: `forward_create_reverse` (`src-tauri/src/sync/mod.rs`, line 149) opens a Mutagen
session that listens `0.0.0.0` inside the Podman machine and dials a Windows-host loopback port,
which is how `wire_sandbox_to_inference` (`docs/features/local-llm/02_dev.md`, Phase G) lets a
container reach a process on the Windows host via `host.containers.internal`. No Rust code today
hosts a server on that dialled-to port for a container to call *into* — `reqwest` (already a
dependency, `src-tauri/Cargo.toml`) is used only as an HTTP client. No `axum`/`warp`/`hyper` server
framework is a dependency; `tokio` is (`features = ["full"]`), so `tokio::net::TcpListener` is
available with no new dependency.

**No per-sandbox app-side registry maps a sandbox name to its workspace path.** `set_proxy_rules` and
every other `proxy::*` command (`src-tauri/src/commands/proxy.rs`) take `workspace_path` as an
explicit argument from the frontend, which already has it (`Project.workspace_path`, a frontend-only
concept — no Rust `Project` struct exists). The one backend-side path from a sandbox *name* to its
workspace path is `crate::backends::types::Sandbox.workspace_path`, populated in
`PodmanProvider::list_instances` (`src-tauri/src/provider/mod.rs`, line 351) from the container's
`WORKSPACE_LABEL`, with a name-derived fallback (`crate::backends::workspace_path`,
`src-tauri/src/backends/mod.rs`) for containers created before that label existed. `proxy::exists`
and `proxy::status` (`src-tauri/src/proxy/mod.rs`) already call `list_instances` for a different
reason (finding the `<sandbox>-proxy` container); this is the seam a backend-initiated component
(one with no frontend-supplied `workspace_path` to hand it) needs to resolve one for itself.

## Proposed Model

### Three enforcement layers, in order: a fixed private/metadata block, static host rules, and an interactive fallback

The two deferred items compose into one addon-side policy pipeline, evaluated per request in this
order:

1. **Baseline block (fixed, not user-authored).** RFC 1918 ranges (`10.0.0.0/8`, `172.16.0.0/12`,
   `192.168.0.0/16`), loopback (`127.0.0.0/8`, `::1`), and link-local (`169.254.0.0/16`, `fe80::/10`
   — the metadata endpoint `169.254.169.254` is a member of this range, not a separate case) are
   never reachable through the proxy, regardless of any rule. This is the "block the metadata
   endpoint outright" half of the deferred item, generalized to the whole class it belongs to.
2. **Static host rules.** A per-sandbox ordered list of `{ host, action: "allow" | "deny" }` entries,
   plus a `defaultAction` (`"allow"` or `"deny"`) for a host matching no rule. This is the "reject
   requests to hosts not on an allowlist" half.
3. **Interactive fallback.** When enabled, a host matching no static rule is not resolved by
   `defaultAction` at all — the addon asks the app, blocking that one flow until an answer arrives
   (or a timeout expires), and the answer is written back as a new static rule (layer 2), so the same
   host never prompts again. This is the deferred "per-request approval" item, expressed as an
   alternate resolution for exactly the case layer 2's `defaultAction` would otherwise handle — the
   shared seam the two deferred items were always going to need.

A sandbox's policy has one of three modes, stored in `.mio/proxy-policy.json` alongside
`.mio/proxy-rules.json`: `"off"` (today's behavior — no layer 2 or 3, and, chosen below, no layer 1
either), `"static"` (layers 1 and 2), `"interactive"` (layers 1, 2, and 3).

### Rule storage: a new `.mio/proxy-policy.json`, materialized the same way `.mio/proxy-rules.json` is

`.mio/proxy-policy.json` (`src-tauri/src/proxy/policy.rs`, new, `mio_files`-backed like
`proxy::rules::load_rules`/`save_rules`) holds `{ mode, defaultAction, rules: [{ host, action }] }`.
Unlike `proxy-rules.json`, this file holds no secret material — a policy rule is a host and an
allow/deny verdict, safe to read from inside the sandbox's own bind-mounted workspace. It still gets
a materialized copy under `<app_data_root>/proxy/<sandbox>/policy.json`, mounted read-only into the
proxy container at `/addons/policy.json`, mirroring `rules.json`'s split — not for secrecy this time,
but so the addon's reload story matches the existing one exactly: edit through a command, restart the
container, exactly like `set_proxy_rules`/`apply_rules` today. The materialized copy also carries the
sandbox's own name (`"sandbox": "<name>"`), which the addon needs to identify itself when it calls
the approval channel (Interactive Approval Channel, below) — nothing else gives the addon process its
own sandbox identity, since `create_sidecar` (`src-tauri/src/provider/types.rs`,
`MachineProvider::create_sidecar`) has no `env` field to stamp one in as a container-wide variable.

### A second mitmproxy addon script, loaded before the existing one

`src-tauri/resources/mitm_policy_addon.py` (new) is embedded via `include_str!` in
`src-tauri/src/proxy/mod.rs`, written to `<state>/policy_addon.py` at every `create_proxy` (same
refresh-on-upgrade approach as the existing `ADDON_SCRIPT`), and mounted read-only at
`/addons/policy_addon.py`. `create_proxy`'s `command` vec gains a second `-s
/addons/policy_addon.py`, placed *before* the existing `-s /addons/addon.py` — mitmdump loads
multiple `-s` scripts and calls same-named hooks across all of them in argument order, so the policy
script's `request()` hook runs, and can set `flow.response` to short-circuit the flow, before
`SecretInjector.request()` gets a chance to inject a real credential into a request that is about to
be blocked. `SecretInjector.request()` (`mitm_addon.py`) gains a guard — skip if `flow.response` is
already set — so a blocked flow's synthetic response is never overwritten. Layer 1 (the baseline
block) runs in a `server_connect` hook, not `request()`: mitmproxy resolves the upstream host's IP to
open that connection, and `server_connect` fires with the resolved address already known
(`flow.server_conn.address` conceptually — the addon's actual attribute name is confirmed during
implementation, not asserted here), which is the only point in the request lifecycle where a
hostname that *resolves* to a blocked range, not just a hostname that *is* one, can be caught. Layer
2's host-string rules run in `request()`, where `flow.request.pretty_host` and `flow.request.path`
are already used by `SecretInjector`.

### Interactive approval channel: one app-global reverse tunnel, reused per Podman connection, not per sandbox

The addon's interactive-fallback path needs to call something outside its own container and get a
synchronous answer. The channel: a new Rust-hosted TCP server (`src-tauri/src/proxy/approval.rs`,
new), a minimal JSON-line request/response protocol (no HTTP framework — one endpoint does not
justify a new dependency when `tokio::net::TcpListener` already ships with the existing `tokio`
dependency), bound `127.0.0.1:<port>` on the Windows host. `forward_create_reverse`
(`src-tauri/src/sync/mod.rs`) is reused unmodified to make it reachable as
`host.containers.internal:<PROXY_APPROVAL_VM_PORT>` from inside the Podman machine — the same
function, same `0.0.0.0`-bind requirement, `wire_sandbox_to_inference` already uses for
`INFERENCE_VM_PORT`. One reverse tunnel per Podman connection (mirroring `INFERENCE_VM_PORT`'s
app-global scope, not a per-sandbox scope) is sufficient: `host.containers.internal` resolves
identically from every bridge network on the same machine, so every proxy container on that machine
reaches the same listener regardless of which sandbox it belongs to; the request payload (host,
sandbox name from the materialized `policy.json`) is what disambiguates, not the port. The addon's
policy-addon `request()` hook, on a layer-3 fallback, opens a connection to
`host.containers.internal:<PROXY_APPROVAL_VM_PORT>`, sends `{ sandbox, host }`, and awaits a response
— an `async def request(self, flow)` hook, mitmproxy's documented mechanism for a hook to await a
coroutine without blocking other flows on the same proxy process.

### Approval resolution: an in-memory pending-request registry bridges the TCP server and a new Tauri command

The Rust-side TCP server, on receiving `{ sandbox, host }`, resolves `sandbox` to a workspace path via
`PodmanProvider::list_instances` → `crate::backends::types::Sandbox.workspace_path` (Current State),
registers a pending decision (a new in-memory registry keyed by a generated request id, analogous in
shape to `activity::TaskRegistry` (`src-tauri/src/activity/mod.rs`) but holding a
`tokio::sync::oneshot::Sender<Decision>` per entry rather than task progress), and emits a Tauri event
(`app.emit`) so the frontend can show the prompt. A new command, `respond_proxy_approval(request_id,
decision)` (`src-tauri/src/commands/proxy.rs`), resolves the matching `oneshot::Sender`, which
unblocks the TCP server's still-open connection to the addon, which returns the decision as the
response body. Separately, and not on the synchronous path, the Rust side appends `{ host, action:
decision }` to that sandbox's `.mio/proxy-policy.json` (through the same `policy::save_rules` layer 2
uses) so a future request to the same host resolves at layer 2, and — if the addon is still running —
restarts the proxy container (`restart_proxy_container`, `src-tauri/src/proxy/mod.rs`) so its
in-memory rule set catches up; the *current* flow does not wait for the restart, since it already has
its answer from the direct response body.

### Frontend: a policy section in the existing `ProxyPanel.svelte`, not a new panel

`src/routes/projects/ProxyPanel.svelte` gains a mode selector (`off`/`static`/`interactive`), a
`defaultAction` toggle, and a rule list editor matching the existing secret-rule list's row pattern —
`set_proxy_policy(name, workspacePath, policy)` (new command) saves and, if the proxy is running,
re-materializes and restarts, the same flow `set_proxy_rules` already follows. A new toast/dialog
component (design deferred to `02_dev.md`'s frontend phase) renders on the `proxy-approval-request`
event and calls `respond_proxy_approval`. Unlike the base proxy toggle itself (baked into `podman
create`, requires recreating the sandbox to flip — `docs/features/network-proxy/02_dev.md`,
Cross-Cutting Concerns), policy mode and rules are pure proxy-container-side configuration and take
effect on a live sandbox with only a proxy-container restart, no sandbox recreation.

## Technology Choices with Tradeoffs

### Enforcement point: mitmproxy addon hooks (chosen) vs. `iptables`/`nftables` in the proxy container vs. a DNS-level blocklist

Chosen: `server_connect` for the baseline IP-range block, `request` for host-string rules and the
interactive fallback — both hooks the addon already has full request context in, on the same process
that already terminates every intercepted request.

- **`iptables`/`nftables` rules inside the proxy container** — rejected: it would be the first
  resource in this feature line to need `NET_ADMIN` on any container, a capability
  `docs/features/network-proxy/01_ideas.md`'s Technology Choices already rejected granting to the
  *sandbox* on isolation grounds (`docs/features/tailscale-access/01_ideas.md` rejected it first, for
  a different feature); granting it to the *proxy* container is a smaller blast radius but still a new
  capability this project has avoided everywhere else, for a mechanism (IP/port filtering) coarser
  than what mitmproxy's own request view already gives for free (path prefixes, not just hosts).
- **A DNS-level blocklist, answering `NXDOMAIN` for denied domains** — rejected: it re-introduces
  exactly the dependency `docs/features/network-proxy/02_dev.md`'s Spike Results found broken on this
  project's podman machine (`aardvark-dns` cannot start — "Failed to connect to user scope bus via
  local transport"), and a proxy-enabled sandbox already has no resolver of its own (Current State) —
  there is no DNS query on the sandbox side for a DNS server to intercept.

### Baseline block scope: gated behind policy mode `!= "off"` (chosen) vs. always on whenever the proxy is enabled at all

Chosen: the private-range/metadata block is part of the same opt-in as the rest of this feature — a
sandbox created before this feature shipped, or one left at policy mode `"off"`, keeps `network-proxy`
v1's unrestricted behavior exactly.

- **Always-on regardless of policy mode** — considered, not chosen: forcing the block onto every
  already-shipped proxy-enabled sandbox is a silent behavior change for anyone whose workflow
  legitimately reaches a private IP through the proxy (an internal registry, a VPN-routed service),
  and this project's established pattern for anything that narrows a sandbox's reachable surface is
  explicit opt-in (`docs/features/network-proxy/02_dev.md`, Resolved Decision 3, for the proxy itself).
  The narrower, higher-value version of this option — block unconditionally, independent of the
  broader policy toggle — is recorded as an Open Question below rather than decided either way here.

### Rule storage: a new `.mio/proxy-policy.json` (chosen) vs. folding policy rules into the existing `.mio/proxy-rules.json`

Chosen: a sibling file with its own schema (`{ host, action }` vs. `proxy-rules.json`'s `{ host,
path_prefix, header, secret_id }`).

- **One combined file** — rejected: the two rule kinds have different failure semantics — a secret
  rule is purely additive (a missing match just leaves a header untouched, `rules::materialise`'s
  `filter_map` already documents this) while a policy rule can *block* a request outright — and
  conflating them would force the addon (or a shared Rust materialization path) to reason about both
  schemas together for no shared benefit; the two are read by two different addon scripts already
  (Proposed Model).

### Approval channel: reuse `forward_create_reverse` with an app-global port (chosen) vs. a per-sandbox reverse tunnel vs. file-based polling

Chosen: one reverse tunnel per Podman connection, a fixed `PROXY_APPROVAL_VM_PORT` constant
(`src-tauri/src/proxy/mod.rs`, alongside `PROXY_PORT`), mirroring `INFERENCE_VM_PORT`'s scope
(`src-tauri/src/sync/mod.rs`).

- **A per-sandbox reverse tunnel** — rejected: every proxy container's `0.0.0.0` bind for this
  purpose would live inside the *same* shared Podman machine regardless of which sandbox it serves,
  so per-sandbox ports would need their own allocation and bookkeeping (an analog of `allocate_network`'s
  subnet walk) for no reachability benefit — `host.containers.internal` already resolves identically
  from every bridge network on the machine, proven for the sandbox side in
  `docs/features/network-proxy/02_dev.md`'s Spike Results and assumed, not yet spiked, for the proxy
  side (Validation).
- **The addon writes a "pending decision" file, and the app polls for it** — rejected: it trades an
  extra poll-interval's worth of prompt latency for no simplicity win, since the addon still needs its
  own poll loop to notice an answered file, exactly the complexity a direct, awaited TCP call avoids;
  mitmproxy's `async def` hook support (Proposed Model) makes the direct call the same amount of code
  as the polling variant's addon-side half, with none of its host-side half.

### Interactive persistence: only "allow" decisions promoted to a permanent rule by default (chosen) vs. persisting every decision including "deny"

Chosen: an approved host becomes a permanent static-rule entry (Proposed Model); a denied prompt is
answered for the flow that asked but is not written into `.mio/proxy-policy.json` unless the UI offers
a distinct "always block this host" action.

- **Persist every decision, allow or deny** — considered, not chosen as the only behavior: a single
  "not right now" click is not necessarily "never" the way a single "yes" click plausibly does mean
  "this host is fine going forward" for a credential-shaped one-time integration; the cost of getting
  this wrong differs by direction — an under-persisted deny just prompts again, while an
  over-persisted deny that should have been contextual silently and permanently narrows a sandbox's
  reachable set. The UI phase (`02_dev.md`) can still offer both a session-only and a permanent deny
  action; this document does not foreclose that, only the default.

## Validation

**Phase A1 spike run 2026-08-18** (`docs/features/proxy-network-policy/02_dev.md`, Phase A1;
scripts kept at `docs/features/proxy-network-policy/spike/`), against the pinned
`mitmproxy/mitmproxy:11.1.3` image. Covered the multi-script/`flow.response`/`server_connect` half of
the two carried-over assumptions below; the `host.containers.internal`-from-the-proxy-container and
`async def` hooks half (Phase A2) is a separate spike, not yet run.

- **`mitmdump -s a.py -s b.py` hook order** — cleared. A same-named hook (`request`) defined in both
  scripts fires for `a.py` before `b.py`, matching `-s` argument order, confirmed via `podman logs`
  timestamps.
- **`flow.response` short-circuiting a later script's mutation** — cleared. Setting `flow.response` in
  `a.py`'s `request()` suppressed `b.py`'s unguarded header mutation from ever reaching the client (a
  blocked path returned only `a.py`'s synthetic 403 body/headers); the same mutation, run on an
  unblocked path, did reach the real upstream, confirming `b.py`'s hook does something real when not
  suppressed. One nuance: `b.py`'s `request()` hook still *executes* after `flow.response` is set (it
  is not skipped automatically) — it just has no client-visible effect, since mitmproxy never opens the
  upstream connection once `flow.response` is set. The `mitm_addon.py` guard
  (`02_dev.md` Phase B: `if flow.response is not None: return`) is therefore about avoiding wasted work
  and touching request state after a block, not a correctness requirement to prevent leakage — but is
  still worth keeping for that reason.
- **`server_connect` fires and can prevent the connection — mechanism confirmed** — cleared, mechanism
  is `data.server.error = "<reason>"`, not `flow.kill()`. The hook signature itself is
  `server_connect(self, data: ServerConnectionHookData)` — it does not receive a `flow` object at all,
  so `flow.kill()` was never available here regardless. Setting `data.server.error` inside the hook
  produces a 502 response to the client carrying that string, confirmed for a literal private IP
  target (`10.255.255.1`).
- **`server_connect` fires with the resolved upstream address available — diverges from the
  assumption.** For a literal IP target, `data.server.address` is trivially "resolved" (it already is
  the IP). For a hostname target, `data.server.address` at hook time is still the **unresolved
  `(hostname, port)` tuple** — confirmed both by reading `mitmproxy/proxy/server.py`'s
  `open_connection` (the `ServerConnectHook` fires before the `asyncio.open_connection(...)` call that
  performs DNS resolution internally) and empirically: a naive `server_connect` addon that only checks
  whether `data.server.address[0]` parses as an IP literal let a hostname resolving to `10.255.255.2`
  through, and mitmproxy proceeded to actually attempt (and fail with `ECONNREFUSED`) a real connection
  to that private IP before any block could apply. **Workaround confirmed**: the addon can do its own
  resolution inside `server_connect` (`socket.getaddrinfo(host, port)`) before mitmproxy's own connect
  attempt, and block on the result via `data.server.error` — verified working for a hostname mapped to
  a private IP via the test container's `/etc/hosts` (`podman run --add-host`). See
  `docs/features/proxy-network-policy/spike/server_connect_selfresolve.py`. This does not change the
  choice of hook (`server_connect` is still the only point before the real connection attempt), but it
  does mean `mitm_policy_addon.py`'s baseline-block implementation (`02_dev.md` Phase B) must perform
  its own DNS resolution rather than trusting mitmproxy to have already resolved the address by hook
  time, as `02_dev.md`'s Proposed Model text currently assumes ("`server_connect` fires with the
  resolved upstream address already known"). **Flagged for a Phase B follow-up pass — not corrected in
  `02_dev.md`'s Phase B section by this spike**, per this spike's own scope.

**Phase A2 spike run 2026-08-18** (`docs/features/proxy-network-policy/02_dev.md`, Phase A2; scripts
kept at `docs/features/proxy-network-policy/spike/` — `host_containers_internal_from_proxy.sh`,
`async_concurrency.py`, `run_async_concurrency.sh`), against the same pinned
`mitmproxy/mitmproxy:11.1.3` image, using throwaway `mio-spike-*` containers/networks and a
throwaway listener started inside the podman machine via `podman machine ssh`, all removed after.
Covers the two assumptions left open by Phase A1:

- **`host.containers.internal` reachability from the *proxy* container specifically** — cleared. A
  container attached only to a fresh `--disable-dns` bridge network (`mio-spike-proxynet`) with a
  pinned static IP (`10.91.0.2`, via `--network mio-spike-proxynet:ip=10.91.0.2`) — the same
  attachment shape `create_sidecar`'s `SidecarSpec.network` gives the real proxy container, not the
  generic bridge-only container `network-proxy/02_dev.md`'s own spike used for the *sandbox* side —
  reached `host.containers.internal:18080`, a `python3 -m http.server --bind 0.0.0.0` listener
  started inside the podman machine's own root namespace (`podman machine ssh`, not a container),
  and got HTTP 200. (The mitmproxy image has no `curl`; the in-container client used was `python3
  -c "urllib.request.urlopen(...)"` — a client-tool substitution, not a change to what was tested.)
  This directly confirms the reachability Phases D–F's approval channel needs: the proxy container
  reaching `host.containers.internal:PROXY_APPROVAL_VM_PORT` for the Rust-hosted approval server.
- **`async def request(self, flow)` hooks do not block other concurrent flows** — cleared. An addon
  (`async_concurrency.py`) with `async def request` awaits `asyncio.sleep(5)` for a request to
  `slow.mio-spike.test` and returns immediately for `fast.mio-spike.test`. Two `curl --proxy`
  requests fired concurrently at the same `mitmdump` process: the fast request completed in 10ms
  and its `wait` returned first, while the slow request took 5011ms. The addon's own `HOOKLOG` lines
  confirm both hooks started at the same relative timestamp (`t=8.136`) and the fast one's response
  reached its client immediately, while the slow one didn't resume until `t=13.136` (5s later) — the
  awaited sleep suspends only that one flow's coroutine, not the process. This directly confirms the
  behavior Phase E's `async def request` fallback (a synchronous approval-server round trip) depends
  on: one sandbox waiting on an unanswered approval prompt does not stall every other sandbox's
  traffic through a *different* mitmdump process, nor — more precisely, since this spike used one
  `mitmdump` process serving two flows — a single sandbox's other concurrent requests through the
  *same* proxy.
- **Environment note, not a finding about mitmproxy or this feature**: published container ports
  (`podman create -p`) did not forward from the podman machine through to the Windows host in this
  environment (confirmed independently with a plain `nginx` container — reachable from inside the
  podman machine's own namespace, connection-refused from the Windows host). Worked around by
  driving the `curl --proxy` calls from inside the podman machine (`podman machine ssh`) rather than
  the true Windows host for the async-concurrency check only; `host.containers.internal` reachability
  (the first bullet) was tested container-to-machine-namespace directly and is unaffected. Flagged
  here only in case a future spike or e2e run hits the same host-port-forwarding quirk on this
  machine.

Both assumptions Phase A2 was gating are now cleared with no divergence from what `02_dev.md`'s
Proposed Model assumed for either mechanism — unlike Phase A1's `server_connect` finding, nothing
here requires a design correction to Phases D–F.

## Open Questions

- **Policy-addon-to-proxy-container reachability, and multi-`-s`/`async` hook behavior** — the gating
  spike above; both halves (Phase A1: multi-script composition/`server_connect`; Phase A2:
  `host.containers.internal`-from-proxy-container/`async def` hook concurrency) have now run and
  cleared against the pinned mitmproxy image, with one implementation-detail correction from Phase A1
  (`server_connect`'s self-resolution requirement, flagged for a Phase B follow-up pass) and no
  corrections from Phase A2.
- **Unconditional metadata/private-range block.** Should the baseline layer-1 block (Proposed Model)
  eventually apply to every proxy-enabled sandbox regardless of policy mode, given how narrow and
  broadly uncontroversial it is compared to host-level allow/deny — or does it stay opt-in
  indefinitely, tied to policy mode, as chosen above? Left open because it is a defensible call either
  way and affects every sandbox created before this feature shipped, not just new ones.
- **Interactive-mode timeout behavior for an unattended sandbox.** This project's primary workload is
  an AI coding agent running with no human watching. A synchronous per-host prompt that fails closed
  (deny) after a timeout could silently stall or fail a long unattended run at the first new host it
  ever contacts; failing open (allow) after a timeout defeats the point of asking at all for exactly
  the unattended case where a human is least likely to be present to say no. Whether interactive mode
  should default to fail-closed, fail-open, or refuse to be enabled at all on a sandbox not actively
  attached to a live terminal/agent-web session is undecided, and is a product decision as much as a
  technical one.
- **Approval-registry lifetime and app-restart behavior.** The in-memory pending-request registry
  (Proposed Model, Approval Resolution) does not survive an app restart. A request in flight when the
  app restarts — or when it was never running at all, e.g. a sandbox's agent working overnight with
  the desktop app closed — has no answerer. Whether that case should fail closed immediately (no
  retry) rather than hold the addon's connection open indefinitely is undecided; it interacts directly
  with the timeout question above.
- **Rule ordering and specificity within static rules.** `{ host, action }` as scoped assumes exact
  host match. Whether a rule can match a subdomain wildcard or an IP literal/CIDR (not just the
  exact string `flow.request.pretty_host` returns), and how a rule list with both an `allow` and a
  `deny` entry for the same host resolves (first match, most specific match, deny-wins), is undecided.
- **Policy-mode default for newly created proxy-enabled sandboxes.** Whether a sandbox created with
  the proxy toggle on also defaults to policy mode `"off"` (matching `network-proxy`'s own
  Resolved Decision 3 default-off stance) or `"static"` with an empty allow-list and `defaultAction:
  "allow"` (a no-op that primes the UI for a user who is about to add rules) is undecided.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A host explicitly marked `deny` in `.mio/proxy-policy.json` still receives requests | The proxy container was not restarted after the policy change — the addon reads `/addons/policy.json` once at load, same as `rules.json` (`docs/features/network-proxy/02_dev.md`, Resolved Decision 11) | Edit policy through `set_proxy_policy` (`src-tauri/src/commands/proxy.rs`), which re-materializes and restarts the container, rather than editing `.mio/proxy-policy.json` by hand |
| A hostname that is not itself a private IP still reaches an internal service | The hostname resolves to a private/loopback/link-local address and only the layer-2 host-string rule ran; the layer-1 `server_connect` IP-range check (Proposed Model) is missing or was skipped | Confirm the baseline block runs in a `server_connect` hook, which fires after DNS resolution, not only in `request()`, which only sees the literal Host string |
| A blocked request's response still carries an injected secret header | `mitm_policy_addon.py`'s `-s` flag was ordered after `mitm_addon.py`'s in `create_proxy`'s `command` vec, or `SecretInjector.request()` was not given the `flow.response` guard | Load the policy script first (`-s /addons/policy_addon.py` before `-s /addons/addon.py`) and confirm `SecretInjector.request()` returns immediately when `flow.response` is already set |
| An interactive-mode sandbox's tool call hangs for a long time on the first request to a new host | Expected by design when a human is not present to answer the approval prompt — see Open Questions, "Interactive-mode timeout behavior" | Not a bug in isolation; if it is undesirable for a given sandbox's workflow, use policy mode `"static"` with an authored rule list instead of `"interactive"` |
| The approval prompt never appears in the UI even though a sandbox is in `"interactive"` mode | The app-global reverse tunnel (`forward_create_reverse` call for `PROXY_APPROVAL_VM_PORT`) was never created, or its Mutagen session died | Confirm the reverse-tunnel session is up the same way `docs/features/local-llm/02_dev.md`'s Phase G forwards are checked; recreate it the way `wire_sandbox_to_inference` does |
| A request to a raw IP address, or traffic from a tool that ignores `http_proxy`/`https_proxy`, reaches a denied host anyway | Non-proxy-aware traffic bypasses mitmproxy entirely via the bridge network's own NAT — an inherited, not new, gap (`docs/features/network-proxy/02_dev.md`, Resolved Decision 7) | Expected and documented, not a bug; this feature only ever sees traffic that actually flows through the proxy |
| `podman create`/`mitmdump` fails to start after adding the second addon script | The new `SidecarMount` for `policy_addon.py` was not added alongside the existing `addon.py` mount in `create_proxy` (`src-tauri/src/proxy/mod.rs`), or the mounted path does not match the `-s` argument | Confirm both `-s` paths in `command` have a matching `SidecarMount.dest` entry in `mounts` |
| A second sandbox's approval prompt updates the wrong sandbox's `.mio/proxy-policy.json` | The materialized `policy.json`'s `"sandbox"` field was stale (written before a rename) or missing, and the approval server used it to resolve the workspace path | Confirm `create_proxy`/policy materialization always stamps the sandbox's current name, and that `respond_proxy_approval`'s workspace-path resolution goes through `PodmanProvider::list_instances` (Current State), not a cached value |
