---
type: dev-plan
title: "Proxy Remote Deploy: Build Plan"
feature: proxy-remote-deploy
---
# Proxy Remote Deploy: Build Plan

This document is the build plan for letting a proxy-enabled sandbox survive `CliBackend::deploy`
(`src-tauri/src/backends/mod.rs`), scoped in `docs/features/proxy-remote-deploy/01_ideas.md`, and
supersedes it wherever the two disagree. Nothing is implemented yet — every phase below is
`Remaining`. Unlike most build plans in this repository, the final phase (Phase E, wiring the new
machinery into `deploy` and lifting `BackendError::ProxyDeployUnsupported`) is explicitly blocked:
`01_ideas.md`'s Validation section could not run the live two-machine spike its core risk (rootless
bridge-network behaviour on a genuine non-WSL remote Podman host) requires, and this document does
not fabricate one. Phases A through D build real, independently useful groundwork — new trait
methods, new `proxy/` functions, new command routing — that changes no runtime behaviour of `deploy`
by itself; Phase E is the single integration point that makes a proxy-enabled deploy actually happen,
and a future agent must not start it before that spike has run and cleared.

## The Concrete Model

`CliBackend::deploy` gains a target-side proxy path that mirrors the sequencing
`services::sandbox::create` already uses locally: ensure the mitmproxy image on the target
connection, create the proxy sidecar there, create the sandbox container with the resulting
`ProxyAttachment`'s env/network, start it, then inject the freshly generated remote CA. The
low-level `proxy/` functions this reuses (`create_proxy`, `inject_ca`, `apply_rules`,
`restart_proxy_container`) already take a `provider: &Arc<dyn MachineProvider>` parameter rather than
resolving one internally, so no signature change is needed there — `deploy` simply passes the
`target_provider` it already builds for the sandbox container itself
(`crate::provider::default_provider(self.runner.clone(), target_connection.to_string())`).

