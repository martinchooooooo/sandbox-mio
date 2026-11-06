---
type: dev-plan
title: "Proxy Non-HTTP Capture: Build Plan"
feature: proxy-non-http-capture
---
# Proxy Non-HTTP Capture: Build Plan

This document is the build plan for the visibility-only slice scoped in
`docs/features/proxy-non-http-capture/01_ideas.md` — counting, per proxy-enabled sandbox, distinct
outbound connections that bypassed the `network-proxy` mitmproxy sidecar
(`docs/features/network-proxy/02_dev.md`) — and supersedes `01_ideas.md` wherever the two disagree.
Nothing is implemented yet; every phase below is `Remaining`. No gating spike was required: the
mechanism reuses `scan_listening_ports`/`parse_listening_ports`'s already-proven
`podman exec <sandbox> cat /proc/net/tcp /proc/net/tcp6` read (`src-tauri/src/network/mod.rs`), parsing
two columns that existing parser does not, so there is no unconfirmed assumption left to spike before
committing to phases. SOCKS5/generic-TCP capture (the deferred idea's other named direction) is not
part of this plan — see `docs/features/proxy-non-http-capture/04_improvements.md`.

## The Concrete Model

A proxy-enabled sandbox's poll loop reads `/proc/net/tcp`/`/proc/net/tcp6` from inside the sandbox
every `BYPASS_POLL_INTERVAL`, classifies every `TCP_ESTABLISHED` row by comparing its decoded remote
`(IpAddr, u16)` against the sandbox's own proxy address (from `proxy::status`'s `proxy_ip` and the
fixed `PROXY_PORT`), and accumulates the non-matching ones — bypassed connections — into a capped,
in-memory, per-sandbox set. No new Podman resource, host port, or on-disk file is introduced anywhere
in this feature; the entire accumulated state lives in one Tauri-managed struct,
`proxy::bypass::BypassRegistry`, reset whenever the app process restarts.

The loop starts the first time the frontend opens a sandbox's pane (mirroring `watch_ports_file`'s own
trigger, `watch_workspace_ports` in `src-tauri/src/commands/panes.rs`, invoked from
`src/routes/projects/SandboxItem.svelte`), is a no-op for a sandbox with no proxy sidecar, and stops —
along with its accumulated count — when the sandbox is undeployed or destroyed, through the same shared
teardown point `network::close_sandbox` already runs through
(`close_panes_and_forwards` in `src-tauri/src/services/sandbox.rs`).

`ProxyPanel.svelte`'s existing static caveat sentence ("Traffic that ignores the proxy ... is not
inspected") gains a live count sourced from a new `proxy_bypass_status` command, polled the same way
the panel already polls `proxy_status`/`list_proxy_rules`, plus a lightweight client-side timer so the
count keeps refreshing while the panel stays open.

## Resolved Decisions

1. **Poll interval for bypass detection** (`01_ideas.md` Open Question 1) — a dedicated constant,
   `BYPASS_POLL_INTERVAL = 5s` (`src-tauri/src/proxy/bypass.rs`), longer than
   `PORTS_FILE_POLL_INTERVAL`'s 2 seconds (`src-tauri/src/network/mod.rs`): bypass visibility is a
   lower-urgency signal than port exposure, and the longer interval halves the added `podman exec`
   load per proxy-enabled sandbox relative to reusing the shorter one.
