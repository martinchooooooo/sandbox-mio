---
type: dev-plan
title: "Proxy Network Policy: Build Plan"
feature: proxy-network-policy
---
# Proxy Network Policy: Build Plan

This document is the build plan for the domain allow/deny and interactive-approval capabilities
scoped in `docs/features/proxy-network-policy/01_ideas.md`, and supersedes it wherever the two
disagree. No production code is implemented yet — every phase from B onward below is `Remaining`.
`01_ideas.md`'s gating spike was split into two independently-gating halves so the static-policy half
of this feature (Phases B–C) can ship without waiting on the interactive-approval half's riskier
assumption (Phases D–F). Phase A1 (multi-script composition, `flow.response` short-circuiting,
`server_connect`) has been run and is recorded below, with one finding flagged for a Phase B
follow-up pass. Phase A2 (approval-channel reachability) has also been run and is recorded below —
both halves cleared with no design correction needed, so Phases D–F's approval-channel assumptions
stand as written.

## The Concrete Model

A proxy-enabled sandbox with policy enforcement turned on gets two additions to the resources
`docs/features/network-proxy/02_dev.md` already creates: a second mitmproxy addon script
(`mitm_policy_addon.py`) mounted alongside the existing `mitm_addon.py`, and a policy file
(`.mio/proxy-policy.json`) mirroring `.mio/proxy-rules.json`'s workspace-definition /
app-data-materialized-copy split.

`mitm_policy_addon.py` runs three checks per request, in order: a fixed baseline block on
RFC 1918/loopback/link-local destinations (a `server_connect` hook, catching a hostname that
*resolves* to a blocked range as well as a literal blocked IP), a host-string allow/deny list read
from the materialized `/addons/policy.json` (a `request` hook, first-match-wins, falling back to a
per-sandbox `defaultAction`), and — only when the sandbox's policy mode is `"interactive"` — an
`async` fallback that calls out to a new Rust-hosted approval server instead of applying
`defaultAction`, blocking that one flow until the server answers or a timeout elapses.

The approval server (`src-tauri/src/proxy/approval.rs`) is a single, app-global `tokio::net::TcpListener`
bound to an ephemeral `127.0.0.1` port on the Windows host, reached from inside the Podman machine at
`host.containers.internal:PROXY_APPROVAL_VM_PORT` through one `forward_create_reverse` session per
Podman connection — the same primitive `wire_sandbox_to_inference` uses for `INFERENCE_VM_PORT`, not
a new one. A request identifies its sandbox by name (stamped into the materialized `policy.json`),
which the approval server resolves to a workspace path via `PodmanProvider::list_instances` →
`crate::backends::types::Sandbox.workspace_path`, the same lookup `proxy::status`/`proxy::exists`
already perform for a different reason.

## Resolved Decisions

1. **Policy-addon-to-proxy-container reachability, and multi-`-s`/`async` hook behavior**
   (`01_ideas.md` Open Question) — resolved by spike, not by design decision. Split into Phase A1
   (multi-script composition and `server_connect`, gating Phases B–C) and Phase A2 (proxy-container
   `host.containers.internal` reachability and `async def request`, gating Phases D–F). Both have now
   run (`Implemented`); Phase A1 flagged one implementation-detail correction for Phase B (see its
   section below), Phase A2 found no divergence from what this document assumed.
2. **Unconditional metadata/private-range block** (`01_ideas.md` Open Question) — stays opt-in, tied
   to policy mode `!= "off"`, as `01_ideas.md`'s Proposed Model chose. Not revisited in this document;
   `04_improvements.md` (this feature's own, not `network-proxy`'s) is where a future always-on
   variant would be recorded if usage shows the opt-in gate is routinely forgotten.
3. **Interactive-mode timeout behavior** (`01_ideas.md` Open Question) — resolved as fail-closed
   (`deny`) after a fixed 120-second timeout (`PROXY_APPROVAL_TIMEOUT_SECS` in
   `src-tauri/src/proxy/approval.rs`), no per-sandbox override and no fail-open mode in v1. The
   `ProxyPanel.svelte` interactive-mode toggle carries copy stating it suits attended sessions, not
   unattended overnight agent runs — documented as a real limitation, not solved further (a
   configurable timeout or a fail-open mode is deferred, see `04_improvements.md`).
