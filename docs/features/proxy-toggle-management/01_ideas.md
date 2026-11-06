---
type: ideas
title: "Proxy Toggle Management: App-Global Default and In-Place Recreation"
feature: proxy-toggle-management
---
# Proxy Toggle Management: App-Global Default and In-Place Recreation

This document scopes two follow-ons deferred out of `docs/features/network-proxy/04_improvements.md`
("App-global default for the proxy toggle" and "Switching an existing sandbox between proxied and
unproxied"), bundled as one feature because both are about the same underlying surface: what
proxied state a sandbox ends up in and how that state is set. The first is small — an `AppSettings`
field and its create-dialog wiring. The second is the substantial piece: recreating a sandbox's
container in place around its preserved workspace so an already-created sandbox can change its
proxied state without losing the sandbox identity (name, SSH access, workspace contents) a user has
already built up around it. Relevant existing files: `src-tauri/src/settings/types.rs` and
`src-tauri/src/settings/mod.rs` (`AppSettings`, `resolve_default_provider`), `src-tauri/src/backends/mod.rs`
(`CliBackend::create`, `ensure_sandbox_identity`, `ensure_sandbox_alias`, `install_sandbox_key`,
`forget_sandbox_host_key`), `src-tauri/src/backends/types.rs` (`SandboxBackend`, `Sandbox`),
`src-tauri/src/provider/mod.rs` and `types.rs` (`PodmanProvider::create_instance`/`remove`,
`InstanceSpec`), `src-tauri/src/proxy/mod.rs` (`create_proxy`, `remove_proxy`, `exists`, `status`),
`src-tauri/src/services/sandbox.rs` (`create`, `destroy`, `close_panes_and_forwards`),
`src-tauri/src/network/mod.rs` (`NetworkManager::close_sandbox`), `src-tauri/src/workspace/mod.rs`
(`close_app`), `src-tauri/src/runtime/mod.rs` and `types.rs` (`RuntimeManager::reproduce_remote`),
`src-tauri/src/ssh/mod.rs` (`write_alias`), and `src-tauri/resources/Containerfile`
(`MISE_DATA_DIR`).

**A note on `docs/features/network-proxy/02_dev.md`'s accuracy**: that document's phase headings
still read "Remaining" for Phases B through E, but the code they describe is already built —
`src-tauri/src/proxy/mod.rs`, `src-tauri/src/proxy/rules.rs`, `src-tauri/src/commands/proxy.rs`, the
`proxy: Option<bool>` parameter on `create_sandbox` (`src-tauri/src/commands/sandbox.rs`), and the
create-dialog toggle in `src/routes/projects/CreateSandboxItem.svelte` all exist and match that
document's Concrete Model and Resolved Decisions. This document treats network-proxy as fully shipped
and does not attempt to correct its stale phase headers — that is a housekeeping fix for whoever next
touches that file, not part of this feature's scope.

## Current State

**No app-global default exists for the proxy toggle.** `AppSettings` (`src-tauri/src/settings/types.rs`)
has `default_provider` but no proxy-related field. `create_sandbox` (`src-tauri/src/commands/sandbox.rs`)
defaults an absent `proxy` argument to `false` unconditionally (`proxy.unwrap_or(false)`), not through
any settings lookup. `CreateSandboxItem.svelte` seeds its `provider` state from `get_settings()` in
`onMount` but hardcodes `let proxy = $state(false)` with no equivalent read. `default_provider`'s own
wiring is the pattern to mirror: `settings::resolve_default_provider` (`src-tauri/src/settings/mod.rs`)
loads the persisted value (with a `MIO_MACHINE_PROVIDER` env override for e2e, which has no proxy
analogue needed here), and `src/routes/settings/+page.svelte` is where the persisted value is edited
and saved through `get_settings`/`set_settings`.

**No sandbox recreation mechanism exists anywhere in this codebase.** The only related code is a
"dismiss the update prompt" marker: `dismissed_image_version`/`dismiss_image_update`
(`src-tauri/src/backends/mod.rs`), surfaced through `get_dismissed_image_version`/`dismiss_image_update`
(`src-tauri/src/commands/image.rs`). It records that a user saw and ignored a base-image version bump;
nothing removes or recreates a container. `SandboxBackend::destroy` (`src-tauri/src/backends/types.rs`)
and `CliBackend::create` (`src-tauri/src/backends/mod.rs`) exist independently and are never chained
together for one sandbox today.

**The workspace bind mount is the one piece of sandbox state proven to survive container removal.**
`PodmanProvider::create_instance` (`src-tauri/src/provider/mod.rs`) mounts `spec.workspace_source` as
a `--volume host:/root/work` bind mount — an ordinary host directory, not a Podman-managed volume.
`PodmanProvider::remove` (`src-tauri/src/provider/mod.rs`) runs `podman rm --force`, which deletes the
container and never touches a bind-mount source. `CliBackend::resolve_create_workspace`
(`src-tauri/src/backends/mod.rs`) confirms the directory-reuse assumption already holds for a
sandbox created against an external folder (`existing_path: Some(path)`): it accepts any path outside
`workspace_root()` that no *other* live sandbox already claims, which is exactly the shape a recreate
needs for a sandbox's own already-existing workspace — except for the default (workspace-root-derived)
case: `resolve_create_workspace`'s `Some` branch rejects any path inside `workspace_root()` with
`BackendError::PathAlreadyTracked`, specifically to stop a *new* sandbox from importing a path the
default flow already owns. A recreate of a default-location sandbox cannot go through the public
`existing_path: Some(...)` path for this reason — see Proposed Model.

**Everything a fresh container needs besides the workspace is not preserved by a bind mount and has
its own re-establishment path already built for a different scenario (moving a sandbox to a new
container elsewhere via `deploy`).**

- **mise-installed toolchains** live at `MISE_DATA_DIR=/root/.local/share/mise`
  (`src-tauri/resources/Containerfile`), outside `/root/work`. Only the manifest (`mise.toml`) and
  lockfile (`mise.lock`) inside the workspace survive a `podman rm`; the actual installed tool
  binaries do not. `RuntimeManager::reproduce_remote` (`src-tauri/src/runtime/types.rs`,
  implemented in `src-tauri/src/runtime/mod.rs`) already solves exactly this problem for `deploy`
  (`src-tauri/src/backends/mod.rs`): it copies the manifest/lockfile onto a freshly created container
  and runs `mise install` to reinstall the lock-pinned tools. It takes a `MachineRef` target and a
  `workspace_host_path`, with no assumption that the target is remote.
- **The SSH keypair** (`ensure_sandbox_identity`, `src-tauri/src/backends/mod.rs`) is a per-sandbox
  keypair under `<app_data_root>/ssh-keys/<name>/`, generated once and reused: the function checks
  the file exists before regenerating. A recreate that keeps the same sandbox name reuses the same
  keypair for free, with nothing to redo.
- **The authorized key inside the container** (`install_sandbox_key`, `src-tauri/src/backends/mod.rs`)
  is copied in via `provider.copy_in` to the fixed path `sshd_config.container` names, landing inside
  the container's own filesystem outside `/root/work`. A `podman rm` erases it; a recreated container
  needs `install_sandbox_key` run again against the new container id.
- **The sshd host key** is baked into the base image, unrelated to any per-container state.
  `forget_sandbox_host_key` (`src-tauri/src/backends/mod.rs`) unconditionally clears the sandbox's
  cached `~/.ssh/sandbox-mio/known_hosts` entry on every `create`/`deploy`, defensively, in case the
  image changed since the entry was cached — a recreate should do the same even though, ordinarily,
  the image (and therefore the host key) is unchanged across a same-image recreate.
- **The SSH alias** (`ensure_sandbox_alias`, `src-tauri/src/backends/mod.rs`) writes an OpenSSH `Host`
  block to `~/.ssh/sandbox-mio/config` (`src-tauri/src/ssh/mod.rs`) keyed by sandbox name. Its
  `ProxyCommand` embeds only the Podman connection name and the container name — both identical
  before and after an in-place recreate that keeps the same sandbox name on the same connection — so
  the alias's *content* does not change across a recreate. `write_alias` is idempotent
  (`src-tauri/src/ssh/mod.rs`'s block-replace-by-marker logic), so re-running it costs nothing and
  matches the ordering `CliBackend::create` already establishes.
- **Panes and forwards** are torn down together by `services::sandbox::close_panes_and_forwards`
  (`src-tauri/src/services/sandbox.rs`): `workspace::close_app` (`src-tauri/src/workspace/mod.rs`)
  closes every embedded webview pane for the sandbox and clears its `WorkspaceRegistry` entries;
  `NetworkManager::close_sandbox` (`src-tauri/src/network/mod.rs`) terminates every Mutagen forward
  (terminal, agent-web, exposed ports) for the sandbox, kills any relay-pool `socat` processes, and
  cancels its `.mio/ports.json` poll task. This helper already runs before `destroy` and `undeploy`
  mutate the container out from under a live SSH channel, and is the one this feature reuses verbatim
  before removing the old container. Panes are lazily re-established on demand, not automatically:
  `open_terminal_pane`/`open_agent_web` (`src-tauri/src/commands/panes.rs`) check `WorkspaceRegistry`
  for an existing pane and otherwise call `prepare_terminal_webview`/`prepare_agent_web_pane`
  (`src-tauri/src/services/panes.rs`), which start a fresh zellij/paseo daemon inside whichever
  container is currently live under the sandbox's name and open a fresh Mutagen forward. `undeploy`
  (`src-tauri/src/services/sandbox.rs`) already establishes the precedent this feature follows: it
  closes panes/forwards, replaces what the sandbox's container is, and never reopens panes itself —
  the user reopens them from the UI, same as opening a Terminal pane on any sandbox for the first
  time in a session.
- **Injected credential bundles** (`src-tauri/src/credentials/mod.rs`) are stored per *service*
  (`bundle_dir(app, service_id)`), not per sandbox — `credentials::capture`/`inject` can push a given
  service's bundle into any sandbox on demand, and nothing does so automatically today. Whatever
  bundle a user injected into the old container's home directory (outside `/root/work`) does not
  survive `podman rm`, and nothing re-injects it automatically after a recreate — this is already true
  of a fresh `create` today and is not a new gap this feature introduces.

**The proxy attachment itself is baked in at `podman create` time and cannot be changed on a live
container.** `docs/features/network-proxy/02_dev.md`'s Cross-Cutting Concerns section ("State
derivation") already states this plainly: "a sandbox cannot be switched between proxied and unproxied
after creation, because its proxy environment variables and network attachment are baked into `podman
create`. Toggling means recreating the sandbox." This feature is that recreation.

## Proposed Model

### An `AppSettings.default_proxy` field, mirroring `default_provider` exactly

`AppSettings` (`src-tauri/src/settings/types.rs`) gains `pub default_proxy: bool` (serde default
`false`, matching every other field's `#[serde(default)]` container attribute already on the struct).
A new `settings::resolve_default_proxy(app) -> Result<bool, SettingsError>`
(`src-tauri/src/settings/mod.rs`) loads the persisted value the same way `resolve_default_provider`
does, with no environment-variable override — `default_provider`'s `MIO_MACHINE_PROVIDER` override
exists so an e2e run can point sandbox creation at a provider without writing to the developer's real
`settings.json`; no e2e need for an equivalent proxy override is known today (see Open Questions).
`create_sandbox` (`src-tauri/src/commands/sandbox.rs`) changes its fallback from
`proxy.unwrap_or(false)` to resolving the setting when the caller passes no explicit value, the same
shape `provider` already uses one line above it. `CreateSandboxItem.svelte` extends its existing
`onMount` settings fetch (already reading `defaultProvider`) to also read `defaultProxy` and seed the
`proxy` `$state` variable, still overridable per sandbox by the existing switch — and still forced to
`false` the moment the provider picker selects microsandbox, unchanged from today. `src/routes/settings/+page.svelte`
gains a toggle next to (or beside) the existing `defaultProvider` picker, following that section's
existing `$state`/`saved`-flag/`get_settings`/`set_settings` round-trip pattern.

### A recreate-in-place operation, keyed off the sandbox's own preserved workspace

A new internal sequence — a service function in `src-tauri/src/services/sandbox.rs` (tentatively
`retoggle_proxy`) fed by a new Tauri command — removes a sandbox's container and creates a fresh one
under the same name, same workspace, with the opposite proxy attachment, then re-establishes every
piece of per-container state Current State enumerates as not bind-mount-durable.

Gates before anything is touched, mirroring `CliBackend::create`'s existing proxy-attachment refusal:
the target sandbox must be on the Podman provider and exec-transport-capable
(`Sandbox::supports_exec_transport`, `src-tauri/src/backends/types.rs`) — a `PublishRelay` sandbox's
zellij reachability depends on pasta's `--host-lo-to-ns-lo`, which a proxy attachment replaces, exactly
as `CliBackend::create` already refuses today. A deployed sandbox (one with a live Mutagen session
under its name) is refused too: recreate operates on the *local* container only, and a deployed
sandbox's live container lives on a different Podman connection this operation never touches.

The sequence:

1. Capture the sandbox's current `workspace_path` and `provider` from `SandboxBackend::list`
   (`src-tauri/src/backends/types.rs`) before anything is removed.
2. `close_panes_and_forwards` (`src-tauri/src/services/sandbox.rs`), reused verbatim — the same call
   `destroy`/`undeploy` already make before disrupting the SSH channel a pane's forward runs over.
3. If the target state is proxied and no proxy sidecar exists yet, run `ensure_proxy_image` then
   `create_proxy` (`src-tauri/src/proxy/mod.rs`) — the identical two calls `services::sandbox::create`
   already makes — **before** removing the old container. A failed image pull or network allocation
   at this step leaves the still-working old sandbox completely untouched, the same fail-fast-before-
   mutating discipline `services::sandbox::create`'s own proxy branch already follows.
4. `provider.remove(name)` (`src-tauri/src/provider/mod.rs`, `podman rm --force`) removes the old
   container. The workspace bind-mount source is a host directory outside the container and is
   untouched.
5. Recreate the container against the *captured* `workspace_path` directly — a new internal
   constructor path in `src-tauri/src/backends/mod.rs`, not a call through the public `create` command
   with `existing_path: Some(workspace_path)`, because `resolve_create_workspace`'s `existing_path`
   branch rejects any path inside `workspace_root()` (`BackendError::PathAlreadyTracked`) precisely to
   stop a *different*, brand-new sandbox from importing a path a default-location sandbox already
   owns — a check that misfires against a sandbox recreating *itself*. The new path skips that
   validation (the workspace is already known-good — it is the same sandbox's own directory) and
   otherwise builds the same `InstanceSpec` `CliBackend::create` builds, with the new proxy attachment
   (`Some`/`None`) in place of the old one.
6. `forget_sandbox_host_key` then `install_sandbox_key` then `ensure_sandbox_alias`
   (`src-tauri/src/backends/mod.rs`), the same three calls `CliBackend::create`'s `exec_capable` branch
   already makes for a fresh sandbox, run again against the new container.
7. If the target state is proxied, `inject_ca` (`src-tauri/src/proxy/mod.rs`) pipes the sidecar's CA
   cert into the new container — the same step `services::sandbox::create` performs via its private
   `inject_proxy_ca` helper.
8. `runtime.reproduce_remote(name, &local_machine_ref, workspace_path)`
   (`src-tauri/src/runtime/types.rs`) reinstalls the mise-locked toolchain the fresh container lost,
   targeting the sandbox's own local connection as the `MachineRef` instead of a remote one — the
   method has no assumption baked in that its target is remote, only that it is *some* `MachineRef`
   with a provider to drive.
9. If the target state is unproxied and a proxy sidecar existed before step 4, `remove_proxy`
   (`src-tauri/src/proxy/mod.rs`) tears it down now, after the new unproxied container is confirmed
   up — the sandbox has no network path to the old sidecar the instant its own container is recreated
   without the bridge attachment, so nothing is lost by removing the sidecar last.

No rollback exists once step 4 has run: the old container is gone, so a failure partway through steps
5–9 cannot "put it back." This is different from `services::sandbox::create`'s rollback discipline,
which can always fall back to "nothing was created yet" because the sandbox never existed before that
call. Here the accepted failure mode is "the container needs another recreate attempt with the same
target state"; the workspace itself is never at risk, only the container. See Open Questions.

### The toggle lives in the proxy panel, not the create dialog

The create-dialog switch (`CreateSandboxItem.svelte`) only ever sets a sandbox's *initial* proxy
state — Proposed Model above changes what it defaults to, not what it does. Retoggling an *existing*
sandbox's state is a distinct, disruptive action (it closes every open pane, drops the installed
toolchain pending reinstall, and briefly makes the sandbox unreachable) that belongs in the proxy
panel `docs/features/network-proxy/02_dev.md` Phase E already plans under `src/lib/workspace/`
(alongside `PortsManager.svelte`'s pattern), next to the panel's existing `proxy_status` display. The
new control is a confirmation-gated action, not a bare switch, given the disruption it causes.

## Technology Choices with Tradeoffs

### Recreate-in-place (chosen) vs. every considered alternative

Chosen: destroy the old container, create a fresh one under the identical sandbox name against the
same workspace directory, re-establish per-container state.

- **Keep the old container attached to both networks and flip which one carries live traffic** —
  not viable at all, not merely rejected on cost: `docs/features/network-proxy/02_dev.md`'s Spike
  Results already proved Podman refuses to combine `pasta` with a second `--network` on one container
  (`cannot set multiple networks without bridge network mode, selected mode pasta`), and proxy
  environment variables are set via `--env` at `podman create` time — Podman has no live-update path
  for either a container's network attachments or its environment. There is no "don't remove the
  container" option to weigh against recreate; recreate is the only mechanism that can change either.
- **Rename-based recreate: create the new container under a throwaway name, then rename it to the
  original once healthy** — rejected: Podman has no atomic container rename that also relocates a
  bind-mounted workspace's identity, and this project's SSH identity/alias are keyed by sandbox name
  (`ensure_sandbox_identity`, `ensure_sandbox_alias`, both `src-tauri/src/backends/mod.rs`), so a
  rename step would need its own identity/alias relocation logic — doubling the surface this feature
  touches for a benefit (a moment where two containers briefly coexist) recreate does not need, since
  nothing else depends on the sandbox staying reachable mid-recreate.
- **A one-shot script drives `podman create`/`podman rm` directly, bypassing `SandboxBackend`
  entirely** — rejected: it would duplicate `CliBackend::create`'s exact `InstanceSpec`-building
  logic (image, labels, workspace mount, exec-transport gating) outside the one place that logic is
  already tested and maintained, for no isolation benefit — recreate is not a different sandbox
  lifecycle model, it is `create` and `destroy` sequenced around the same name.

### Reusing `RuntimeManager::reproduce_remote` for the local reinstall (chosen) vs. a new "reinstall in place" method

Chosen: call the existing method with a `MachineRef` pointed at the sandbox's own local connection
instead of a genuinely remote one.

- **A new, recreate-specific reinstall method** — rejected: the problem recreate has (a fresh
  container with the manifest but none of the installed tools) is identical in shape to the problem
  `deploy` already solved for a *remote* fresh container, and `reproduce_remote`'s contract (seed
  manifest/lockfile, run `mise install`) carries no remote-only assumption — it takes a `MachineRef`
  and a `workspace_host_path`, and resolves the provider to drive from the `MachineRef` alone
  (`RuntimeManager::provider_on`, mirrored in `src-tauri/src/runtime/mod.rs`). The one cost is a
  redundant manifest/lockfile copy onto a container whose bind-mounted workspace already has the
  current manifest — cheap and harmless, not worth a second method to avoid.

### Podman-only, exec-transport-only gating (chosen) vs. attempting a microsandbox equivalent

Chosen: recreate is refused outright for a microsandbox-provider sandbox or a `PublishRelay` Podman
sandbox, mirroring `CliBackend::create`'s existing proxy-attachment gate exactly.

- **Support microsandbox too** — rejected: microsandbox has no proxy story at all yet
  (`docs/features/network-proxy/02_dev.md`, Resolved Decision 9 — `MsbProvider` refuses every proxy-
  related `MachineProvider` method outright), so there is no proxied state to toggle into or out of
  for that provider. Revisit only alongside microsandbox's own proxy support, not before it.

### App-global default feeds only fresh creates, never an explicit recreate (chosen) vs. reading it inside recreate too

Chosen: recreate always takes an explicit target-state argument from its caller; `AppSettings.default_proxy`
is consulted only by `create_sandbox`'s no-explicit-value fallback.

- **Have recreate fall back to the app-global default when no explicit target is given** — rejected:
  an existing sandbox already has an explicit, currently-observable proxied state
  (`proxy::exists`/`proxy::status`, `src-tauri/src/proxy/mod.rs`), so "no stated preference" never
  applies to it the way it does to a sandbox that has never existed. Collapsing the two would make the
  app-global default silently retoggle an existing sandbox if a caller ever omitted the argument by
  mistake — a correctness footgun for a purely cosmetic simplification.

### Confirmation-gated UI action (chosen) vs. a bare switch matching the create dialog's

Chosen: the proxy panel's retoggle control requires an explicit confirmation before running.

- **A switch identical to the create dialog's** — rejected: the create-dialog switch sets a state
  that has no consequence yet (nothing exists to disrupt). The panel's switch, on an existing sandbox,
  closes every open pane, drops the installed toolchain pending reinstall, and interrupts anything
  running in a Terminal pane — a real, momentarily destructive action along the lines
  `docs/features/host-tool-ssh-access/01_ideas.md` already treats as needing an explicit,
  surfaced choice rather than a silent flip, for a comparable trust/attack-surface change.

## Validation

A live spike was run on 2026-08-18, per the recommendation this section previously recorded. Evidence
and full narrative live in `spike/README.md` and `spike/recreate_spike.rs`
(`docs/features/proxy-toggle-management/spike/`); this section records the outcome.

The spike drove the real `PodmanProvider::create_instance`/`remove` (`src-tauri/src/provider/mod.rs`),
`CliBackend::ensure_key`, `ensure_sandbox_alias`, `forget_sandbox_host_key`
(`src-tauri/src/backends/mod.rs`), and `RuntimeManager::init`/`reproduce_remote`/`prepare_terminal`
(`src-tauri/src/runtime/mod.rs`) against a throwaway `mio-spike-toggle-test` sandbox on the real,
running `podman-machine-default` — not a mock, and not the developer's real sandboxes (`kok`, `jjj`),
which were confirmed present and untouched before and after the run. It created a sandbox, wrote a
workspace marker file, installed an SSH identity and alias, confirmed SSH and a Terminal-pane-
equivalent (`prepare_terminal`) worked, then recreated the container in place (`provider.remove` then
`create_instance` again against the identical workspace directory) and re-checked all three.

All three risks this section previously flagged as read-only-established (not empirically tested)
are now confirmed empirically:

- **Risk 1 — a bind-mounted workspace directory survives `podman rm --force`**: confirmed. The marker
  file wasn't just present on the host after `podman rm --force`; it was readable *inside* the freshly
  recreated container via `podman exec ... cat`, i.e. the bind mount genuinely re-attached to live
  content, not merely an empty directory that happened to survive.
- **Risk 2 — the SSH alias's rendered content is byte-identical before and after a same-name
  recreate**: confirmed by direct string comparison of the rendered block (not just "close enough") —
  identical `ProxyCommand`/`User`/`IdentityFile`/`UserKnownHostsFile`/`IdentitiesOnly`/
  `StrictHostKeyChecking` lines both times, exactly as `render_block` (`src-tauri/src/ssh/mod.rs`)
  predicted from reading alone.
- **Risk 3 — a fresh SSH connection / Terminal pane works against the recreated container**:
  confirmed both ways. A fresh `ssh` call succeeded with no host-key prompt or mismatch (the base
  image bakes a fixed sshd host key, so an unchanged image across a same-name recreate means an
  unchanged host key), and a fresh `RuntimeManager::prepare_terminal` call against the recreated
  container came back with a working `zellij web` token/port once `reproduce_remote` had reinstalled
  the mise-locked toolchain.

The spike's own harness — not the design — hit one gap on its first run: `reproduce_remote` failed
with `"mise.toml" could not be found on the host` because the spike initially never called
`RuntimeManager::init` to seed the manifest/lockfile, a step `services::sandbox::create` performs for
every real sandbox but `CliBackend::create` does not. Adding that one `runtime.init(&workspace)` call
before the first `create_instance` fixed it; the second run passed end to end with no other changes.
This is a confirmation, not a design finding: it shows `reproduce_remote` correctly *finds* an
existing manifest/lockfile rather than needing to create one, exactly what Proposed Model step 8 (and
`02_dev.md`'s Phase C step 6) already assumes — every sandbox `retoggle_proxy` can be called against
already went through `services::sandbox::create`, which always calls `RuntimeManager::init` before the
container exists, so the manifest is guaranteed present by the time any recreate could ever run
against it.

No divergence from this document's Proposed Model surfaced. `02_dev.md`'s phase plan can proceed as
written.

## Open Questions

- **Whether `resolve_default_proxy` needs a `MIO_*` environment override.** `resolve_default_provider`
  has `MIO_MACHINE_PROVIDER` so e2e can steer sandbox creation without touching the developer's real
  `settings.json`. Whether a proxy e2e spec needs the same escape hatch, or can simply pass
  `proxy: true` explicitly to `create_sandbox` every time (as today's proxy specs already do), is
  undecided — resolve once `03_e2e.md` for this feature is written and a concrete spec either does or
  does not need it.
- **What "no rollback after step 4" means for the UI on a mid-recreate failure.** The container is
  gone and the recreate did not finish; the proxy panel needs to show *something* other than the
  proxy-status display it shows for a healthy sandbox. Whether that is a distinct error state offering
  a retry (re-running the same recreate sequence, which is idempotent from step 4 onward since the old
  container is already gone) or a generic error toast identical to any other failed command is
  undecided.
- **Whether recreate should require the sandbox to be stopped first, or work regardless of running
  state.** `PodmanProvider::remove` already force-stops via `podman rm --force`
  (`src-tauri/src/provider/mod.rs`), so nothing technically requires a prior stop — but a user with an
  agent mid-task in an open Terminal pane loses that work the moment `close_panes_and_forwards` runs,
  with no warning beyond whatever the confirmation dialog (Proposed Model) says. Whether the
  confirmation copy needs to say more than "this will disconnect open panes" (e.g., naming which panes
  are currently open) is undecided pending `02_dev.md`'s frontend phase.
- **Whether credential bundles should be automatically re-injected after a recreate.** Current State
  notes this is already an existing gap for a fresh `create` (nothing auto-injects a captured bundle
  into a brand-new sandbox either), so recreate does not make anything strictly worse — but a user
  toggling proxy on an sandbox they already set up credentials in will notice the regression more
  acutely than a user creating a sandbox fresh. Whether this feature should close that gap (for both
  create and recreate at once) or leave it exactly as undecided as it already is today is open.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Recreating a default-location sandbox fails with `PathAlreadyTracked` | The recreate path called the public `create` command with `existing_path: Some(workspace_path)`, and `resolve_create_workspace` (`src-tauri/src/backends/mod.rs`) rejects any `existing_path` inside `workspace_root()` by design | Use the internal recreate constructor path (Proposed Model) that bypasses `resolve_create_workspace`'s `existing_path` validation instead of routing through the public `create` command |
| A recreated sandbox's Terminal pane opens against a mise toolchain that isn't installed | `MISE_DATA_DIR` (`src-tauri/resources/Containerfile`) lives outside `/root/work`, so a fresh container has the manifest but not the installed tools | Run `RuntimeManager::reproduce_remote` (`src-tauri/src/runtime/mod.rs`) against the sandbox's own local `MachineRef` before considering the recreate complete |
| SSH to a just-recreated sandbox fails with a host-key mismatch | `forget_sandbox_host_key` was skipped on recreate, leaving a cached `known_hosts` entry from the removed container | Call `forget_sandbox_host_key` (`src-tauri/src/backends/mod.rs`) unconditionally before `install_sandbox_key`, exactly as `CliBackend::create` already does |
| A recreated sandbox is unreachable over its SSH alias even though `write_alias` never ran again | Not actually a bug: the alias's `ProxyCommand` names only the connection and container name, both unchanged by a same-name recreate — the alias was never stale | Confirm the failure is really host-key or authorized-key related (see the two gotchas above) before suspecting the alias itself |
| A toggled-off sandbox's old proxy container/network lingers after recreate | `remove_proxy` (`src-tauri/src/proxy/mod.rs`) was never called, or was called before the new unproxied container was confirmed up and the failure path skipped it | Call `remove_proxy` only after the new container is confirmed running (Proposed Model step 9); `proxy::sweep` (`src-tauri/src/proxy/mod.rs`) also catches an orphan on next app startup as a backstop |
| A credential a user injected into a sandbox before toggling its proxy state is missing afterward | Injected credential bundles live in the container's home directory outside `/root/work` and do not survive `podman rm`, which recreate always runs | Expected today (Open Questions) — re-run `credentials::inject` (`src-tauri/src/credentials/mod.rs`) for the affected service after the recreate completes |