2. **Cap size for the distinct-bypassed-endpoint set** (`01_ideas.md` Open Question 2) — a single
   constant, `BYPASS_CAP = 32` distinct `(IpAddr, u16)` entries per sandbox. Chosen as a round number
   comfortably larger than the number of distinct external hosts a typical sandboxed workflow (an
   agent's package downloads, API calls, `git` remotes) is expected to contact — revisit upward if real
   usage shows it binding.
3. **`sample` field detail-view scope** (`01_ideas.md` Open Question 3) — ships in v1, and needs no
   cap of its own: `sample` is the capped distinct set itself, formatted as `"ip:port"` strings, so
   Resolved Decision 2's cap already bounds it. This also removes the need for a decision on a separate
   sample cap, which `01_ideas.md` had left open alongside it.
4. **IPv6 rows in practice** (`01_ideas.md` Open Question 4) — parsed unconditionally, with no special
   case: `parse_established_bypass` (Phase A) decodes both `/proc/net/tcp` and `/proc/net/tcp6` rows
   into the same typed `(IpAddr, u16)` shape, and an `IpAddr::V6` value can never equal the sandbox's
   `IpAddr::V4` proxy address under `PartialEq`, so every `tcp6 TCP_ESTABLISHED` row is correctly
   classified as bypassed by the same comparison that handles IPv4, with no `is_ipv6` branch needed in
   the parser.
5. **Default-on vs. its own toggle** (`01_ideas.md` Open Question 5, resolved there already, restated
   here as settled) — automatic for every proxy-enabled sandbox, no separate opt-in command or setting.
   `watch_proxy_bypass` (Phase B) is invoked unconditionally alongside `watch_workspace_ports` and is a
   no-op for a sandbox with no proxy sidecar.

## Remaining: Phase A — bypass detection core

- `src-tauri/src/proxy/bypass.rs` (new): declared as `pub mod bypass;` in `src-tauri/src/proxy/mod.rs`
  alongside the existing `pub mod rules;` / `pub mod types;`.
- `BYPASS_POLL_INTERVAL: std::time::Duration` and `BYPASS_CAP: usize` constants (Resolved Decisions 1
  and 2).
- `BypassAccumulator` (private): `distinct: std::collections::HashSet<(std::net::IpAddr, u16)>`,
  `total_sightings: u64`. `total_sightings` increments by the number of bypassed rows observed *per
  tick*, not per distinct endpoint — it is a coarse activity counter, not a connection count (see
  `01_ideas.md`, Counting granularity), and callers must not read it as "N connections."
- `BypassRegistry` (`pub`, `#[derive(Default)]`): `accumulators: std::sync::Mutex<HashMap<String,
  BypassAccumulator>>`, `tasks: std::sync::Mutex<HashMap<String, tokio::task::JoinHandle<()>>>`. Managed
  by Tauri the same way `TaskRegistry`/`WorkspaceRegistry`/`MachineSpecsCache` already are
  (`app.manage(...)` calls in `src-tauri/src/bootstrap.rs`, `register_state`) — Phase B.
- `fn parse_established_bypass(output: &str, proxy_addr: (std::net::IpAddr, u16)) ->
  Vec<(std::net::IpAddr, u16)>`: parses `/proc/net/tcp`/`/proc/net/tcp6` text (column 2, `rem_address`,
  format `<hex addr>:<hex port>`; column 3, `st`, filtering to hex `01` = `TCP_ESTABLISHED`), decodes
  the hex address (IPv4: 4 bytes, byte-reversed from the kernel's little-endian 32-bit word; IPv6: 16
  bytes as four little-endian 32-bit words, each word's 4 bytes reversed independently, words kept in
  original order — the same kernel-native encoding `parse_listening_ports`, in the same file, already
  parses one column of), and returns every decoded `(ip, port)` pair not equal to `proxy_addr`. A pure
  function — cover it with a `#[cfg(test)]` unit test using a captured `/proc/net/tcp` sample line
  (e.g. one row for the proxy's own address, one for a bypassed remote) before wiring it to a live
  `podman exec`, since the codebase has no other bypass-parsing precedent to lean on for correctness.
- `async fn scan_established_bypass(provider: &Arc<dyn MachineProvider>, sandbox: &str, proxy_addr:
  (std::net::IpAddr, u16)) -> Vec<(std::net::IpAddr, u16)>`: best-effort, mirroring
  `scan_listening_ports`'s own shape and its exact argv (`sh -c "cat /proc/net/tcp /proc/net/tcp6
  2>/dev/null"` via `provider.exec`, `ExecOpts::default()`) — any exec failure (container stopped,
  `cat` missing) yields an empty `Vec` rather than propagating an error, so a scan hiccup never kills
  the poll loop.
- `fn record_tick(registry: &BypassRegistry, sandbox: &str, bypassed: Vec<(std::net::IpAddr, u16)>)`:
  locks `accumulators`, inserts new distinct entries up to `BYPASS_CAP` (further distinct entries are
  dropped once capped, not evicted-and-replaced — Resolved Decision 2 assumes cap sizing headroom
  rather than an LRU policy), and adds `bypassed.len() as u64` to `total_sightings` unconditionally
  (uncapped, per the field's definition above).
- `async fn poll_bypass(app: tauri::AppHandle, provider: Arc<dyn MachineProvider>, sandbox: String,
  proxy_addr: (std::net::IpAddr, u16))`: `loop { sleep(BYPASS_POLL_INTERVAL); let bypassed =
  scan_established_bypass(&provider, &sandbox, proxy_addr).await; record_tick(&app.state::<
  BypassRegistry>(), &sandbox, bypassed); }` — re-resolves the managed `BypassRegistry` from
  `app` each tick rather than capturing a borrowed reference, since the task must outlive the command
  handler that spawned it (mirrors how every other `AppHandle`-carrying background task in this crate,
  e.g. `proxy::create_proxy`'s own callers, is threaded through).
- `pub fn start_watch(registry: &BypassRegistry, app: tauri::AppHandle, provider: Arc<dyn
  MachineProvider>, sandbox: String, proxy_ip: String)`: parses `proxy_ip` into `std::net::IpAddr`
  (a malformed or empty `proxy_ip` — the sandbox has no running proxy — is a silent no-op, matching
  Resolved Decision 5's "unconditional call, no-op when not applicable" shape), builds `proxy_addr =
  (ip, PROXY_PORT)` (`PROXY_PORT` from `crate::proxy`), and idempotently inserts a spawned
  `poll_bypass` task into `registry.tasks` under a lock — a second call for an already-watched sandbox
  is a no-op, mirroring `watch_ports_file`'s own `if watches.contains_key(sandbox) { return; }` guard.
- `pub fn stop_watch(registry: &BypassRegistry, sandbox: &str)`: aborts and removes the task from
  `registry.tasks`, then removes the sandbox's entry from `registry.accumulators` — both best-effort,
  no error if the sandbox was never watched (a never-proxied sandbox's teardown calls this
  unconditionally, Phase B).
- `pub fn status(registry: &BypassRegistry, sandbox: &str) -> types::ProxyBypassStatus`: reads
  `accumulators.get(sandbox)` (a sandbox with no entry yet — never watched, or watched but no tick has
  landed — returns the zero-valued default), formats each distinct `(ip, port)` as `"ip:port"` for
  `sample`.

## Remaining: Phase B — types and command wiring

- `src-tauri/src/proxy/types.rs`: add `ProxyBypassStatus { distinct_count: u32, total_sightings: u64,
  sample: Vec<String> }`, `#[derive(Debug, Clone, Default, serde::Serialize)]`, `#[serde(rename_all =
  "camelCase")]` — matching `ProxyStatus`'s own derive/rename shape in the same file.
- `src-tauri/src/bootstrap.rs`, `register_state`: `app.manage(crate::proxy::bypass::BypassRegistry::
  default());`, alongside the existing `app.manage(TaskRegistry::default());` /
  `app.manage(WorkspaceRegistry::default());` / `app.manage(MachineSpecsCache::default());` lines.
- `src-tauri/src/commands/proxy.rs`: two new commands.
  - `watch_proxy_bypass(name: String, registry: State<'_, Arc<ProviderRegistry>>, bypass: State<'_,
    proxy::bypass::BypassRegistry>, app: tauri::AppHandle) -> Result<(), AppError>`: returns `Ok(())`
    immediately if `!proxy::exists(&registry, &name).await`; otherwise reads `proxy::status` for
    `proxy_ip`, and — if non-empty — calls `proxy::bypass::start_watch` with
    `registry.local(ProviderId::Podman)`.
  - `proxy_bypass_status(name: String, bypass: State<'_, proxy::bypass::BypassRegistry>) ->
    Result<ProxyBypassStatus, AppError>`: infallible in practice, wraps
    `Ok(proxy::bypass::status(&bypass, &name))`.
- `src-tauri/src/commands/mod.rs`: add `proxy::watch_proxy_bypass` and `proxy::proxy_bypass_status` to
  the `tauri::generate_handler![]` inventory — both commands are unreachable from the frontend without
  this (`AGENTS.md`'s own Common Gotchas table already names this exact failure mode for any new
  command).
- `src-tauri/src/services/sandbox.rs`: `close_panes_and_forwards` (the function `undeploy` and
  `destroy` both call to close panes/forwards before touching the SSH channel they run over) gains
  `crate::proxy::bypass::stop_watch(&app.state::<crate::proxy::bypass::BypassRegistry>(), name);`
  immediately after its existing `network.close_sandbox(name).await;` line. No new function parameter
  is needed — `close_panes_and_forwards` already receives `app: &tauri::AppHandle`, and
  `AppHandle::state::<T>()` resolves the same Tauri-managed singleton `bootstrap.rs` registered in
  Phase B.

## Remaining: Phase C — frontend

- `src/routes/projects/SandboxItem.svelte` (near the existing `invoke("watch_workspace_ports", {
  name: project.name, workspacePath: project.workspace_path })` call, line 73): add a paired
  `invoke("watch_proxy_bypass", { name: project.name })`, fire-and-forget (no `await`ed error handling
  needed — the command no-ops server-side for a non-proxied sandbox and is not expected to fail for a
  proxied one).
- `src/routes/projects/ProxyPanel.svelte`: add a `ProxyBypassStatus` TypeScript interface mirroring
  `proxy::types::ProxyBypassStatus` (camelCase, matching the existing `ProxyStatus` interface's own
  comment convention in this file), a `bypass = $state<ProxyBypassStatus | null>(null)`, and fold a
  `proxy_bypass_status` call into the existing `refresh()` function. Add a client-side interval
  (`setInterval`, a local constant mirroring `BYPASS_POLL_INTERVAL`'s value, e.g. `5000`) that calls
  `refresh()` while `status?.running` is true, cleared in the `$effect`'s cleanup return (Svelte 5's
  effect-cleanup convention — the existing `$effect` block at the bottom of the file gains a `return`
  closing over the interval handle). Update the existing static line — "Traffic that ignores the proxy
  (raw sockets, git-over-SSH) is not inspected." — to append the live count when
  `bypass.distinctCount > 0`, e.g. rendering an additional line "`{bypass.distinctCount} connections
  were not inspected`" (the deferred idea's own wording,
  `docs/features/network-proxy/04_improvements.md`) directly beneath it.
- No `messages/*.json` keys are added in this phase. Per the root `AGENTS.md` Translations section and
  `feature-implement`'s own convention (already followed by every `<!-- TODO(i18n) -->`-marked string
  in `ProxyPanel.svelte` today), the i18n pass runs after the user has approved the working feature —
  new copy added here should carry the same `<!-- TODO(i18n) -->` marker the file's existing strings
  use, not new translation keys.

## Cross-Cutting Concerns

**No new host-global state.** Unlike every prior addition to the `proxy` subsystem
(`docs/features/network-proxy/02_dev.md` and its sibling planning docs each introduce a new Podman
network, container, bind-mounted file, or host port), this feature introduces none of those: the entire
accumulated signal lives in one Tauri-managed in-memory struct (`BypassRegistry`, Phase A), keyed by
sandbox name — already the run-scoped identifier every e2e tier treats correctly — and touches no path
under `app_data_dir()`, no `podman network`/`podman create`, and no `e2e/lib/run-identity.ts` port
allocation. This is the first `proxy`-subsystem feature with no parallel-worktree cost to reason about
beyond the sandbox-name keying every other feature in this area already relies on.

**Mock-backend (`light` tier) coverage.** `scan_established_bypass` (Phase A) issues the exact same
`podman exec` argv `scan_listening_ports` already issues in production
(`src-tauri/src/network/mod.rs`), which the `light` e2e tier (`MIO_MOCK_BACKEND=1`) already exercises
successfully for that existing caller. Confirm during Phase A implementation whether the mock
`ProcessRunner`'s handling of that argv (an unregistered, free-form `exec`, not a `podman.<verb>`-keyed
call routed through `mockctl`'s `KNOWN_CALLS`/`derive_podman_key`) needs any new registration for this
feature's own call site — if `scan_listening_ports`'s existing `light`-tier coverage needed none, this
feature's identical call needs none either.

**State derivation and lifetime.** Nothing here is persisted anywhere. `BypassRegistry`'s state exists
only for the running app process's lifetime and is reset to empty on every app restart — a sandbox that
had accumulated a nonzero bypass count before a restart starts over at zero the next time its pane is
opened and `watch_proxy_bypass` is called again. This is a deliberate simplification consistent with
the feature's "lightweight signal, not an audit trail" framing (`01_ideas.md`), not an oversight.

**No teardown-ordering hazard.** `proxy::bypass::stop_watch` (Phase A/B) operates on `BypassRegistry`
alone, a resource entirely independent of `NetworkManager`'s own `forwards`/`ports_watch` maps and of
`proxy::remove_proxy`'s container/network/state-directory teardown
(`docs/features/network-proxy/02_dev.md`, Teardown ordering). Unlike `remove_proxy`, which must remove
the proxy container before its network, there is no ordering constraint between `stop_watch` and any
other teardown step — it can run anywhere in `close_panes_and_forwards`, before or after
`network.close_sandbox`, with no observable difference.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `proxy_bypass_status` always returns zero counts even though the sandbox is proxy-enabled and a bypassing tool is running | `watch_proxy_bypass` was never invoked for this sandbox (the frontend call site in `src/routes/projects/SandboxItem.svelte` was not added alongside `watch_workspace_ports`), or it no-op'd because `proxy::status`'s `proxy_ip` was still empty at call time | Confirm the paired `invoke("watch_proxy_bypass", ...)` call exists next to `watch_workspace_ports`'s, and that it runs after the sandbox's proxy sidecar has finished creating (`proxy::status.running` true) |
| The distinct-endpoint count stalls at 32 even though new hosts are clearly being contacted | `BYPASS_CAP` (Resolved Decision 2) was reached; `record_tick` (`src-tauri/src/proxy/bypass.rs`) deliberately drops further distinct entries once capped rather than evicting old ones | Not a bug; raise `BYPASS_CAP` if a specific workflow needs a larger working set — `total_sightings` keeps incrementing regardless of the cap, so activity is still visible even once the distinct set is full |
| The bypass count resets to zero after the app restarts, even though the sandbox itself was untouched | `BypassRegistry` (Phase A) is in-memory only and Tauri-managed state does not survive a process restart | Expected — see Cross-Cutting Concerns, State derivation and lifetime; this feature deliberately has no persistence |
| A connection to the proxy's own address is counted as bypassed | `proxy_addr` was built from a stale or mismatched `proxy_ip` (e.g. `watch_proxy_bypass` was called before `proxy::create_proxy` finished writing `meta.json`) | Confirm `proxy::status`'s `proxy_ip` is non-empty and correct before calling `start_watch`; `watch_proxy_bypass` (Phase B) already guards on a non-empty `proxy_ip`, so re-check the guard rather than the parser if this occurs |
| A unit test for `parse_established_bypass` fails to decode a captured `/proc/net/tcp6` line correctly | IPv6 addresses in `/proc/net/<proto>` are encoded as four little-endian 32-bit words, each word's own 4 bytes reversed independently but the four words kept in original order — a full 16-byte reversal (treating the address as one big-endian block) produces the wrong address | Decode in 4-byte groups, each group byte-reversed on its own, per Phase A's parser note; do not reverse the full 16-byte sequence at once |
| Two `watch_proxy_bypass` calls for the same sandbox (e.g. the pane is reopened) double the poll rate or spawn a second task | `start_watch`'s idempotency check (`registry.tasks.contains_key`) was bypassed or removed | Confirm `start_watch` (`src-tauri/src/proxy/bypass.rs`) still guards on `tasks` before spawning, mirroring `watch_ports_file`'s own guard in `src-tauri/src/network/mod.rs` |