4. **Approval-registry lifetime and app-restart behavior** (`01_ideas.md` Open Question) — resolved
   by folding it into decision 3: the pending-request registry is purely in-memory
   (`ApprovalRegistry` in `approval.rs`) and never persisted, so a request orphaned by an app restart
   is indistinguishable from one nobody has answered yet — both resolve via the same 120-second
   fail-closed timeout, with no special-cased restart detection needed.
5. **Rule ordering and specificity** (`01_ideas.md` Open Question) — v1 ships exact host-string match
   only, first-match-wins over the `rules` array in `.mio/proxy-policy.json`, no wildcard/subdomain
   or CIDR matching. This matches `mitm_addon.py`'s existing `SecretInjector` matching convention
   (`rule.get("host") != host: continue`) for consistency between the two addon scripts. Wildcard and
   CIDR rule support is deferred (`04_improvements.md`).
6. **Policy-mode default for newly created proxy-enabled sandboxes** (`01_ideas.md` Open Question) —
   resolved as `"off"` for every sandbox, new or existing, matching `network-proxy`'s own conservative
   default (`docs/features/network-proxy/02_dev.md`, Resolved Decision 3). Policy mode is not offered
   in the create-sandbox dialog at all in v1 — it is set afterward from `ProxyPanel.svelte`, since
   (unlike the proxy toggle itself) it needs no sandbox recreation.
7. **Interactive persistence direction** (new; narrows `01_ideas.md`'s Technology Choices entry of the
   same name) — only an `"allow"` decision is written back to `.mio/proxy-policy.json` as a permanent
   rule. A `"deny"` decision answers the one blocked flow and is *not* persisted, so the same host
   prompts again on a future request. `ProxyPanel.svelte`'s approval dialog does not offer a separate
   "always block" action in v1 — a permanent deny rule can still be authored by hand through the
   static rule editor (Phase C).
8. **Baseline-block IP ranges** (new; not an open question in `01_ideas.md`, settled here for
   precision) — `NetworkPolicy` in `mitm_policy_addon.py` blocks `10.0.0.0/8`, `172.16.0.0/12`,
   `192.168.0.0/16`, `127.0.0.0/8`, `::1/128`, `169.254.0.0/16`, and `fe80::/10`, hard-coded as
   module-level constants, not user-configurable and not read from `policy.json`.

## Implemented: Phase A1 — gating spike, multi-script composition (gates Phases B–C)

Run 2026-08-18 against the pinned `MITMPROXY_IMAGE_DEFAULT` tag (`mitmproxy/mitmproxy:11.1.3`,
confirmed in `src-tauri/src/proxy/mod.rs`), using throwaway `mio-spike-*` containers, all removed
after. Scripts kept at `docs/features/proxy-network-policy/spike/` (`a.py`, `b.py`,
`server_connect.py`, `server_connect_selfresolve.py`). Full results recorded in `01_ideas.md`'s
Validation section; summary:

- `mitmdump -s a.py -s b.py` loads both, and a same-named hook (`request`) defined in both fires for
  `a.py` before `b.py`, matching argument order — **confirmed**.
