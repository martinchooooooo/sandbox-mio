---
type: dev-plan
title: "Proxy Toggle Management: Build Plan"
feature: proxy-toggle-management
---
# Proxy Toggle Management: Build Plan

This document is the build plan for the app-global proxy default and the sandbox-recreate primitive
scoped in `docs/features/proxy-toggle-management/01_ideas.md`, and supersedes it wherever the two
disagree. Nothing is implemented yet — every phase below is `Remaining`.

## The Concrete Model

### The default setting is a fourth `AppSettings` field, resolved the same way `default_provider` already is

`AppSettings` (`src-tauri/src/settings/types.rs`) gains `default_proxy: bool`. `create_sandbox`
(`src-tauri/src/commands/sandbox.rs`) resolves an absent `proxy` argument through a new
`settings::resolve_default_proxy` (`src-tauri/src/settings/mod.rs`) instead of hardcoding `false`,
mirroring the line immediately above it that already resolves an absent `provider` argument through
`settings::resolve_default_provider`. `CreateSandboxItem.svelte` and `src/routes/settings/+page.svelte`
both extend their existing `defaultProvider` read/write round-trips to carry `defaultProxy` alongside
it.

### Recreate is `destroy` and `create` sequenced around one preserved workspace, driven by a new `SandboxBackend::recreate` seam

A new trait method, `SandboxBackend::recreate` (`src-tauri/src/backends/types.rs`), takes a sandbox
name and the *target* proxy attachment (`Option<&ProxyAttachment>`, `None` meaning "no proxy" —
exactly the shape `create` already takes), removes the sandbox's current container, and creates a
fresh one under the same name against the same workspace directory. `CliBackend::recreate`
(`src-tauri/src/backends/mod.rs`) is the sole implementation (Podman-only — see Resolved Decisions).
It shares its post-create exec-transport setup (host-key forget, key install, alias write) with
`CliBackend::create` and `CliBackend::deploy` through a new private helper,
`provision_exec_transport`, extracted from the near-identical three-call sequence all three already
need.

A new orchestration function, `services::sandbox::retoggle_proxy` (`src-tauri/src/services/sandbox.rs`),
is the caller `SandboxBackend::recreate` alone cannot be — it also has to close panes/forwards first,
build or tear down the proxy sidecar around the container swap, inject the CA when switching on, and
reinstall the mise-locked toolchain the fresh container lost. It takes a `target_proxied: bool` (never
a default — see Resolved Decisions) and the same dependency bundle `services::sandbox::create` already
takes, plus a `&SandboxRouter` (`src-tauri/src/services/routing.rs`) to resolve the sandbox's current
machine and gate on it being local and exec-capable in one call.

## Resolved Decisions

1. **Whether `resolve_default_proxy` needs a `MIO_*` environment override** (`01_ideas.md` Open
   Question 1) — resolved no. `resolve_default_provider`'s `MIO_MACHINE_PROVIDER` override exists so
   an e2e run can steer sandbox creation without writing to the developer's real `settings.json`;
   every proxy-related light/standard spec today (and every one this feature's own `03_e2e.md` will
   add) can pass `proxy: true`/`proxy: false` explicitly to `create_sandbox` instead of relying on the
   ambient default, the same way they already pass an explicit `provider` when they need one that
   isn't the persisted default. Revisit only if a concrete spec is written that cannot express itself
   this way.
2. **What the UI shows on a mid-recreate failure** (`01_ideas.md` Open Question 2) — resolved: the
   command surfaces the failure as an ordinary `AppError`, and the proxy panel shows a distinct
   "traffic inspection change failed — the sandbox's container needs to be recreated; retry?" banner
   in place of its normal `proxy_status` display whenever `retoggle_sandbox_proxy`'s last call failed,
   rather than the generic toast every other command failure gets. Retrying means calling
   `retoggle_sandbox_proxy` again with the same `target_proxied` value — safe because every step of
   `CliBackend::recreate` and `services::sandbox::retoggle_proxy` from container removal onward is
   idempotent (`provider.remove` on an absent container, `create_instance` under a free name, key
   install/alias write, `reproduce_remote`, `inject_ca`/`remove_proxy` all tolerate being repeated).