What genuinely does not exist yet is a remote-side counterpart to the *file-backed* half of the
proxy's state. `state_dir`/`certs_dir`/`ca_pem_path` (`src-tauri/src/proxy/mod.rs`) resolve every
path under `crate::paths::app_data_root(app)` — a directory on the machine running the desktop app,
which for a remote deploy target is not the same machine `podman create`'s bind mounts resolve
against (`docs/features/remote-sync/01_ideas.md`'s foundational fact). A new family of `_remote`
functions in `proxy/mod.rs` gives the proxy sidecar's `addon.py`, materialised `rules.json`, and
`certs/` directory a home on the *target* host's own filesystem — `<home>/mio-proxy/<sandbox>/`,
directly analogous to `resolve_workspace`'s `<home>/work/<name>` for the sandbox workspace itself —
created and populated over a raw SSH command/pipe, the same transport shape `remote_mkdir`
(`src-tauri/src/backends/mod.rs`) already uses for the workspace directory. The small,
non-secret `meta.json` cache (network name, proxy IP) stays on the laptop under the existing local
`state_dir`, since nothing about it needs to live on the remote host and keeping it local lets
`status()` answer without an SSH round trip.

Rule *definitions* (`.mio/proxy-rules.json`) need no new transport at all — they already live inside
the sandbox's own workspace, which the existing Mutagen sync session mirrors to the remote once
deployed. Only the *materialised* `rules.json` (real secret values) is deploy-specific: it is
generated fresh from the laptop's `secrets.json` and written directly into the remote proxy's state
directory at deploy time and at every subsequent rule change, mirroring `write_materialised_rules`'s
local behaviour but through the new remote-write path — never by transferring the raw `secrets.json`
store itself.

The local proxy sidecar is **stopped, not removed**, by `deploy` — the same treatment `deploy`
already gives the local sandbox container itself ("kept stopped, not removed, so undeploy can
restart it"). This is load-bearing: the local sandbox container's `--network mio-proxy-<sandbox>`
attachment is fixed at `podman create` time, and deleting that network out from under a stopped
container that still names it would strand `undeploy`'s later `store.start(name)`.

Commands that act on an already-deployed sandbox's proxy (`proxy_status`, `set_proxy_rules`,
`restart_proxy`) need to know, at call time, whether that sandbox's authoritative proxy is currently
local or on a deploy target — the same question `SandboxRouter` (`src-tauri/src/services/routing.rs`)
already answers for every other transport-sensitive command. A new `src-tauri/src/services/proxy.rs`
resolves a route via `SandboxRouter::locate`, and — when the route names a non-local Podman
connection — resolves that connection's SSH endpoint through a new `SandboxBackend::connection_info`
trait method, so the command layer never has to know the difference between a local and a
deploy-target proxy beyond which function variant (`proxy::apply_rules` vs.
`proxy::apply_rules_remote`, etc.) to call.

Undeploying a proxy-enabled sandbox removes the remote proxy sidecar entirely (container, network,
remote state directory — mirroring how `store.remove_remote` already removes the remote sandbox
container) and restarts the local proxy sidecar `deploy` left stopped, so the sandbox returns to
exactly the locally-proxied state it was in before deploy. Destroying a still-deployed,
proxy-enabled sandbox removes both the local and the remote proxy sidecar, since nothing survives
the sandbox's own destruction on either side.

## Resolved Decisions

1. **Whether to lift `ProxyDeployUnsupported` before the live spike clears** (`01_ideas.md` Open
   Question) — not lifted by this document. Phases A–D build every piece of new machinery with zero
   change to `deploy`'s observable behaviour; Phase E is the sole phase that removes the refusal and
   wires the machinery in, and is explicitly blocked on the live two-machine spike
   `01_ideas.md`'s Validation section could not run.
2. **Undeploy symmetry for the local proxy** (`01_ideas.md` Open Question) — resolved yes:
   `services::sandbox::undeploy` restarts the local `<sandbox>-proxy` container (a new
   `proxy::start_proxy_if_present`, Phase D) immediately after `store.start(name)`, mirroring the
   existing symmetry between `deploy` stopping the local sandbox container and `undeploy` restarting
   it.
3. **The remote proxy's fate on undeploy** (`01_ideas.md` Open Question) — resolved: removed
   entirely. `undeploy` calls the new `proxy::remove_proxy_remote` (Phase B) against the target
   connection before `store.remove_remote` runs, mirroring `store.remove_remote`'s own removal of the
   remote sandbox container — nothing about a proxy-enabled sandbox is left behind on a connection the
   sandbox no longer runs on.
4. **`services::sandbox::destroy`'s missing proxy cleanup on the deployed-sandbox branch**
   (`01_ideas.md` Open Question) — resolved as an in-scope fix, not deferred: Phase D adds calls to
   remove both the local (stopped) and remote proxy sidecar in that branch, ordered alongside the
   existing `store.remove_remote`/`store.destroy` calls (see Phase D for exact ordering).
5. **Progress reporting for a remote proxy image pull** (`01_ideas.md` Open Question) — resolved:
   no new mechanism. `ensure_proxy_image` (`src-tauri/src/proxy/mod.rs`) already takes a generic
   `provider` parameter and already reports through `activity::TaskRegistry`; calling it with the
   target-pinned provider during deploy gets progress reporting for free. Whether this should be
   unified with `docs/features/remote-deploys/01_ideas.md`'s own open question about base-image build
   progress reporting stays open — that document owns the decision, not this one.
6. **Directional dependency between `proxy/` and `backends/`** (new; not posed in `01_ideas.md` but
   forced by Phase A/B's design) — resolved: `proxy/` does not gain a dependency on any `backends/`
   *trait* or function, preserving the one-directional `backends → proxy` dependency that already
   exists (`backends/mod.rs` already calls `crate::proxy::exists`, etc.). It does reuse the plain
   `ConnectionInfo` struct (`src-tauri/src/backends/types.rs`, widened from `pub(super)` to
   `pub(crate)` in Phase A) as a data type — justified by the same precedent
   `docs/features/proxy-credential-secrets/02_dev.md` Resolved Decision 5 already established for
   `proxy/` calling `credentials/`: a plain-data or pure-function reference is not the kind of
   dependency the "subsystems never depend on each other" rule (`src-tauri/AGENTS.md`) is protecting
   against — only a dependency on another subsystem's *trait* (breaking independent mockability) is.
   The SSH command-construction logic itself (argv building, spawning) is duplicated into `proxy/mod.rs`
   rather than calling `backends::remote_mkdir` or its siblings, since those are free functions in a
   module `proxy/` must not depend on.
7. **How a command resolves which connection a sandbox's proxy operations should currently address**
   (new) — resolved: a new `src-tauri/src/services/proxy.rs` orchestration module, using
   `SandboxRouter::locate` plus the new `SandboxBackend::connection_info` trait method (Phase A/C).
8. **`sweep`'s scope across connections** (new) — resolved: stays local-only, exactly as it is
   implemented today (`crate::proxy::sweep`, `src-tauri/src/proxy/mod.rs`, called from
   `src-tauri/src/bootstrap.rs` against `registry.local(ProviderId::Podman)` only). Remote proxy
   cleanup is handled entirely by `deploy`/`undeploy`/`destroy`'s explicit lifecycle calls (Resolved
   Decisions 2–4), not by a periodic sweep across every registered connection — enumerating every
   connection on every app startup is an unbounded, expensive operation this feature does not need.
   A cross-connection sweep remains a possible future improvement (`04_improvements.md`) if orphaned
   remote proxy resources turn out to accumulate in practice (e.g. from a crash mid-deploy).

## Remaining: Phase A — Cross-module plumbing

- `src-tauri/src/backends/types.rs`: widen `ConnectionInfo` from `pub(super)` to `pub(crate)`. Add
  `async fn connection_info(&self, connection: &str) -> Result<Option<ConnectionInfo>, BackendError>`
  to the `SandboxBackend` trait, doc-commented as returning `None` when `connection` names no
  registered connection or is a local Podman machine with no SSH endpoint (the same case
  `parse_connection_entry` already returns `None` for).
- `src-tauri/src/backends/mod.rs`: `CliBackend::connection_info` implements the trait method by
  delegating to the existing `Self::remote_connection_info(&self.runner, connection)` — no change to
  that function itself, only a new public entry point onto it.
- `src-tauri/src/backends/mock.rs`: mock `SandboxBackend::connection_info` returns `Ok(None)`
  unconditionally. The light e2e tier never registers a real remote connection, matching
  `list_hosts`'s existing always-empty mock behaviour (`docs/features/cloud-sandboxes/01_ideas.md`
  Common Gotchas) — this feature does not change that gap, only avoids making it worse.