- Setting `flow.response` in `a.py`'s `request()` hook suppresses `b.py`'s `request()` hook's header
  mutation from reaching the client — **confirmed**. `b.py`'s hook still runs after `flow.response` is
  set (mitmproxy does not skip later scripts' hooks automatically); its mutation just has no
  client-visible effect since the upstream connection is never opened. The `flow.response is not None`
  guard Phase B adds to `SecretInjector.request()` is therefore a cleanliness/efficiency measure, not
  a leakage-prevention requirement — keep it anyway, but do not assume its absence would leak anything.
- A `server_connect` hook fires and can prevent the connection — **confirmed, mechanism is
  `data.server.error = "<reason>"`**, not `flow.kill()`. The hook signature is `server_connect(self,
  data: ServerConnectionHookData)`; it never receives a `flow` object, so `flow.kill()` was not an
  available option at all.
- A `server_connect` hook fires "with the resolved upstream address available" — **diverges from the
  assumption for a hostname target**. `data.server.address` at hook time is the unresolved
  `(hostname, port)` tuple for a hostname target; mitmproxy resolves DNS internally, later, inside its
  own connect call, with no hook in between. A naive implementation checking only whether
  `data.server.address[0]` is itself a blocked IP literal lets a hostname that *resolves* to a blocked
  range through — the connection is actually attempted (and only fails because nothing is listening) —
  exactly the gap the Common Gotchas table below already anticipated for a different reason. **A
  confirmed workaround exists**: the addon can perform its own `socket.getaddrinfo(host, port)`
  resolution inside `server_connect`, before mitmproxy's own connect attempt, and block on the result
  the same way (`data.server.error`) — see `server_connect_selfresolve.py`, verified against a hostname
  mapped to a private IP via `podman run --add-host`.

**Flag for Phase B, not resolved here**: the Proposed Model text in `01_ideas.md` ("`server_connect`
fires with the resolved upstream address already known") and this document's Phase B bullet for
`mitm_policy_addon.py`'s `server_connect` implementation both assume mitmproxy has already resolved
the hostname by hook time. That assumption is false for a hostname target. Phase B's addon
implementation needs to perform its own resolution (as `server_connect_selfresolve.py` does) rather
than trusting `data.server.address` to already be an IP — this is a concrete implementation-detail
correction, not a change to the chosen hook or overall architecture, but Phase B's bullet point should
be read with this correction in mind rather than as originally written. Not rewritten here since
redesigning Phase B is out of this spike's scope — needs a follow-up pass before Phase B is
implemented.

## Implemented: Phase A2 — gating spike, approval-channel reachability (gates Phases D–F)

Run 2026-08-18, same throwaway-resource discipline as A1, against the same pinned
`mitmproxy/mitmproxy:11.1.3` image. Scripts kept at `docs/features/proxy-network-policy/spike/`
(`host_containers_internal_from_proxy.sh`, `async_concurrency.py`, `run_async_concurrency.sh`). Full
results recorded in `01_ideas.md`'s Validation section; summary — both cleared, no divergence from
what this document's Proposed Model assumed:

- **`host.containers.internal` reachability from the *proxy* container specifically** — **confirmed**.
  A container attached only to a fresh `--disable-dns` bridge with a pinned static IP (the exact
  attachment shape `create_sidecar`'s `SidecarSpec.network` gives the real proxy container, built via
  `allocate_network`'s `--disable-dns --subnet` and `create_sidecar`'s `--network <name>:ip=<ip>`) —
  reached a `0.0.0.0`-bound listener started inside the Podman machine's own root namespace via
  `podman machine ssh`, HTTP 200. This is the reachability Phase D's approval channel needs: the proxy
  container reaching `host.containers.internal:PROXY_APPROVAL_VM_PORT`.
- **`async def request(self, flow)` hooks do not block other concurrent flows** — **confirmed**. Two
  concurrent `curl --proxy` requests through one `mitmdump` process, one held for 5s inside `await
  asyncio.sleep(5)`, the other answered immediately: the fast request completed in 10ms and returned
  before the slow one (5011ms), and the addon's own hook-entry timestamps show both hooks starting
  together with only the slow one's coroutine suspended. This is the behavior Phase E's interactive
  fallback (an `await`ed synchronous approval-server round trip) depends on — one flow blocked on an
  unanswered prompt does not stall the proxy's other concurrent flows.
- **Environment quirk, not a finding about this feature**: published container ports did not forward
  from the Podman machine through to the Windows host in this environment (confirmed independently
  with a plain `nginx` container, unrelated to mitmproxy or this addon). Worked around by driving the
  concurrency check's `curl` calls from inside the Podman machine via `podman machine ssh` rather than
  the true Windows host. Noted here in case it recurs during Phase D/E's real implementation or a
  future e2e run on this machine — no action needed for this feature's design.

Both halves Phase A2 was gating are cleared with no design correction needed — unlike Phase A1's
`server_connect` finding, Phases D–F's assumptions about this channel stand as written.

## Remaining: Phase B — policy rule storage and materialization

- `src-tauri/src/proxy/policy.rs` (new): `PolicyMode` enum (`Off`, `Static`, `Interactive`),
  `PolicyAction` enum (`Allow`, `Deny`), `PolicyRule { host: String, action: PolicyAction }`,
  `ProxyPolicy { mode: PolicyMode, default_action: PolicyAction, rules: Vec<PolicyRule> }`.
  `load_policy(workspace_path)`/`save_policy(workspace_path, &ProxyPolicy)` through `crate::mio_files`,
  reading/writing `.mio/proxy-policy.json` (mirrors `proxy::rules::load_rules`/`save_rules`, absent
  file treated as `PolicyMode::Off` with empty rules).
- `src-tauri/src/proxy/policy.rs`: `materialise(&ProxyPolicy, sandbox: &str) -> serde_json::Value` —
  no secret resolution needed (policy carries no secret material, `01_ideas.md` Technology Choices),
  just the mode/defaultAction/rules plus a `"sandbox"` field stamped from the `sandbox` argument, for
  the addon's own identity in Phase D's approval calls.
- `src-tauri/src/proxy/mod.rs`: `write_materialised_policy(app, sandbox, workspace_path)` alongside
  the existing `write_materialised_rules`, writing `<state>/policy.json`. `create_proxy` writes an
  initial empty-`Off` `policy.json` the same way it writes an empty `rules.json` today (the workspace
  does not exist yet at that point — same ordering constraint `01_ideas.md`'s Current State notes for
  `rules.json`). A new `apply_policy(app, provider, sandbox, workspace_path)` re-materializes and
  calls `restart_proxy_container`, mirroring `apply_rules`.
- `src-tauri/resources/mitm_policy_addon.py` (new): `NetworkPolicy` addon class. `__init__` loads
  `/addons/policy.json` once (same load-once model as `SecretInjector`). `server_connect(self, data)`
  enforces the baseline block (Resolved Decision 8) against the resolved upstream address. `request(self,
  flow)` enforces the host-string rules and `defaultAction` when `mode != "interactive"`; when `mode ==
  "interactive"`, an unmatched host is handled in Phase E, not here — Phase B ships with interactive
  mode accepted in the schema but behaving as `defaultAction` until Phase E lands, documented in the
  class docstring as a known interim gap.
- `src-tauri/src/proxy/mod.rs`: embed `mitm_policy_addon.py` via `include_str!`, write it to
  `<state>/policy_addon.py` at every `create_proxy` (mirrors `ADDON_SCRIPT`). `create_proxy`'s
  `SidecarSpec.mounts` gains a `SidecarMount { host_path: host_path(&policy_addon_path), dest:
  "/addons/policy_addon.py", read_only: true }` and a `SidecarMount` for the materialized
  `policy.json` at `/addons/policy.json`; `command` gains `-s /addons/policy_addon.py` inserted
  *before* the existing `-s /addons/addon.py` (Resolved Decision, `01_ideas.md` Proposed Model).
- `src-tauri/resources/mitm_addon.py`: `SecretInjector.request()` gains a guard — `if flow.response is
  not None: return` — as its first line.
- `src-tauri/src/proxy/types.rs`: no new `ProxyError` variants expected; `policy.rs` reuses the
  existing `ProxyError::CommandFailed`/`Io` shapes `rules.rs` already uses.

## Remaining: Phase C — commands and static-policy frontend

- `src-tauri/src/commands/proxy.rs`: `list_proxy_policy(workspace_path) -> ProxyPolicy` and
  `set_proxy_policy(name, workspace_path, policy: ProxyPolicy)` — the latter saves through
  `policy::save_policy`, then, if `proxy::exists(&registry, &name).await`, calls `apply_policy`, the
  same conditional-restart shape `set_proxy_rules` already has.
- `src-tauri/src/commands/mod.rs`: register `list_proxy_policy`, `set_proxy_policy` in
  `tauri::generate_handler![]`.
- `src/routes/projects/ProxyPanel.svelte`: a new policy section below the existing secret-rule editor
  — a mode selector (`off`/`static`/`interactive`; `interactive` is inert until Phase F ships its UI,
  but the mode value itself is meaningful starting here since Phase B's addon already accepts it), a
  `defaultAction` radio (`allow`/`deny`), and a rule list editor reusing the existing rule-row markup
  pattern (`host` + `action` instead of `host` + `path_prefix` + `header` + `secret_id`). Calls
  `list_proxy_policy`/`set_proxy_policy` the way the existing section calls
  `list_proxy_rules`/`set_proxy_rules`.
- `src/lib/errors.ts`: no new `AppErrorKind` expected — policy commands fail through the existing
  `ProxyError` → `BackendError` → `AppError::Backend` path.
- `messages/en.json`, `de.json`, `ru.json`: keys for the new section, deferred to after user approval
  of the working feature, per the `feature-implement` skill's i18n-last convention (matching
  `network-proxy`'s own Phase E note).

At the end of Phase C, static allow/deny policy (deferred item 1) is fully usable end to end;
interactive approval (deferred item 2, Phases D–F) is additive from here.

## Remaining: Phase D — approval channel infrastructure

- `src-tauri/src/proxy/mod.rs`: `pub const PROXY_APPROVAL_VM_PORT: u16 = 51299;` (adjacent to
  `INFERENCE_VM_PORT`'s value of 51199 in `src-tauri/src/sync/mod.rs` — a different constant in a
  different module, chosen not to collide with it) and `pub const PROXY_APPROVAL_FORWARD: &str =
  "proxy-approval";`, mirroring `inference::INFERENCE_FORWARD`'s naming.
- `src-tauri/src/proxy/approval.rs` (new): `ApprovalRequest { sandbox: String, host: String }` (the
  JSON line the addon sends), `ApprovalDecision::{Allow, Deny}` (the JSON line the server replies
  with). `pub fn ephemeral_host_port() -> Result<u16, ProxyError>` — a fresh per-launch
  `127.0.0.1:0` bind-and-release, the same pattern as `inference::ephemeral_port()`
  (`src-tauri/src/inference/mod.rs`); **not** a fixed constant, since this port is on the Windows
  host and a fixed value would collide across concurrently running app instances/worktrees (unlike
  `PROXY_APPROVAL_VM_PORT`, which lives inside the Podman machine and is safe fixed, per this skill's
  parallel-worktree rule that only a *host* port needs dynamic allocation).
- `src-tauri/src/proxy/approval.rs`: `ApprovalRegistry` — `Arc<Mutex<HashMap<Uuid,
  tokio::sync::oneshot::Sender<ApprovalDecision>>>>`, stored in Tauri `State`. `pub async fn
  run_approval_server(app: AppHandle, registry: Arc<ApprovalRegistry>, port: u16)` — accepts
  connections, reads one `ApprovalRequest` line, resolves `sandbox` to a workspace path via
  `PodmanProvider::list_instances` (`ProviderRegistry::local(ProviderId::Podman)`, the same call
  `proxy::status` makes), registers a `oneshot` receiver keyed by a generated request id, emits a
  `proxy-approval-request` Tauri event `{ request_id, sandbox, host }`, awaits the receiver with a
  120-second timeout (`tokio::time::timeout`, Resolved Decision 3), and writes the resulting
  `ApprovalDecision` back on the same connection.
- `src-tauri/src/bootstrap.rs`: on startup (gated the same way other real-infra startup steps are —
  not under `mock_backend_enabled()`, since the `light` tier never creates a real proxy container),
  bind `ephemeral_host_port()`, spawn `run_approval_server`, and call `forward_create_reverse` with
  `PROXY_APPROVAL_FORWARD`, the active Podman connection, `PROXY_APPROVAL_VM_PORT`, and the bound host
  port — lazily, on first use (the first `create_proxy` call with policy mode `"interactive"`) rather
  than unconditionally at every app launch, to avoid the Mutagen forward's overhead for users who
  never enable interactive mode. Idempotent the way `forward_create_reverse` already is.
- `src-tauri/src/commands/proxy.rs`: `respond_proxy_approval(request_id: Uuid, decision:
  ApprovalDecision, registry: State<'_, Arc<ApprovalRegistry>>)` — looks up and fires the matching
  `oneshot::Sender`; a missing id (already timed out, or a stale/duplicate click) is a silent no-op,
  not an error.
- `src-tauri/src/commands/mod.rs`: register `respond_proxy_approval`.

## Remaining: Phase E — interactive addon hook and rule persistence

- `src-tauri/resources/mitm_policy_addon.py`: `NetworkPolicy.request()` becomes `async def`. When
  `mode == "interactive"` and no static rule matches, it opens a TCP connection to
  `host.containers.internal:{PROXY_APPROVAL_VM_PORT}` (the port number is baked into the materialized
  `policy.json` by `policy::materialise`, Phase B — not hard-coded in the script, so a future port
  change needs no image/script change), sends `{"sandbox": ..., "host": ...}` as one JSON line, awaits
  one JSON line back, and applies the decision (`flow.response` on deny, pass-through on allow).
  A connection failure or read timeout (the addon's own client-side timeout, set slightly above the
  server's 120 seconds so the server's timeout always resolves first) applies `defaultAction` as a
  fallback — the addon does not hang the sandbox indefinitely if the reverse tunnel itself is down,
  only up to its own bounded wait.
- `src-tauri/src/commands/proxy.rs` or `src-tauri/src/proxy/approval.rs`: on an `Allow` decision
  (not on `Deny`, Resolved Decision 7), `respond_proxy_approval`'s handler also calls
  `policy::load_policy`/`save_policy` for the resolved workspace path, appending `PolicyRule { host,
  action: Allow }`, then — if the proxy is still running — `apply_policy` to restart the container. This
  runs after the `oneshot` fire, not before: the flow that asked already has its answer from the
  direct response line (Phase D), so the persistence-and-restart is not on that flow's critical path.

## Remaining: Phase F — approval-prompt frontend

- `src/lib/proxy/` (new directory, alongside `src/lib/workspace/`'s pattern): a small
  `.svelte.ts` store listening for the `proxy-approval-request` Tauri event (`listen()` from
  `@tauri-apps/api/event`) and holding the current pending request, if any, as `$state`.
- `src/routes/projects/ProxyApprovalDialog.svelte` (new): renders when the store holds a pending
  request, shows `sandbox`/`host`, and two buttons (Allow/Deny) calling `respond_proxy_approval` with
  the corresponding `ApprovalDecision`. Mounted once, likely in `src/routes/+layout.svelte` beside the
  existing `<Toaster />` mount, since a pending request can arrive for any sandbox regardless of which
  project route is currently open.
- `src/routes/projects/ProxyPanel.svelte`: the mode selector's `interactive` option becomes fully live
  (Phase C shipped the value, this phase makes it behave), with copy noting the 120-second fail-closed
  timeout and the unattended-session caveat (Resolved Decision 3).
- `messages/en.json`, `de.json`, `ru.json`: keys for the dialog and the interactive-mode copy,
  deferred to after user approval, same as Phase C.

## Cross-Cutting Concerns

**Parallel-worktree state.**

- `.mio/proxy-policy.json` lives in the workspace, already per-sandbox and already outside any
  shared/global directory — no new concern beyond what `.mio/proxy-rules.json` already established.
- `<app_data_root>/proxy/<sandbox>/policy.json` and `policy_addon.py` are under `state_dir`, already
  routed through `crate::paths::app_data_root` and therefore `MIO_APP_DATA_DIR`-relocatable — no new
  concern.
- `PROXY_APPROVAL_VM_PORT` (51299) is a fixed port *inside* the Podman machine, the same
  parallel-worktree-safe category `INFERENCE_VM_PORT` already established: safe because each
  worktree's e2e run can target its own Podman machine/connection (`PODMAN_CONNECTION`,
  `MIO_PODMAN_MACHINE`), so the collision domain is per-machine, not host-wide.
- The approval server's **host-side** port is the one genuine host-wide collision risk this feature
  introduces, and Phase D resolves it with `ephemeral_host_port()` rather than a fixed constant —
  called out explicitly because getting this specific one wrong (pinning it, "for consistency with
  `PROXY_APPROVAL_VM_PORT`") is the most likely mistake a future agent makes here.
- `PROXY_APPROVAL_FORWARD`'s Mutagen session name is a fixed string, not slug-scoped — matching
  `INFERENCE_FORWARD`'s existing precedent of one forward per Podman connection rather than one per
  sandbox (`01_ideas.md`, Technology Choices, "Approval channel"). Two worktrees pointed at the *same*
  Podman connection (not just the same machine name reused) would still collide on this session name;
  this is an accepted, inherited risk shared with `INFERENCE_FORWARD`, not a new one this feature
  introduces.
- No new bundled binary, no new `build.rs` asset group — `mitm_policy_addon.py` is embedded via
  `include_str!`, the same mechanism `mitm_addon.py` already uses, not a separate binary artifact.

**State derivation.** Policy mode and rules are derived from `.mio/proxy-policy.json` on disk, read
fresh on every `list_proxy_policy` call — no in-memory cache, matching `proxy::rules::load_rules`.
The addon's own in-memory copy (loaded once at container start, Phase B) is the one piece of policy
state that is *not* re-derived live; a change takes effect only after `apply_policy`'s restart, the
same restart-to-reload contract `01_ideas.md`'s Current State and this feature's Common Gotchas
(below) both call out for `rules.json`.

**Teardown.** No new teardown path: `remove_proxy` (`src-tauri/src/proxy/mod.rs`) already deletes the
whole `<app_data_root>/proxy/<sandbox>/` tree, which now additionally contains `policy.json` and
`policy_addon.py` — no code change needed there. The approval server and its reverse tunnel are
app-global, not sandbox-scoped, and are not torn down when a single sandbox is destroyed; they stop
when the app exits, the same lifecycle `wire_sandbox_to_inference`'s forward already has.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A host explicitly marked `deny` in `.mio/proxy-policy.json` still receives requests | The proxy container was not restarted after the policy change — the addon reads `/addons/policy.json` once at load | Edit policy through `set_proxy_policy` (`src-tauri/src/commands/proxy.rs`), which calls `apply_policy` and restarts the container, rather than editing `.mio/proxy-policy.json` by hand |
| A hostname that is not itself a private IP still reaches an internal service | The hostname resolves to a private/loopback/link-local address and only the `request()` host-string rule ran; the `server_connect` baseline check (Resolved Decision 8) is missing, disabled, or failed Phase A1's spike silently | Confirm `NetworkPolicy.server_connect` (`mitm_policy_addon.py`) is registered and fires — re-run the Phase A1 spike check if in doubt |
| A blocked request's response still carries an injected secret header | `-s /addons/policy_addon.py` was ordered after `-s /addons/addon.py` in `create_proxy`'s `command` vec, or `SecretInjector.request()`'s `flow.response` guard (Phase B) is missing | Confirm script order in `command` and the guard's presence as `SecretInjector.request()`'s first line |
| An interactive-mode sandbox's tool call hangs for roughly two minutes on the first request to a new host, then fails | Expected: no one answered the approval prompt before the 120-second timeout (Resolved Decision 3) | Not a bug in isolation; switch the sandbox to policy mode `"static"` with an authored rule if unattended operation is expected |
| The approval prompt never appears in the UI even though a sandbox is in `"interactive"` mode | The app-global reverse tunnel (`PROXY_APPROVAL_FORWARD`) was never created (lazy-created on first interactive-mode `create_proxy`, Phase D) or its Mutagen session died | Confirm the `proxy-approval` forward session is listed by `mutagen forward list`; Phase D's lazy-create runs again on the next interactive-mode proxy create if it is missing |
| `respond_proxy_approval` succeeds but nothing happens | The request id had already timed out server-side (120 seconds elapsed) before the user clicked; the lookup in `ApprovalRegistry` is a silent no-op for a missing id (Phase D, by design) | Not a bug; the addon already received the timeout's `defaultAction` fallback. If prompts are routinely arriving too late to answer, that is a UX issue for Phase F, not a registry bug |
| A second sandbox's approval prompt updates the wrong sandbox's `.mio/proxy-policy.json` | The materialized `policy.json`'s `"sandbox"` field was stale (written before a rename) or the approval server used a cached workspace-path lookup instead of a fresh `PodmanProvider::list_instances` call | Confirm `policy::materialise` always stamps the sandbox's *current* name and `run_approval_server`'s workspace-path resolution is not cached across requests |
| `podman create`/`mitmdump` fails to start after Phase B lands | A `SidecarMount` for `policy_addon.py` or `policy.json` is missing from `create_proxy`'s `mounts`, or its `dest` does not match the corresponding `-s`/read path | Confirm both new mounts exist and their `dest` values match `/addons/policy_addon.py` and `/addons/policy.json` exactly |
| The host-side approval server appears to work in manual testing but two worktrees' `standard` runs interfere with each other | `ephemeral_host_port()` (Phase D) was replaced with a fixed constant "for simplicity" | Never fix the host-side approval port; only `PROXY_APPROVAL_VM_PORT` (inside the Podman machine) is safe to fix — see Cross-Cutting Concerns |