3. **Whether recreate requires the sandbox to be stopped first** (`01_ideas.md` Open Question 3) —
   resolved: no technical requirement (`podman rm --force` handles a running container), but the
   confirmation dialog (Phase E) calls `workspace_state` (`src-tauri/src/commands/panes.rs`, already
   exists) before showing itself and names which panes (Terminal, Agent) are currently open for the
   sandbox, so the user sees concretely what closes rather than a generic warning.
4. **Whether recreate re-injects credential bundles automatically** (`01_ideas.md` Open Question 4) —
   resolved: out of scope for this feature, left exactly as it already stands for a fresh `create`
   (nothing auto-injects a bundle there either). Recorded in `04_improvements.md` rather than solved
   here, matching how `docs/features/network-proxy/04_improvements.md` already defers "secret values
   sourced from the `credentials/` bundle store" for the same reason — no general per-sandbox mapping
   from "these services were once injected here" exists to drive an automatic re-injection from.

## Remaining: Phase A — App-global default

- `src-tauri/src/settings/types.rs`: add `pub default_proxy: bool` to `AppSettings`, doc-commented
  the way `default_provider` is (absent-from-old-settings-file behavior, what it seeds).
- `src-tauri/src/settings/mod.rs`: add `pub fn resolve_default_proxy(app: &AppHandle) -> Result<bool,
  SettingsError> { Ok(load(app)?.default_proxy) }` — no environment override (Resolved Decision 1).
- `src-tauri/src/commands/sandbox.rs`: `create_sandbox` resolves an absent `proxy` argument through
  `settings::resolve_default_proxy(&app)?` instead of `proxy.unwrap_or(false)`, mirroring the
  `provider` resolution immediately above it.