## Remaining: Phase B — Remote-state primitives in `proxy/mod.rs`

- `remote_home(user: &str) -> String` — `/root` for `root`, else `/home/<user>`; a deliberate,
  small duplication of the equivalent inline match in `resolve_workspace`
  (`src-tauri/src/backends/mod.rs`), kept local to `proxy/` per Resolved Decision 6.
- `remote_state_root(sandbox: &str, target: &ConnectionInfo) -> String` —
  `format!("{}/mio-proxy/{sandbox}", remote_home(&target.user))`.
- `ssh_argv(target: &ConnectionInfo) -> Vec<String>` (or an equivalent private builder) — the shared
  `-o BatchMode=yes -o StrictHostKeyChecking=accept-new -i <identity> -p <port> <user>@<host>` prefix,
  matching `remote_mkdir`'s existing flags exactly so remote proxy operations fail the same way (host
  key changes, unreachable host) that remote workspace operations already do.
- `remote_mkdir(target: &ConnectionInfo, dir: &str) -> Result<(), ProxyError>` — `ssh ... mkdir -p
  '<dir>'`, structurally identical to `backends::remote_mkdir` but returning `ProxyError`.
- `remote_write_file(target: &ConnectionInfo, path: &str, contents: &[u8]) -> Result<(), ProxyError>`
  — spawns `ssh ... sh -c "cat > '<path>'"` with stdin piped, writes `contents`, and awaits the
  child, the same spawn-then-write-stdin-then-wait shape `PodmanProvider::exec_piped`
  (`src-tauri/src/provider/mod.rs`) already uses for a different transport.
- `remote_read_file(target: &ConnectionInfo, path: &str) -> Result<Vec<u8>, ProxyError>` — `ssh ...
  cat '<path>'`, captured stdout; a non-zero exit (file absent) maps to `ProxyError::CommandFailed`.
- `remote_rm_rf(target: &ConnectionInfo, dir: &str) -> Result<(), ProxyError>` — `ssh ... rm -rf
  '<dir>'`, called only from `remove_proxy_remote`, best-effort exactly like local `remove_proxy`'s
  final state-directory deletion.
- `wait_for_ca_remote(target: &ConnectionInfo, sandbox: &str) -> Result<(), ProxyError>` — polls with
  the same attempt count/interval constants `wait_for_ca` uses, but each attempt runs `ssh ... test -f
  '<remote_state_root>/certs/mitmproxy-ca-cert.pem'` instead of a local `Path::exists` check, to avoid
  transferring the file itself on every poll.
- `create_proxy_remote(app: &AppHandle, provider: &Arc<dyn MachineProvider>, sandbox: &str, target:
  &ConnectionInfo) -> Result<ProxyAttachment, ProxyError>` — network allocation is unchanged
  (`allocate_network`, already connection-generic through `provider`); creates the remote state root
  and its `certs/` subdirectory via `remote_mkdir`; writes the embedded `ADDON_SCRIPT` and an empty
  `rules.json` array via `remote_write_file`; builds the same `SidecarSpec` shape `create_proxy` does,
  with `SidecarMount.host_path` set to the remote paths instead of `certs_dir`/`addon_path`/
  `rules_path`; calls `provider.create_sidecar`/`provider.start` unchanged; calls
  `wait_for_ca_remote` in place of `wait_for_ca`; still calls the existing (local) `write_meta` so the
  non-secret network name/IP cache stays laptop-side, additionally recording a `"location":
  "remote"` field and the target connection name so `status()` can report where a sandbox's proxy
  currently lives.
- `read_ca_pem_remote(target: &ConnectionInfo, sandbox: &str) -> Result<Vec<u8>, ProxyError>` —
  `remote_read_file` at `<remote_state_root>/certs/mitmproxy-ca-cert.pem`.
- `write_materialised_rules_remote(app: &AppHandle, sandbox: &str, workspace_path: &str, target:
  &ConnectionInfo) -> Result<(), ProxyError>` — reuses `rules::load_rules`/`rules::materialise`
  unchanged, writing the result via `remote_write_file` to `<remote_state_root>/rules.json` instead of
  `std::fs::write`.
- `apply_rules_remote(app: &AppHandle, provider: &Arc<dyn MachineProvider>, sandbox: &str,
  workspace_path: &str, target: &ConnectionInfo) -> Result<(), ProxyError>` — calls
  `write_materialised_rules_remote` then `restart_proxy_container(provider, sandbox)` (unchanged —
  restarting the container is already connection-generic through `provider`).
- `remove_proxy_remote(provider: &Arc<dyn MachineProvider>, sandbox: &str, target: &ConnectionInfo)`
  — mirrors `remove_proxy`'s best-effort container-then-network-then-state-directory order, using
  `remote_rm_rf` for the last step; logs warnings the same way, never propagates an error.
- `stop_proxy(provider: &Arc<dyn MachineProvider>, sandbox: &str) -> Result<(), ProxyError>` — `
  provider.stop(&proxy_container_name(sandbox))`, a one-line addition used by `deploy` (Phase E)
  against the local provider.
- `start_proxy_if_present(provider: &Arc<dyn MachineProvider>, sandbox: &str) -> Result<(), ProxyError>`
  — `if exists(provider, sandbox).await { provider.start(&proxy_container_name(sandbox)).await?; }`,
  used by `undeploy` (Phase D, Resolved Decision 2).
- `exists`/`status` (existing functions) are widened to accept `provider: &Arc<dyn MachineProvider>`
  directly instead of `&Arc<ProviderRegistry>`, dropping their internal `registry.local(ProviderId::
  Podman)` resolution. Every existing call site (`backends/mod.rs::deploy`'s guard,
  `services/sandbox.rs`, `commands/proxy.rs`) is updated to resolve a provider first and pass it in —
  `deploy`'s own guard explicitly resolves `registry.local(ProviderId::Podman)` (it is asking "is the
  sandbox about to be deployed currently proxy-enabled locally," which is inherently a local-only
  question), while `commands/proxy.rs`'s commands resolve theirs through the new `services/proxy.rs`
  (Phase C). `sweep` keeps its existing `&Arc<ProviderRegistry>` signature and local-only scope
  (Resolved Decision 8).