- `src/routes/projects/CreateSandboxItem.svelte`: extend the existing `onMount` settings fetch
  (`get_settings`) to also read `defaultProxy` and seed `let proxy = $state(...)` from it, still
  forced to `false` whenever the provider picker selects microsandbox (unchanged existing behavior at
  the picker's `onclick`).
- `src/routes/settings/+page.svelte`: add a `defaultProxy` `$state` boolean, a `Switch` control beside
  the existing `defaultProvider` picker's `Field.Field`, include it in the object passed to
  `set_settings`, and seed it from `get_settings` in the existing load effect — the exact three-point
  pattern `defaultProvider` already follows in this file.
- `messages/en.json` (and `de.json`, `ru.json`): a label key for the new settings toggle, deferred to
  after the feature is approved per the `feature-implement` skill's i18n-last convention; use a
  `<!-- TODO(i18n) -->`-style placeholder in the interim, matching the existing placeholder already on
  the create dialog's "Inspect traffic" label in `CreateSandboxItem.svelte`.

## Remaining: Phase B — Backend recreate seam

- `src-tauri/src/backends/types.rs`: add `BackendError::SandboxNotFound { name: String }` (recreate
  called for a name `list()` does not return), `BackendError::RecreateUnsupported { name: String }`
  (non-Podman provider or non-exec-capable image — the same condition `CliBackend::create` already
  gates proxy attachment on, promoted to a typed variant since this feature exposes it as a distinct,
  user-facing refusal rather than an inline `CommandFailed` string), and add `recreate` to the
  `SandboxBackend` trait:
  ```
  async fn recreate(&self, name: &str, proxy: Option<&crate::proxy::types::ProxyAttachment>) -> Result<Sandbox, BackendError>;
  ```
  (shown for the signature only — no source is checked in from this document; see the project's
  no-source-in-comments convention, `CLAUDE.md`).
- `src-tauri/src/backends/mod.rs`:
  - Extract `provision_exec_transport(&self, provider: &Arc<dyn MachineProvider>, name: &str,
    provider_id: ProviderId, connection: &str) -> Result<String, BackendError>` from the three-call
    sequence (`forget_sandbox_host_key`, `install_sandbox_key`, then `ensure_sandbox_alias` or
    `ensure_msb_sandbox_alias` depending on `provider_id`) that `CliBackend::create` and
    `CliBackend::deploy` each already inline separately. Both existing call sites switch to the
    extracted helper; behavior is unchanged, this is a pure de-duplication that `recreate` becomes the
    third caller of.
  - `CliBackend::recreate`: looks up the sandbox's current `workspace_path` and `provider` from
    `self.list()`, returning `SandboxNotFound` if absent; returns `RecreateUnsupported` unless the
    provider is `ProviderId::Podman` and the found `Sandbox::supports_exec_transport()` is true;
    builds the same `InstanceSpec` shape `create` builds (image, `WORKSPACE_LABEL` label, `env`/
    `network` from `proxy`) directly against the captured `workspace_path`, bypassing
    `resolve_create_workspace` entirely — that function's `existing_path` validation exists to gate a
    *different*, brand-new sandbox importing a path (`01_ideas.md`, Current State), which does not
    apply to a sandbox recreating itself against its own already-known-good directory; calls
    `provider.remove(name)` then `provider.create_instance(&spec)`; finishes with
    `provision_exec_transport`.
- `src-tauri/src/error.rs`: `kind()` arms for the two new `BackendError` variants (via whatever
  `AppError` conversion `From<BackendError> for AppError` already gives them — both fall through the
  existing `other => AppError::Backend(other)` arm, so no new `AppError` variant is needed, only the
  `BackendError::kind()` fallthrough already covers `Backend(_)` generically. No change needed here
  beyond confirming the fallthrough covers it — call out explicitly during implementation rather than
  add dead code speculatively).
- `src-tauri/src/backends/mock.rs` or equivalent mock plumbing (if `SandboxBackend` has a mock
  implementation distinct from `CliBackend` — confirm at implementation time; `MIO_MOCK_BACKEND`
  today swaps individual subsystem traits, and `SandboxBackend` itself is `CliBackend` always, driven
  by a mocked `ProcessRunner`/`MachineProvider` underneath, per `docs/features/scriptable-mock-backend/02_dev.md`
  — if so, `recreate`'s `light`-tier coverage comes for free through the same seam `create`/`deploy`
  already get, and this bullet is a no-op confirmation, not new code).

## Remaining: Phase C — Recreate orchestration

- `src-tauri/src/services/sandbox.rs`: `retoggle_proxy(name: &str, target_proxied: bool, store:
  &Arc<dyn SandboxBackend>, runtime: &Arc<dyn RuntimeManager>, network: &Arc<dyn NetworkManager>,
  registry: &Arc<ProviderRegistry>, router: &SandboxRouter, app: &tauri::AppHandle) -> Result<Sandbox,
  AppError>`:
  1. `router.locate(name)` (`src-tauri/src/services/routing.rs`) resolves `route.machine` and
     `route.exec_capable` in one call. Refuse with `BackendError::RecreateWhileDeployed { name }` (new
     variant, `src-tauri/src/backends/types.rs`) when `route.machine != registry.local_ref(ProviderId::Podman)`
     — `resolve_machine` inside `locate` already returns the deploy target's `MachineRef` for a
     deployed sandbox and the local one otherwise, so this one comparison is both the "is this
     sandbox deployed" gate and the source of the `MachineRef` step 6 below needs. Refuse with
     `BackendError::RecreateUnsupported { name }` when `!route.exec_capable`.
  2. `close_panes_and_forwards(name, network, app)` (`src-tauri/src/services/sandbox.rs`, existing,
     reused verbatim).
  3. If `target_proxied` and no proxy sidecar exists yet (`crate::proxy::exists(registry, name)`),
     `ensure_proxy_image` then `crate::proxy::create_proxy(app, &podman, name)` — identical to
     `services::sandbox::create`'s own proxy branch, run before anything is removed so a failure here
     leaves the old sandbox untouched.
  4. `store.recreate(name, attachment.as_ref())` — the new `SandboxBackend` method from Phase B, where
     `attachment` is `Some(&ProxyAttachment)` from step 3 when `target_proxied`, else `None`.
  5. If `target_proxied`, `inject_proxy_ca` (`src-tauri/src/services/sandbox.rs`'s existing private
     helper, already used by `create`) pipes the sidecar's CA into the fresh container.
  6. `runtime.reproduce_remote(name, &route.machine, &sandbox.workspace_path)`
     (`src-tauri/src/runtime/types.rs`) reinstalls the mise-locked toolchain — `route.machine` from
     step 1 is exactly the local `MachineRef` this needs, with no separate resolution.
  7. If `!target_proxied` and a proxy sidecar existed before step 4, `crate::proxy::remove_proxy(app,
     &podman, name)` now that the new unproxied container is confirmed up.
  8. Returns the `Sandbox` `store.recreate` produced.

  No rollback runs after step 4 fails partway — the old container is already gone by the time
  `store.recreate` is called (its removal is the first thing that method does), so nothing exists to
  restore. Resolved Decision 2 covers what the caller does with a failure here.

## Remaining: Phase D — Command and progress reporting

- `src-tauri/src/activity/types.rs`: add `TaskKind::SandboxRecreate` beside `ProxyImagePull`, for the
  same reason that variant exists — a multi-step operation (image pull, container swap, `mise
  install`) that would otherwise look hung.
- `src-tauri/src/commands/sandbox.rs`: `retoggle_sandbox_proxy(name: String, proxied: bool, ...) ->
  Result<Sandbox, AppError>`, wrapping `services::sandbox::retoggle_proxy` in `activity::scope(app,
  "sandbox-recreate", TaskKind::SandboxRecreate, Some(name.clone()), ...)` the same way
  `ensure_proxy_image` already wraps its own pull.
- `src-tauri/src/commands/mod.rs`: register `sandbox::retoggle_sandbox_proxy` in
  `tauri::generate_handler![]`.
- `src-tauri/src/error.rs` / `src/lib/errors.ts`: add `kind()`/`AppErrorKind` entries for
  `SandboxNotFound`, `RecreateUnsupported`, and `RecreateWhileDeployed` if Phase B's fallthrough
  (`other => AppError::Backend(other)`) does not already give each a distinct `kind` string the
  frontend can branch on — confirm which is true during implementation before adding redundant arms.

## Remaining: Phase E — Frontend

- `src/lib/workspace/` proxy panel component (`docs/features/network-proxy/02_dev.md` Phase E,
  already built beside `PortsManager.svelte`'s pattern under `src/routes/projects/`): add a "Switch
  traffic inspection on/off" control next to the existing `proxy_status` display, gated behind a
  confirmation dialog (Resolved Decision 3) that calls `workspace_state`
  (`src-tauri/src/commands/panes.rs`) first to list currently-open panes for the sandbox in its copy.
  On confirm, calls `retoggle_sandbox_proxy` and shows `activity`'s existing task-progress UI for
  `TaskKind::SandboxRecreate` (`src/lib/activity/activity.svelte.ts`, the same pattern `ProxyImagePull`
  already renders through) rather than inventing a second progress UI.
- Failure state (Resolved Decision 2): the panel renders a distinct banner instead of its normal
  `proxy_status` block whenever the last `retoggle_sandbox_proxy` call for this sandbox failed, with a
  retry button that re-invokes the command with the same `proxied` value.
- `messages/en.json` (+ `de.json`, `ru.json`): keys for the new control, its confirmation copy, and the
  failure banner — deferred to after approval per the i18n-last convention (Phase A note applies here
  too).

## Cross-Cutting Concerns

**No new host-global resource.** Recreate reuses the proxy subsystem's existing per-sandbox network
(`mio-proxy-<sandbox>`) and state directory (`<app_data_root>/proxy/<sandbox>/`,
`src-tauri/src/proxy/mod.rs`) exactly as `create`/`destroy` already do — it never allocates a new
network, port, or bundled binary of its own. `AppSettings.default_proxy` lives in the same
`settings.json` `default_provider` already does, under the same `MIO_SETTINGS_DIR`/`MIO_APP_DATA_DIR`
override (`src-tauri/src/settings/mod.rs`). The new `TaskKind::SandboxRecreate` is in-memory activity
state, not a new persisted or host-global path.

**State derivation is unchanged.** Proxy enablement is still derived from the `<sandbox>-proxy`
container's presence (`docs/features/network-proxy/02_dev.md`, Resolved Decision 3;
`crate::proxy::exists`) — `retoggle_proxy` changes which container exists, never writes a persisted
"this sandbox is proxied" flag anywhere. A crash mid-recreate leaves the container in whichever state
`CliBackend::recreate` reached, and the next `proxy_status`/`list_sandboxes` call reports exactly that
state with no separate reconciliation needed — the same "derived, not persisted" property
`docs/features/network-proxy/02_dev.md` already relies on.

**Teardown ordering matches `create`'s existing rollback discipline where it still applies, and
explicitly does not where it cannot.** Steps 1–3 of `retoggle_proxy` (Phase C) can fail with the old
sandbox fully intact, exactly like a failed `services::sandbox::create`. From step 4 onward
(`store.recreate`, which removes the old container as its first action) there is no state to roll
back to — see Resolved Decision 2. `remove_proxy` (step 7) always runs after the new container is
confirmed, not before, so a `target_proxied: false` recreate never leaves the sandbox in a state with
neither the old nor the new network attached and no addressable proxy left to blame a mid-step failure
on.

**Parallel-worktree e2e placement.** Every spec exercising a real recreate belongs in the `standard`
tier, not `light` — the same constraint `docs/features/network-proxy/02_dev.md`'s Cross-Cutting
Concerns already states for any spec touching the real `10.90.<index>` subnet allocator, since
`retoggle_proxy`'s `target_proxied: true` path goes through the identical `allocate_network`
(`src-tauri/src/proxy/mod.rs`). A `light`-tier spec can still cover the `MIO_MOCK_BACKEND=1` path
(command registered, gating errors returned for a non-exec-capable/deployed/unknown sandbox) without
touching real Podman.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `retoggle_sandbox_proxy` fails with `RecreateWhileDeployed` for a sandbox that looks local in the UI | `router.locate(name)` resolved a live Mutagen session under the sandbox's name, meaning its actual container lives on a different Podman connection | Undeploy the sandbox first (`undeploy_sandbox`), then retoggle its proxy state locally |
| A retoggled sandbox's Terminal pane opens against a mise toolchain that isn't installed | `store.recreate` produces a fresh container with the workspace's `mise.toml`/`mise.lock` but none of the previously-installed tool binaries (`MISE_DATA_DIR` is outside `/root/work`) | Confirm `services::sandbox::retoggle_proxy`'s step 6 (`runtime.reproduce_remote`) ran and succeeded before treating the recreate as complete |
| The proxy panel shows the generic error toast instead of the dedicated failure banner after a failed retoggle | The panel's failure-state check (Phase E) was not wired to `retoggle_sandbox_proxy`'s result specifically | Route `retoggle_sandbox_proxy`'s rejection through the panel's dedicated failure-banner state, not the app-wide generic error toast path |
| Retrying a failed retoggle with the opposite `proxied` value than originally requested leaves the sandbox in neither state cleanly | Every step from `store.recreate` onward assumes the same `target_proxied` value as the failed attempt; switching mid-retry can leave an orphaned proxy sidecar or a missing CA injection | Always retry with the exact same `proxied` value the failed call used; only start a fresh, different-direction retoggle once a retry with the original value has succeeded |
| A default-location sandbox's recreate fails with `PathAlreadyTracked` | `CliBackend::recreate` was implemented by delegating into `resolve_create_workspace` instead of building the `InstanceSpec` directly against the captured `workspace_path` | Bypass `resolve_create_workspace` entirely in `CliBackend::recreate` (Phase B) — it exists to gate a *different* sandbox's import of an already-tracked path, not a sandbox recreating itself |
| `provision_exec_transport`'s extraction changes `deploy`'s behavior | The helper was given a single `connection` parameter but `deploy` targets `target_connection` while `create`/`recreate` target `self.connection` — a copy-paste that hardcodes one connection breaks the other two callers | Pass `connection: &str` explicitly at every call site (Phase B) rather than defaulting the helper to `self.connection` internally |