## Remaining: Phase C — `services/proxy.rs` and command routing

- `src-tauri/src/services/proxy.rs` (new): `pub async fn resolve(router: &SandboxRouter, store:
  &Arc<dyn SandboxBackend>, registry: &Arc<ProviderRegistry>, name: &str) -> Result<(Arc<dyn
  MachineProvider>, Option<crate::backends::types::ConnectionInfo>), AppError>` — resolves a
  `SandboxRoute` via `router.locate(name)`, resolves `provider = registry.provider_for(&route.machine)`,
  and — only when `route.machine.provider == ProviderId::Podman` and `route.machine.id` differs from
  `registry.local_ref(ProviderId::Podman).id` — additionally calls `store.connection_info(&route.machine.id)`
  to produce `Some(ConnectionInfo)`; otherwise returns `(provider, None)`. Thin wrappers
  `apply_rules`, `restart`, `status` branch on the `Option` to call the local or `_remote` `proxy::`
  variant.
- `src-tauri/src/commands/proxy.rs`: `proxy_status`, `set_proxy_rules`, `restart_proxy` gain a
  `router: State<'_, SandboxRouter>` and `store: State<'_, Arc<dyn SandboxBackend>>` parameter
  (matching the existing pattern in `src-tauri/src/commands/panes.rs`/`network.rs`) and delegate to
  the new `services::proxy` wrappers instead of calling `proxy::` directly against
  `registry.local(ProviderId::Podman)`.
- `set_proxy_secret`/`delete_proxy_secret` are unchanged: `secrets.json` stays laptop-only regardless
  of a sandbox's deploy state (`01_ideas.md`, "Secret transport at deploy time" — only the
  *materialised* per-sandbox output travels, never the raw store).

## Remaining: Phase D — Undeploy and destroy lifecycle wiring

- `src-tauri/src/services/sandbox.rs::undeploy`: signature gains `registry: &Arc<ProviderRegistry>`
  (threaded from `src-tauri/src/commands/deploy.rs::undeploy_sandbox`, which gains a matching
  `registry: State<'_, Arc<ProviderRegistry>>` parameter). After the existing session lookup and
  before `engine.terminate(name)`, if `store.connection_info(&session.host)` resolves to `Some(info)`
  and `proxy::exists` against that connection's provider is true, call
  `proxy::remove_proxy_remote(&remote_provider, name, &info)` (Resolved Decision 3). After the
  existing `store.start(name)` call, call `proxy::start_proxy_if_present(&registry.local(ProviderId::
  Podman), name)` (Resolved Decision 2).
- `src-tauri/src/services/sandbox.rs::destroy`: in the deployed-sandbox branch (the `Some(session)`
  path), before `store.remove_remote(name, &session.host)`, resolve the remote provider the same way
  and call `proxy::remove_proxy_remote` if a remote proxy exists; after `close_panes_and_forwards`
  and independent of the remote branch, also call the existing `remove_proxy_if_present(app, registry,
  name)` to clean up the local (stopped) sidecar — a proxy-enabled, still-deployed sandbox being
  destroyed has state on both sides, and both must go (Resolved Decision 4).
- Both call sites reuse `store.connection_info` (Phase A) and `proxy::exists`/`remove_proxy_remote`
  (Phase B) with no further new functions.

## Remaining: Phase E — Wire proxy creation into `deploy` and lift the refusal (blocked)

**Do not start this phase until the live two-machine spike described in `01_ideas.md`'s Validation
section has run against a genuine non-WSL remote Podman host and confirmed the bridge-network and
mitmproxy-image assumptions it lists as unconfirmed.** Phases A–D compile, unit-test, and pass the
`light` e2e tier (via mocked `SandboxBackend::connection_info` and `proxy::exists` returning `false`
for every mock sandbox) with zero behavioural change to `deploy` — this phase is the only one that
changes what `deploy` actually does.

- `src-tauri/src/backends/mod.rs::deploy`: remove the early `if crate::proxy::exists(&self.registry,
  name).await { return Err(BackendError::ProxyDeployUnsupported ...) }` block. In its place, resolve
  `let proxied = crate::proxy::exists(&self.registry.local(ProviderId::Podman), name).await;` early
  (unchanged local-only check, see Phase B), and keep it in scope through the rest of the function.
  After `target_provider` is constructed and before `target_provider.create_instance(&spec)`: if
  `proxied`, call `crate::proxy::ensure_proxy_image(&self.app, &target_provider).await?` then
  `crate::proxy::create_proxy_remote(&self.app, &target_provider, name, &info).await?` (`info` is the
  `ConnectionInfo` already bound from `SyncStrategy::Remote(info)` earlier in the function), and use
  the returned `ProxyAttachment`'s `env`/`network` fields when building the `InstanceSpec` in place of
  today's hardcoded `env: Vec::new(), network: None`. After `target_provider.start(name).await?`: if
  `proxied`, call `crate::proxy::read_ca_pem_remote(&info, name)` then
  `crate::proxy::inject_ca(&target_provider, name, &pem).await?`. Immediately after the existing
  `owner.stop(name).await?` call (which stops the *local* sandbox container): if `proxied`, call
  `crate::proxy::stop_proxy(&self.registry.local(ProviderId::Podman), name).await?`.
- `src-tauri/src/backends/types.rs`: `BackendError::ProxyDeployUnsupported` variant and its doc
  comment are removed once this phase lands (no other caller constructs it); `src-tauri/src/error.rs`'s
  `kind()`/`From<BackendError>` arms and `src/lib/errors.ts`'s `AppErrorKind` union drop the matching
  entries (`src-tauri/AGENTS.md`'s error-mirroring gotcha table).
- `src-tauri/src/commands/deploy.rs::deploy_sandbox`: unchanged — it already just calls
  `store.deploy(&name, &host)` and reports whatever `RemoteDeployment` comes back; no proxy-specific
  branching belongs at the command layer.

## Lifecycle / Cross-Cutting Concerns

**Parallel-worktree state.** The new remote state root (`<home>/mio-proxy/<sandbox>/`, Phase B) is
never under `app_data_root` and does not need `MIO_APP_DATA_DIR` — it lives on the *remote host's*
filesystem, keyed by sandbox name, and sandbox names are already run-scoped in e2e (the same
precedent `docs/features/network-proxy/02_dev.md`'s Cross-Cutting Concerns cites for the local
`mio-proxy-<sandbox>` network name). The new local `meta.json` field (`"location"`, connection name)
is written through the existing `state_dir(app, sandbox)`, which is already `MIO_APP_DATA_DIR`-aware
— no new path resolution is introduced there. No new host port is introduced: the remote proxy's
port stays inside its own bridge network exactly as the local one does. No new bundled binary is
introduced. No new environment variable is introduced — `MIO_PROXY_IMAGE` is reused unchanged for a
remote pull.

**Host-global resource scope is unchanged, not new.** This feature does not introduce a new
host-global external resource; it uses more heavily one `docs/features/remote-deploys/01_ideas.md`
already requires (a registered remote Podman connection), which already confines any spec exercising
a real deploy to the host-locked `standard`/`heavy` e2e tier (`e2e/AGENTS.md`). Phase B's SSH-transport
functions (`remote_mkdir`, `remote_write_file`, `remote_read_file`, `remote_rm_rf`) have no mock, the
same way `backends::remote_mkdir` has none today — light-tier coverage of Phases A–D is limited to the
routing/branching logic (`services/proxy.rs`'s `Option<ConnectionInfo>` branch selection,
`SandboxBackend::connection_info`'s mock returning `None`), not the SSH transport itself, which only
`standard`/`heavy` specs against a real registered connection can exercise.

**Teardown remains name-scoped, not global.** `remove_proxy_remote`'s `remote_rm_rf` only ever runs
against one named sandbox's own remote state directory, reached exclusively through `undeploy` or
`destroy` acting on that sandbox — never a periodic or cross-connection sweep (Resolved Decision 8) —
so it cannot cross-worktree-collide with another run's state on the same shared remote connection.

**State derivation is unchanged in kind.** Proxy enablement is still derived purely from container
presence (`docs/features/network-proxy/02_dev.md` Resolved Decision 3): `exists`/`status`
(Phase B) ask whichever connection's `provider` they are given, rather than always asking the local
one, but the underlying "presence, not a persisted flag" model is untouched.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `cargo check` fails after Phase A with a privacy error on `ConnectionInfo` | `ConnectionInfo` (`src-tauri/src/backends/types.rs`) was left `pub(super)` while `proxy/mod.rs` (Phase B) tries to reference it by full path from outside `backends` | Widen `ConnectionInfo` to `pub(crate)` in `src-tauri/src/backends/types.rs` as the first change in Phase A; do not add a duplicate type in `proxy/types.rs` instead (Resolved Decision 6) |
| `undeploy_sandbox` fails to compile after Phase D with a missing-argument error | `services::sandbox::undeploy`'s signature gained a `registry: &Arc<ProviderRegistry>` parameter, but `src-tauri/src/commands/deploy.rs::undeploy_sandbox` was not updated to accept and thread a matching `State<'_, Arc<ProviderRegistry>>` | Add the `registry` parameter to `undeploy_sandbox` and pass it through, mirroring how `destroy_sandbox` (`src-tauri/src/commands/sandbox.rs`) already threads `registry` into `services::sandbox::destroy` |
| A `light` e2e spec covering `proxy_status`/`set_proxy_rules`/`restart_proxy` for a deployed sandbox hangs or errors | Phase C's `services::proxy::resolve` calls `store.connection_info`, which the mock `SandboxBackend` (Phase A) always answers with `None` — a light-tier spec cannot actually exercise the remote branch | Scope specs that need the remote branch to `standard`/`heavy` against a real registered connection, per Cross-Cutting Concerns; `light` can only cover the `None` (local) branch |
| A proxy-enabled sandbox's `undeploy` restarts the local container but its traffic is not inspected afterward | `proxy::start_proxy_if_present` (Phase B/D) restarted the local `<sandbox>-proxy` container, but the local sandbox container's own env vars/network attachment were never touched by `deploy`/`undeploy` in the first place (they were only ever pointed at the local proxy) | Nothing to fix in the proxy path itself if this occurs — check whether the local sandbox container's `--network`/`--env` still reference the local `mio-proxy-<sandbox>` network and IP, which `deploy`'s stop-not-remove design (Resolved Decision 2 discussion, Proposed Model) is specifically meant to preserve |
| A remote proxy's `certs/mitmproxy-ca-cert.pem` is read as empty or truncated right after `create_proxy_remote` returns | `wait_for_ca_remote`'s `ssh ... test -f` polling loop confirmed the file's existence but `mitmdump` was still mid-write when `read_ca_pem_remote` ran immediately after | Confirm `wait_for_ca_remote` (Phase B) polls with the same attempt/interval budget `wait_for_ca` uses locally rather than a single check; if flaky under real network latency, this is exactly the kind of risk the live spike in `01_ideas.md` Validation exists to surface before Phase E ships |
| A sandbox's real secret value is found on a remote host's disk that the deploying sandbox's own rules never referenced | `write_materialised_rules_remote` (Phase B) was passed the wrong `workspace_path`, or a caller substituted the raw `secrets.json` for the per-sandbox materialised output | `write_materialised_rules_remote` must call `rules::load_rules(workspace_path)` scoped to the one sandbox being deployed and never write `secrets.json` itself to any remote path (`01_ideas.md`, "Secret transport at deploy time") |
| `deploy` on a proxy-enabled sandbox fails with a Podman error about an unknown or duplicate network name on the target connection | `allocate_network`'s `10.90.<index>.0/24` walk (`src-tauri/src/proxy/mod.rs`) is per-connection already (each `PodmanProvider::create_network` call is `--connection`-scoped), so a collision here means the same index was already claimed by another proxy on that specific target connection | Confirm `create_proxy_remote` calls `allocate_network(target_provider, sandbox)` (not the local provider) so the walk's overlap-retry runs against the target connection's own existing networks |
