# Microsandbox Provider — Build Plan

This document is the build plan continuing `01_ideas.md`. It covers **only the first, Podman-only
phase** that `01_ideas.md`'s open question "How much of the refactor lands before the first
microsandbox code" defers here: extracting a primitive `MachineProvider` trait with `PodmanProvider`
as its sole implementation, and routing every subsystem that shells out to `podman` directly through
it. No microsandbox code, no second provider, and no user-visible change land in this phase; the
whole of it is verifiable by the existing `standard` e2e tier, because `PodmanProvider` reproduces
today's behaviour byte-for-byte.

Status: **All phases implemented (A–D).** The `MachineProvider` seam and `PodmanProvider` exist; every
subsystem (`backends`, `runtime`, `network`, `credentials`, `agents`) routes its container calls
through it, and `src-tauri/src/lib.rs` manages one `Arc<dyn MachineProvider>` shared across all of
them.

The microsandbox-specific model in `01_ideas.md` — `ProviderCapabilities`, `MachineRef`, the
`MachineManager` narrowing, the `msb` scripted runner, and the `default_provider` setting — is out of
scope here and is left for the follow-on build plans that add `MsbProvider` against the seam this
phase creates.

## The concrete model: `MachineProvider` and `PodmanProvider`

A new module `src-tauri/src/provider/` following the crate's `types.rs`-declares / `mod.rs`-implements
layout. `types.rs` declares the `MachineProvider` trait and its data shapes; `mod.rs` implements
`PodmanProvider` and the `default_provider()` constructor.

### What the trait declares

`MachineProvider` is the container-or-VM primitive seam every subsystem shares. It declares only
operations that generalise across a container engine and a microVM engine:

- `ensure_base_image` / `base_image_ready` — moved from `PodmanCliBackend` unchanged.
- `create_instance(&InstanceSpec) -> String` — returns the created instance id. `InstanceSpec` holds
  the provider-neutral inputs: `name`, `image`, workspace mount source (host path), optional labels,
  and a `PublishModel` describing whether ports are exec-tunnelled or published+relayed. All the
  Podman-specific argv (`--pull=missing`, `--tty`, `--network pasta[:--host-lo-to-ns-lo]`,
  `--publish`, the relay pool from `relay_pool_publish_args()`) lives inside
  `PodmanProvider::create_instance`, not in the spec.
- `list_instances() -> Vec<Sandbox>` — parses the engine's list output into the existing
  `crate::backends::types::Sandbox` shape (id, name, status, workspace_path, image_version), which is
  already provider-neutral. `Sandbox` stays the shared instance type to avoid a redundant parallel
  struct; `Sandbox::supports_exec_transport` remains a Podman-transport concern but is harmless on
  the shared type.
- `start` / `stop` / `remove` — the `podman start` / `stop` / `rm --force` lifecycle.
- `exec(name, argv, ExecOpts) -> Output` — one-shot exec. `ExecOpts` carries `workdir`, `env` pairs,
  and a `detached` flag (Podman's `-d`).
- `exec_piped(name, argv, stdin: &[u8]) -> Output` — exec with a stdin byte stream awaited to
  completion, for `tar_capture` / `tar_inject` / `write_file_via_stdin`.
- `exec_command(name, argv, ExecOpts) -> tokio::process::Command` — returns a configured, unspawned
  command for callers that need live bidirectional streaming (the interactive auth session in
  `agents`, and any future streamed consumer). The caller attaches its own piped stdio and spawns.
- `copy_in(name, host_path, dest)` — `podman cp`.

The trait deliberately omits `published_host_port`: the `PortTransport` seam from
`docs/features/mutagen-transport/02_dev.md` (`src-tauri/src/transport/`) already extracts that lookup,
with `PodmanPortTransport` and `PassthroughPortTransport` injected into `RuntimeManager` and
`NetworkManager` today. Reproducing it on `MachineProvider` would duplicate an existing seam.

### Two execution paths inside `PodmanProvider`

`PodmanProvider` holds the same two fields `PodmanCliBackend` holds today — a pinned `connection:
String` and an `Arc<dyn ProcessRunner>` — and routes calls down one of two paths, exactly mirroring
today's split in `src-tauri/src/backends/mod.rs`:

1. **Runner path (mockable).** Every one-shot, no-stdin call — `create_instance`, `list_instances`,
   `start`, `stop`, `remove`, `exec`, `copy_in`, `ensure_base_image` — builds `--connection
   <connection> <argv>` and calls `runner.run()`, so `ScriptedPodmanRunner` continues to divert it
   under `MIO_MOCK_BACKEND=1`. This is the path `PodmanCliBackend` already uses via
   `self.podman`/`podman_on`/`podman_unbound`.
2. **Direct-command path (not runner-mockable).** `exec_piped` and `exec_command` spawn
   `podman_command()` directly, because `ProcessRunner` models argv-to-`Output` with no stdin and
   cannot represent a piped or streamed child — the same limitation `PodmanCliBackend::listen_events`
   already works around. Consumers of this path are either production-only or mock-gated (see the
   mock-tier section below).

### What stays in `SandboxBackend`

`SandboxBackend` keeps its shape as the Tauri-facing orchestration trait. `PodmanCliBackend` retains
an `Arc<dyn MachineProvider>` and delegates the lifecycle methods (`create`, `list`, `start`, `stop`,
`destroy`, `ensure_base_image`, `base_image_ready`) to it, while keeping the operations that are
Podman-only by nature:

- **`deploy` / `remove_remote`** — remote-target container creation. These operate on an *arbitrary
  target connection*, not the provider's pinned one, so they construct a second `PodmanProvider`
  pinned to the target connection and drive `create_instance` / `remove` through it, reusing one argv
  builder rather than a parallel `Command::new("podman")`. This matches the future `MachineRef {
  provider, id }` direction (a provider instance per machine).
- **`listen_events`** — `podman events` streaming, mock-gated today; no microsandbox counterpart
  exists (`01_ideas.md` open question, deferred). Stays on `SandboxBackend`.
- **`list_hosts` / `add_host` / `prepare_host` / `provision_host`** — Podman system-connection
  registry and SSH host provisioning, meaningless outside Podman.
- **The exec-transport SSH-alias orchestration** — `ensure_sandbox_identity`, `ensure_sandbox_alias`,
  `install_sandbox_key`, `forget_sandbox_host_key`, `ensure_connection_alias`. These sit above the
  container primitive (they write `~/.ssh` config), so they stay in `backends`; they call
  `provider.copy_in` for the authorized-keys file instead of `podman cp` directly.

The container-create argv construction (both the exec-capable and publish+relay branches now
duplicated across `create` and `deploy`) collapses into `PodmanProvider::create_instance`, driven by
`InstanceSpec.publish_model`.

## Resolved Decisions

Restating every Open Question from `01_ideas.md`:

1. **Whether the provider ships user-visible in v1** — out of scope for this phase; no provider
   selection UI or setting is added here. Deferred to the `MsbProvider` build plan.
2. **Whether the base image's Podman dependency is acceptable for v1** — out of scope; this phase adds
   no base-image path beyond today's `podman build`.
3. **How `listen_events` behaves for microsandbox** — deferred; `listen_events` stays a Podman-only
   method on `SandboxBackend` and is not lifted onto `MachineProvider`, so the question does not block
   this phase.
4. **How the paseo and zellij daemons start without a detached exec** — deferred; `ExecOpts.detached`
   maps to Podman's `-d` in this phase, and how `MsbProvider` satisfies `detached` is left to its
   build plan.
5. **Which status vocabulary reaches `translateStatus()`** — deferred; `PodmanProvider::list_instances`
   emits today's lowercase Podman states unchanged.
6. **How `Sandbox.image_version` is derived** — resolved for Podman: `parse_image_version`
   (`src-tauri/src/backends/mod.rs`) moves with `list_instances` and is unchanged. The microsandbox
   snapshot-label variant is deferred.
7. **How much of the refactor lands before the first microsandbox code** — resolved by this document:
   the full `MachineProvider` extraction across `backends`, `runtime`, `network`, `credentials`, and
   `agents` (Phases A–D) ships as one Podman-only, standard-tier-verifiable body of work, with no
   `MachineRef`, `ProviderCapabilities`, `MachineManager` narrowing, or `msb` runner.
8. **How Mutagen handles the `ProxyCommand` alias and host-key prompts** — deferred; a microsandbox
   SSH concern with no bearing on the Podman-only extraction.

Additional decisions settled for this phase:

- **`Sandbox` is the shared instance type**, not a new `Instance` struct — it is already
  provider-neutral, and reusing it avoids a mapping layer and keeps `list` untouched at the IPC
  boundary.
- **Both existing mock strategies are preserved** (see below). This phase does **not** retire the
  hand-written `MockRuntime` / `MockNetworkManager`; doing so is recorded in `04_improvements.md` as a
  separate, larger change.
- **`machine/` is untouched.** Its `podman machine` / WSL shape and the `podman_command()` call at
  `src-tauri/src/machine/mod.rs:120` stay; the `MachineManager` narrowing is a later phase.

## Mock tiers and wiring

The `light` tier stays green with no spec changes because the two existing mock strategies are both
preserved:

- **`backends` uses the real struct over a scripted runner.** `PodmanProvider` holds the same
  `Arc<dyn ProcessRunner>` (`ScriptedPodmanRunner` under `MIO_MOCK_BACKEND=1`), so `PodmanCliBackend`
  driving the provider still routes its one-shot podman calls through the scripted seam. The
  `create` → `list` → `start` round-trip through `ScriptedPodmanRunner`'s tracked-container default
  keeps working.
- **`runtime` and `network` keep their hand-written mocks.** `default_runtime` and
  `default_network_manager` still return `MockRuntime` / `MockNetworkManager` under
  `MIO_MOCK_BACKEND=1`. Only the *real* `MiseRuntime` / `MutagenNetworkManager` gain an `Arc<dyn
  MachineProvider>`, so the provider (and its direct-command path) is exercised in production only —
  the same reachability `MockRuntime` / `MockNetworkManager` have today. No light spec touches the
  provider through these two subsystems.
- **`credentials` and `agents` streamed paths** (`tar_*`, `write_file_via_stdin`, `start_auth`) go
  through `exec_piped` / `exec_command`, which bypass the runner. These paths are not driven by the
  light tier today (they require real container exec with piped stdin) and are not newly reachable
  under mock after the refactor.

Wiring in `run()` (`src-tauri/src/lib.rs`): construct the provider once from the shared
`podman_runner` and connection, `app.manage()` it as `Arc<dyn MachineProvider>`, and pass it into
`default_backend`, `default_runtime`, and `default_network_manager`. Command handlers that call the
`credentials` / `agents` free functions pull `Arc<dyn MachineProvider>` from state and pass it where
those functions currently derive a `connection` from `runtime.local_connection()`.

A side benefit: `network`'s bare `Command::new("podman")` free function (which uses the host
`podman`, not the bundled `podman.exe` that `podman_command()` resolves) is replaced by provider
calls, so `network` finally uses the bundled binary like every other subsystem.

## Implemented: Phase A — provider seam and `PodmanProvider`, `backends` delegation

- New `src-tauri/src/provider/types.rs`: `MachineProvider` trait, `InstanceSpec`, `ExecOpts`,
  `PublishModel`, and `ProviderError` (mirroring `BackendError`'s `CommandFailed` / `ParseError`
  variants, convertible into `BackendError`).
- New `src-tauri/src/provider/mod.rs`: `PodmanProvider` (fields `connection`, `runner`), the two
  execution paths, and `default_provider(runner, connection)`. Move `parse_image_version`,
  `parse_connection_entry` usage as needed, `base_image()`, `image_exists_on`, `ensure_image_on`, and
  the create-argv construction into it.
- `src-tauri/src/backends/`: `PodmanCliBackend` gains a `provider: Arc<dyn MachineProvider>` field;
  `create` / `list` / `start` / `stop` / `destroy` / `ensure_base_image` / `base_image_ready` delegate
  to it. `deploy` / `remove_remote` build a target-pinned `PodmanProvider`. `install_sandbox_key`
  calls `provider.copy_in`. Register `provider` module in `src-tauri/src/lib.rs` and manage the state.
- Verify: `cargo check`, `cargo test`, `./build e2e light`, then `./build e2e` (standard).

As landed, two things differ from the sketch above: `base_image()` stays in `backends/mod.rs`
because `BASE_IMAGE`/`BASE_IMAGE_VERSION` are regex-parsed by `e2e/lib/podman-machine.ts` and their
declaration form is load-bearing (the provider calls `crate::backends::base_image()`); and the
unified `create_instance` always emits `--pull=missing`, so `deploy`'s remote create now carries it
too — a no-op, since `deploy` calls `ensure_base_image` immediately before, guaranteeing the image is
already present. `PodmanCliBackend` keeps its own `runner` field alongside `provider` for the
still-Podman-only host-registry and target-connection paths.

## Implemented: Phase B — route `MiseRuntime` through the provider

- Rename `PodmanMiseRuntime` to `MiseRuntime` (`src-tauri/src/runtime/`); replace its `podman` /
  `podman_on` helpers and the `crate::backends::podman_command()` call with `provider.exec` /
  `provider.copy_in`. The `mise_exec`, `seed_paseo_projects`, `reproduce_remote`, `prepare_terminal`,
  `prepare_agent_web`, `exec_in_container` bodies switch to `ExecOpts` (`workdir`, `env`, `detached`)
  instead of hand-built argv. Port lookup stays on the injected `PortTransport`.
- `reproduce_remote` targets an arbitrary connection, so it takes a target-pinned provider the same
  way `deploy` does.
- `default_runtime` constructs `MiseRuntime { provider, ... }` in the production branch; the
  `MockRuntime` branch is unchanged.
- Verify: `cargo check`, `./build e2e light`, `./build e2e`.

## Implemented: Phase C — route `MutagenNetworkManager` through the provider

- `src-tauri/src/network/mod.rs`: the module-private `podman()` free function is gone; `expose_port`,
  `unexpose_port`, `close_sandbox`, and `scan_listening_ports` call `provider.exec` (the `socat`,
  `pkill`, `start`, and `cat /proc/net/tcp*` invocations) via a per-call `provider_on(connection)`
  helper mirroring `MiseRuntime::provider_on`. `MutagenNetworkManager` holds an `Arc<dyn
  ProcessRunner>` rather than a single pinned provider, since each method drives an explicit
  `connection`/`entry.connection`.
- `default_network_manager` takes the `runner` and injects it into the production branch;
  `MockNetworkManager` is unchanged.
- Verified: `cargo check` passes.

## Implemented: Phase D — route `credentials` and `agents` through the provider

- `src-tauri/src/credentials/mod.rs`: `tar_capture`, `tar_inject`, and `write_file_via_stdin` dropped
  their `Command::new("podman")` calls for `provider.exec` (the pre-`start`) and `provider.exec_piped`
  (the piped tar/stdin body). Their signatures take `&Arc<dyn MachineProvider>` in place of
  `connection: &str`.
- `src-tauri/src/agents/mod.rs`: `start_auth` builds its interactive `podman exec -i` child from
  `provider.exec_command(...)` and attaches its own piped stdio, keeping the `AuthSessionMap` /
  streaming logic. Signature takes `&Arc<dyn MachineProvider>`.
- `src-tauri/src/lib.rs`: the command handlers calling these functions pull `Arc<dyn MachineProvider>`
  from managed state and pass it through.
- Verified: `cargo check` passes. A manual credential-capture / agent-auth exercise on real Podman is
  still recommended before release, since neither streamed path is covered by the light tier.

## Phase E onward: adding `MsbProvider` as a second, user-selectable provider

Phases A–D (above) extracted the seam and left `PodmanProvider` as the only implementation, with no
user-visible change. This section is the build plan for the follow-on work `01_ideas.md`'s Proposed
Model describes and that document's Open Questions left undecided: making microsandbox an actual
second `MachineProvider` implementation a user can select. It continues `01_ideas.md`, not `02_dev.md`
Phases A–D — where the two conflict on a mechanism, this section is authoritative because it accounts
for what Phases A–D actually built (`InstanceSpec`/`PublishModel`, the `PortTransport` seam,
`SshAlias::Proxied`), some of which already generalizes further than `01_ideas.md` anticipated.

### Resolved Decisions (restating `01_ideas.md`'s Open Questions)

1. **Whether the provider ships user-visible in v1.** Resolved: yes. The disk-volume-plus-Mutagen-sync
   workspace design (already chosen in `01_ideas.md`) works around both open upstream bugs rather than
   waiting on them, so there is no technical reason to hide the provider. It ships as a selectable
   `default_provider` (Phase J) with no separate settings flag gate.
2. **Whether the base image's Podman dependency is acceptable for v1.** Resolved: yes, accepted as-is.
   `msb load` importing a `podman save` tar (Phase G) ships in v1; registry publication stays deferred
   in `04_improvements.md`. A microsandbox-only host (no working Podman machine) cannot build the base
   image in v1 — surfaced to the user as a specific error, not a silent hang.
3. **How `listen_events` behaves for microsandbox.** Resolved: `ProviderCapabilities.event_stream` is
   `false` for microsandbox and `listen_events` stays a Podman-only `SandboxBackend` method (unchanged
   from Phases A–D). No polling fallback in v1 — the activity feed simply stays quieter for a
   microsandbox sandbox. Recorded as a candidate improvement in `04_improvements.md`, not built here.
4. **How the paseo and zellij daemons start without a detached exec.** Resolved: `MsbProvider::exec`
   satisfies `ExecOpts.detached` by backgrounding inside the guest shell (`bash -c '<cmd> >/tmp/out.log
   2>&1 & disown'`), matching what the spike observed surviving the parent exec's exit. Must be `bash
   -c`, not `sh -c`: this image's `/bin/sh` is `dash`, which has no `disown` builtin and exits 127
   before backgrounding is confirmed, even though the `&` itself still detaches the process
   successfully — confirmed hands-on 2026-08-11, see the addendum in
   `spike/hands-on-cli.md`. This is a weaker guarantee than Podman's `-d` (no engine-level supervision
   of the backgrounded process), noted as a gotcha below.
5. **Which status vocabulary reaches `translateStatus()`.** Resolved: `MsbProvider::list_instances`
   normalizes microsandbox's seven PascalCase states onto the five lowercase values
   `translateStatus()` (`src/lib/sandbox.ts`) already handles, rather than adding new
   `status_<value>` keys to every locale file. Mapping: `Running`→`running`, `Stopped`→`exited`,
   `Created`→`created`, `Paused`→`paused`, `Draining`→`running` (still reachable, still winding down),
   `Starting`→`created` (not yet reachable, matches Podman's pre-run state), `Crashed`→`exited` (closest
   existing value; the frontend cannot currently distinguish a clean stop from a crash for either
   provider).
6. **How `Sandbox.image_version` is derived.** Resolved: unchanged from Phases A–D for the `msb
   load`-tagged path — `parse_image_version` maps over the full tag `msb list --format json` reports.
   The SDK snapshot path stays out of scope (Technology Choices in `01_ideas.md` already rejected it).
7. **How much of the refactor lands before the first microsandbox code.** Resolved by Phases A–D
   already landing as their own reviewed, Podman-only body of work (see above); this section is the
   "first microsandbox code" that follows.
8. **How Mutagen handles the `ProxyCommand` alias and host-key prompts.** Resolved: reuse
   `SshAlias::Proxied` as built in Phases A–D — it is already provider-agnostic (an arbitrary
   `command` string, a per-alias `known_hosts` path, `StrictHostKeyChecking accept-new` baked into
   `render_block`). No new `SshAlias` variant is needed. Its `known_hosts` is a per-sandbox file the
   app manages exactly like the podman-exec-transport alias's, so the first connection's host key is
   pinned rather than prompted for or silently trusted forever.

   **Corrected 2026-08-14, after the first real sync attempt.** The alias's `command` was originally
   `msb ssh serve <name> --stdio`, pointing at microsandbox's own SSH server. That server answers
   exec and pty channels but silently drops a `subsystem sftp` request, and Mutagen installs its
   remote agent with `scp`, which speaks the SFTP protocol on OpenSSH 9+. Every sandbox create
   therefore died at `unable to connect to beta: ... unable to install agent: unable to copy agent
   binary: unable to run SCP process: exit status 1`, while a plain `ssh <alias> <cmd>` kept working
   — so nothing short of a real sync exercised the difference. The `command` is now
   `msb exec --stream <name> -- mio-sshd`: microsandbox boots the same base image Podman does, and
   that image's `sshd` already configures `Subsystem sftp` (`src-tauri/resources/Containerfile`), so
   the two providers' aliases now differ only in exec substrate. Confirmed hands-on against real
   `msb` 0.6.6: `ssh` and `scp` both exit 0 over the new `ProxyCommand`, `scp` exits 1 over the old
   one, and a full `mutagen sync create` + `flush` round trip lands a host file in the guest's
   `/root/work`.

   Two consequences follow from the switch. First, the key install is no longer `msb ssh authorize`
   (that authorizes against microsandbox's own server, now unused) but the same provider-neutral
   `install_sandbox_key` the Podman path uses, writing the pubkey to the `AuthorizedKeysFile` the
   image's `sshd_config.container` names — so `backends::create` no longer branches on provider for
   the key at all. Second, the old transport's login cwd was the sandbox's `/root/work`, so a failed
   agent install left a ~8 MB `.mutagen-agent*` blob **inside the synced tree**, which then synced
   back to the user's laptop; `mio-sshd` logs in at `/root` and installs to `/root/.mutagen/`,
   outside it.

Two mechanisms Phases A–D built turn out to already generalize past what `01_ideas.md` assumed, so the
phases below build less new surface than `01_ideas.md` implies:

- **`PublishModel::ExecTunnelled` already fits microsandbox exactly**, because `01_ideas.md`'s
  superseding note (`mutagen-transport/02_dev.md`) replaced microsandbox's original publish-based port
  model with the same "dial the guest directly over SSH, no publish step" shape Podman's own
  exec-transport (`docs/features/podman-exec-transport/02_dev.md`) already uses. `MsbProvider` always
  requests `ExecTunnelled`; `PublishRelay` (and the relay-pool `socat` machinery in `network/mod.rs`)
  stays Podman-only and is never constructed for a microsandbox sandbox.
- **`SshAlias::Proxied` already fits microsandbox exactly** (see Resolved Decision 8) — no `ProxyCommand`
  variant needs adding, contrary to `01_ideas.md`'s Proposed Model, which was written before Phases A–D
  built `Proxied` for the unrelated podman-exec-transport feature.

### `ProviderCapabilities`: added to `MachineProvider` (`src-tauri/src/provider/types.rs`)

```
pub struct ProviderCapabilities {
    pub remote_targets: bool,
    pub event_stream: bool,
    pub host_bind_mount: bool,
}
```

A `capabilities(&self) -> ProviderCapabilities` method on `MachineProvider`, called once (not probed
per-operation) since only `host_bind_mount` is genuinely runtime-dependent (a future `msb doctor`-style
probe once upstream issues #1218/#1219 close); the other two are provider-constants for v1.
`publish_after_create` and `loopback_publish` from `01_ideas.md`'s Proposed Model are **not** added:
Phases A–D's `PublishModel::ExecTunnelled` already makes both moot for both providers — neither
provider's `create` call site needs to ask "can I publish after create" or "does a loopback bind
reach the host" once exec-tunnelling is the only model either provider constructs against.
`PodmanProvider::capabilities()` returns `{ remote_targets: true, event_stream: true, host_bind_mount:
true }`; `MsbProvider::capabilities()` returns `{ remote_targets: false, event_stream: false,
host_bind_mount: false }` (constant in v1; see Resolved Decision 1 on why the bind-mount gap doesn't
block v1 despite the constant `false`).

Command handlers in `src-tauri/src/lib.rs` and `src/routes/projects/+page.svelte` consult
`remote_targets` to hide the deploy picker for a microsandbox sandbox (Phase J), matching the gotcha
`01_ideas.md` already names.

### `MachineRef`: replacing the untyped `connection` string (`src-tauri/src/provider/types.rs`)

```
pub enum ProviderId { Podman, Microsandbox }
pub struct MachineRef { pub provider: ProviderId, pub id: String }
```

Serialized as a prefixed string (`podman:mio-tests`, `msb:local`) via custom `Serialize`/`Deserialize`
(`FromStr`/`Display`), so `Deployment.host` (`src/lib/sandbox.ts`), `ForwardInfo.connection`, and any
persisted `.mio/` JSON stay plain strings across the IPC boundary and on disk — no frontend type
changes beyond a parse helper. Every signature currently taking `connection: &str` — `RuntimeManager`,
`NetworkManager`, `MutagenNetworkManager::provider_on`, `MiseRuntime::provider_on`,
`sync::forward_create` — takes `&MachineRef` instead. A `MachineRef` resolves to a provider instance via
a new `provider_for(&MachineRef) -> Arc<dyn MachineProvider>` constructor in `src-tauri/src/provider/mod.rs`
that matches on `provider` and dispatches to `default_provider` (Podman) or the microsandbox
constructor (Phase G), replacing today's single managed `Arc<dyn MachineProvider>` in `lib.rs` with a
resolver function called per-operation. This is the phase with the widest blast radius (touches every
call site enumerated in `01_ideas.md`'s Current State under "The `connection` string") and is
sequenced last (Phase J) so it lands once both providers already work end-to-end behind their own
pinned-connection constructors.

## Implemented: Phase E — `ProviderCapabilities` and `MachineRef` foundational types

- `src-tauri/src/provider/types.rs`: added `ProviderCapabilities`, `capabilities()` on the
  `MachineProvider` trait, `ProviderId`, and `MachineRef` (with `FromStr`/`Display` for the
  `provider:id` string form, plus unit tests for the round-trip and the three rejection cases —
  unknown tag, missing separator, empty id). No call site changes — `MachineRef` stays inert
  (`#[allow(dead_code)]`, matching the codebase's existing convention for a deliberately-
  unconstructed variant like `SyncStrategy::Volume`) until Phase J switches signatures onto it.
- `src-tauri/src/provider/mod.rs`: `PodmanProvider::capabilities()` returns `{ remote_targets: true,
  event_stream: true, host_bind_mount: true }`.
- No user-visible change; no new host-global state. Verified: `cargo check`, `cargo test --lib
  provider::` (4 passing), `./build e2e light` (11/11 specs passing).

## Implemented: Phase F — `msb` as a second scripted process (`src-tauri/src/mockctl/`)

- `src-tauri/src/mockctl/types.rs`: `SystemMsbRunner` and `ScriptedMsbRunner` (with a
  `TrackedMsbInstance` sibling of `TrackedContainer`) join the existing runner pairs.
  `ScriptedMsbRunner`'s tracked default emits `msb ls --format json` shapes: `name`, `status`
  (PascalCase `Running`/`Stopped`), `created_at`, `image` — matching the spike's confirmed four-key
  shape. All three new types are `#[allow(dead_code)]` (unconstructed until Phase G's `MsbProvider`
  wires them into `lib.rs`), matching the codebase's convention for a deliberately-unconstructed
  variant.
- `src-tauri/src/mockctl/mod.rs`: `msb_command()` (a `podman_command()` sibling — `msb` is never
  bundled, so it just runs `msb` off `PATH`, honouring an `MSB_PATH` override), `derive_msb_key`
  (depth-1 for every call issued so far: `msb.create`, `msb.ls`, `msb.start`, `msb.stop`, `msb.rm` — a
  two-word subcommand like `msb ssh serve` gets its own case when Phase G first issues one, mirroring
  `derive_podman_key`'s special cases), and `ScriptedMsbRunner`'s
  `track_create`/`set_running`/`track_remove`/`tracked_ls` methods, one-to-one with
  `ScriptedPodmanRunner`'s. `KNOWN_CALLS` gains the five `msb.*` keys above; Phase G extends it as
  `MsbProvider` adds `exec`/`inspect`/`load`.
- `e2e/lib/run-identity.ts`: added `MSB_HOME` (`RUN_SLUG`-keyed alongside `APP_DATA_ROOT`/
  `WORKSPACE_ROOT`) and `MSB_PATH` (an `MSB_PATH` env override if already set, else
  `~/.microsandbox/bin/msb.exe` — the executable path, not the directory holding it; see "E2E
  isolation" in `01_ideas.md`). `e2e/wdio.conf.ts` sets both at module scope alongside
  `SANDBOX_WORKSPACE_ROOT`, for every tier rather than gated on one. `MSB_HOME` is read only by the
  `msb` binary itself (upstream `resolve_home()`); `MSB_PATH` is read by both `msb` and
  `msb_command()`, which is why the two ends must agree on the executable-path shape.
- `MIO_MACHINE_PROVIDER` (Phase J's `provider_for` default-resolution steer) is **not yet added** —
  deferred to Phase J since nothing consults it until `provider_for` exists.
- Verified: `cargo check`, `./build e2e light` (11/11 specs passing — no new specs added this phase;
  `KNOWN_CALLS` accepting `msb.*` is exercised once Phase G's provider issues real calls under mock).

## Implemented: Phase G — `MsbProvider`: base image, lifecycle, exec (`src-tauri/src/provider/msb.rs`)

- New `src-tauri/src/provider/msb.rs`: `MsbProvider` holding an `Arc<dyn ProcessRunner>` (the
  `msb.*`-keyed one from Phase F) and an `Arc<dyn MachineProvider>` for the already-pinned local
  `PodmanProvider`, used only by `ensure_base_image`'s Podman-build half. No pinned connection of its
  own (microsandbox has no remote-target concept — `capabilities().remote_targets` is `false`).
- `ensure_base_image`/`base_image_ready`: the `podman save` / `msb load --input <tar> --tag
  sandbox-mio/base:<BASE_IMAGE_VERSION>` pipeline from `01_ideas.md`'s Proposed Model, hand-verified
  against real `msb` 0.6.6 (`spike/hands-on-cli.md` addendum, 2026-08-11). Calls
  `self.podman.ensure_base_image()` (the shared provider's mockable build path) then spawns `podman
  save --output <tmp-tar> <tag>` directly via `podman_command()` (bypassing the runner, like
  `exec_piped`/`exec_command`) before `msb load`ing the tar. Under `MIO_MOCK_BACKEND=1` this skips
  straight to a mocked `msb load` call instead of spawning the real `podman save` process — the
  `mock_backend_enabled()` gate `src-tauri/AGENTS.md`'s gotcha table calls for on any spawning method,
  since nothing else guards this path from a real process under mock. Failures during the Podman half
  surface as the new `ProviderError::PodmanUnavailable` variant, so the "v1 accepts the Podman
  dependency" decision (Resolved Decision 2) fails loudly instead of hanging on a build that can never
  succeed. `base_image_ready`/`image_exists` parses `msb image ls --format json` and compares its
  `reference` field to the tag exactly. Both the `--format json` flag and the `reference` field were
  verified against real `msb` 0.6.6 (2026-08-13); comparing exactly rather than substring-matching the
  plain-text listing is what keeps `sandbox-mio/base:70` from satisfying a check for
  `sandbox-mio/base:7`.
- `create_instance`: creates a `--kind disk` named volume (`<name>-ws`, 20G capacity, sparse) via `msb
  volume create`, then `msb create <image> --name <name> --mount-named <vol>:/root/work:kind=disk,
  size=20G --label <k>=<v> ...` — the real argv confirmed hands-on against `msb` 0.6.6 (see the spike
  addendum), differing from this section's original sketch (`-w`/`--tag` were guesses). The volume is
  created before the instance and rolled back if `msb create` fails, and an already-existing volume is
  tolerated rather than fatal: `msb volume create` exits 1 with `volume already exists: <name>` on a
  taken name (confirmed hands-on, 2026-08-11), so a volume stranded by an interrupted create would
  otherwise make every later create of that sandbox name fail on an unrelated step. Reuse is safe —
  `remove` deletes an instance's volume, so a surviving one never belongs to a live instance, and the
  workspace's contents come from the Mutagen sync (Phase H), not the volume. Always builds
  for `PublishModel::ExecTunnelled` and rejects `PublishRelay` with a `ProviderError` —
  `backends::create`'s `publish_model(exec_capable)` helper (`src-tauri/src/backends/mod.rs`) still
  needs to account for provider, not just image version, once Phase J's per-sandbox provider selection
  lands (a microsandbox sandbox is always exec-capable by construction, independent of
  `image_version`).
- `list_instances`/`start`/`stop`/`remove`: map `msb ls --format json` / `msb start` / `msb stop` /
  `msb rm --force` onto `Sandbox`, applying the status-vocabulary mapping from Resolved Decision 5.
  Labels are not in `msb ls` output (per the spike), so `list_instances` calls `msb inspect --format
  json` once per instance to recover `WORKSPACE_LABEL` from `active_config.labels` (hands-on-confirmed
  path, not the doc-only spike's `config.spec.labels` guess) — the N+1 cost `01_ideas.md`'s Technology
  Choices already accepted. `remove` also best-effort removes the workspace volume; a failure there
  doesn't fail the whole remove, since the instance is already gone either way.
- `exec`/`exec_piped`/`exec_command`: `msb exec <name> -- <argv>`, with `ExecOpts.detached` satisfied
  by backgrounding inside the guest shell via `bash -c '<cmd> >/tmp/out.log 2>&1 & disown'` — **must be
  `bash -c`, not `sh -c`** as Resolved Decision 4 originally read; this image's `/bin/sh` is `dash`,
  which has no `disown` builtin and exits 127 even though the `&` itself still backgrounds
  successfully (hands-on-confirmed 2026-08-11, corrected in Resolved Decision 4 above). `exec_piped`/
  `exec_command` spawn `msb exec --stream` directly via a new `pub(crate) fn msb_command()`
  (`src-tauri/src/mockctl/mod.rs`), mirroring `PodmanProvider`'s direct-command path.
- `copy_in`: implemented as `exec_piped` with `sh -c 'mkdir -p "$(dirname <dest>)" && cat > <dest>'`
  fed the host file's raw bytes, **not** the tar-receiver shape this section originally sketched.
  `credentials::tar_inject`'s tar receiver exists because `tar_capture` produces a multi-file tar
  archive to round-trip; every `copy_in` call site here copies exactly one file, so building a new tar
  archive from scratch would need a new crate dependency for no behavioural gain over a direct byte
  stream. `mkdir -p` first is necessary because `cat >` (unlike `podman cp`) won't create a missing
  parent directory, which `PASEO_PROJECTS_FILE`'s destination needs on a fresh instance.
- `src-tauri/src/mockctl/mod.rs`: `derive_msb_key` gained the `volume`/`image` two-word special case
  (mirroring `derive_podman_key`'s `machine`/`system connection`/`image`); `ScriptedMsbRunner` gained
  `tracked_inspect` reporting the label `track_create` recorded, at the same `active_config.labels`
  path real `msb inspect` uses. `KNOWN_CALLS` gained `msb.exec`, `msb.inspect`, `msb.load`,
  `msb.volume-create`, `msb.volume-rm`, `msb.image-ls`, and `podman.save`.
- `src-tauri/src/provider/types.rs`: `ProviderError` gained the `PodmanUnavailable` variant described
  above.
- Not wired into `lib.rs`: Tauri can only manage one `Arc<dyn MachineProvider>` of state, and the
  existing one is Podman's — constructing and selecting between two is exactly Phase J's `provider_for`
  resolver (see that section above). `SystemMsbRunner`/`ScriptedMsbRunner`/`MsbProvider` and friends
  stay `#[allow(dead_code)]`-marked and unconstructed outside this phase's own unit tests until then;
  this corrects Phase F's comments, which had assumed Phase G itself would remove those markers.
- Verify: `cargo check`, `cargo test --lib provider::` (9/9 passing, including 5 new `MsbProvider`
  tests: status-vocabulary mapping, shell-quoting, the detached-exec wrapper string, a full
  create→list→start→stop→remove round trip against `ScriptedMsbRunner`, and the `PublishRelay`
  rejection), `./build e2e light` (11/11 specs, unchanged — no light spec drives `MsbProvider` yet
  since no command/UI reaches it before Phase I/J). The literal "new `msb.*`-keyed light specs
  exercising create→list→start→stop" verification this section originally called for isn't achievable
  yet for the same reason as the lib.rs wiring above: no Tauri command or UI surface exists to drive a
  WebDriver-level spec against `MsbProvider` until Phase I/J land. The unit-test round trip above
  substitutes for it at this phase — real, but not end-to-end through the app.

## Implemented: Phase H — workspace disk volume and Mutagen sync for microsandbox (standalone pieces; not yet wired into `create()`)

Like Phase G, this phase built the concrete pieces but stopped short of wiring them into the live
`create()` path — `PodmanCliBackend::create` is hard-pinned to `self.provider`/`self.connection`
(Podman-specific fields), so it cannot dispatch to a microsandbox sandbox without Phase J's
per-sandbox `MachineRef`/`provider_for` selection landing first. This section's original "Verify"
line already anticipated no `light`-tier coverage for this phase; what's below is real, unit-tested,
`cargo check`-clean code, reachable once Phase J's dispatch calls it — not yet exercised end-to-end.

- `src-tauri/src/backends/types.rs`: `SyncStrategy::Volume` **is a unit variant, not
  `Volume(ConnectionInfo)`** as this section originally sketched — corrected once writing a concrete
  consumer showed the mismatch. `ConnectionInfo` models a direct `host`/`port`/`user`/`identity` SSH
  endpoint (`SyncStrategy::Remote`'s shape); a microsandbox sandbox's sync target has none of that; its
  `beta` URL is just `<name>:<mount>`, resolved entirely through the sandbox's own `SshAlias::Proxied`
  ProxyCommand (see below). Forcing `ConnectionInfo` into this variant would carry four fields nothing
  populates correctly.
- `src-tauri/src/backends/mod.rs`: `ensure_msb_sandbox_alias(name, identity_path)` mirrors
  `ensure_sandbox_alias`'s existing podman-exec-transport path, but its `ProxyCommand` is `msb ssh
  serve <name> --stdio` directly rather than a container-exec tunnel — no new `SshAlias` variant
  needed (Resolved Decision 8; `SshAlias::Proxied` already fits both). `msb_sync_beta(name)` returns
  the `<name>:/root/work` scp-style URL `SyncEngine::create`'s `beta` argument needs — no
  host/port/user, since the alias's `ProxyCommand` already is the full connection. Unit-tested
  (`msb_sync_beta_has_no_host_or_port`).
- `src-tauri/src/sync/mod.rs`: **not changed this phase.** `SyncEngine::create(name, alpha, beta)`
  already takes exactly the `beta` shape `msb_sync_beta` produces — no new sync-engine code is needed,
  only a call site in `create()` (Phase J: `self.sync_engine.create(name, &laptop_dir,
  &msb_sync_beta(name)).await?` alongside `ensure_msb_sandbox_alias`, mirroring how `deploy` already
  calls `write_alias` then leaves the actual `sync create` call to its own caller in `lib.rs`).
- Verify: `cargo check`, `cargo test --lib` (13/13 passing), `./build e2e light` (11/11, unchanged).
  Still not verifiable by `light` (real `msb`/Mutagen sync required) even once wired — needs a manual
  create→edit-on-host→check-in-guest exercise on the real Windows host, and a `standard`-tier e2e spec
  once `03_e2e.md` covers it, exactly as this section originally anticipated.

## Implemented: Phase I — provider-neutral machine readiness (`src-tauri/src/machine/`)

As landed, this phase is **additive rather than a narrowing of `MachineManager`**, which the original
sketch called for. `MachineManager` and `PodmanMachineManager`'s existing methods
(`check_podman_ready`/`start_podman_machine`/`provision_podman`/`reset_stuck_podman_machine` in
`src-tauri/src/lib.rs`) are untouched byte-for-byte, because they back the projects-page recovery gate
(`src/routes/projects/+page.svelte`) and the welcome flow's own `DownloadNecessitiesStep.svelte`, both
verified only by `e2e/test/specs/standard/podman-startup-recovery.spec.ts` against a real machine —
infrastructure this phase has no way to re-run to prove a narrowed trait preserves every existing
signal (button text, activity task id, the `PodmanRebootRequired` distinction). Rewriting that path
without `standard`-tier coverage was judged too risky for a phase whose own Verify line (see below)
was already scoped to `light` plus a manual walkthrough. `04_improvements.md` records the resulting
duplication as a follow-up once `prepare()` gets its own real-infra verification.

- `src-tauri/src/machine/types.rs`: new `SetupStep`, `MachineSetupEvent`, `MachineReadiness`, and the
  `MachineReadinessProvider` trait (`setup_steps()`/`readiness()`/`prepare(Channel)`), mirroring
  `inference::types`'s `SetupStep`/`RuntimeSetupEvent` shape. Implemented by `PodmanMachineManager`
  (same struct as `MachineManager`, both traits side by side) behind the same four steps
  (`wsl-install`/`machine-init`/`machine-start`/`socket-activate`) the sketch named, and by the new
  `MsbMachineManager`.
- New `src-tauri/src/machine/msb.rs`: `MsbMachineManager` reporting three steps — `msb-binary`,
  `hypervisor-platform`, `msb-doctor`. `readiness()` runs `msb doctor` and looks for "ready" in its
  stdout (a runner-call `Err`, e.g. binary not on `PATH`, also reads as not-ready). `prepare()` drives
  `msb doctor --fix` as a single call covering all three checklist rows at once — there is no
  per-row microsandbox command, unlike Podman's four distinct steps — matching HANDOVER.md's "the
  enable-and-no-reboot path the spike confirmed." Each row still reports only what that one call
  proves: spawning the process proves the binary is installed, its exit status reports on the only
  thing `--fix` changes (the Hypervisor Platform feature), and the third row re-probes through
  `readiness()` rather than rubber-stamping itself, since exit 0 is not on its own evidence the host
  ended up ready. A failing row is marked failed and the rows after it stay pending rather than
  showing progress for work that never ran.
- `src-tauri/src/machine/mod.rs`: `MachineRunners` (the `podman`/`msb` `Arc<dyn ProcessRunner>` pair,
  managed once as Tauri state since a plain `Arc<dyn ProcessRunner>` can't be managed twice under two
  meanings) and `readiness_provider_for(ProviderId, &MachineRunners)`, constructing per call like
  `inference::engine_for`.
- `src-tauri/src/provider/types.rs`: `ProviderId` gained `Serialize`/`Deserialize`
  (`#[serde(rename_all = "lowercase")]`) so it crosses IPC as a command parameter; its
  `#[allow(dead_code)]` is gone now that Phase I constructs it for real.
- `src-tauri/src/mockctl/`: `SystemMsbRunner`/`ScriptedMsbRunner`/`TrackedMsbInstance`/`msb_command`/
  `derive_msb_key` lose their "unconstructed until Phase J" markings — Phase I constructs a real `msb`
  runner in `lib.rs::run()` the same way the Podman runner is constructed, independently of Phase G's
  still-dead `MsbProvider`. `KNOWN_CALLS` gains `msb.doctor`; its no-override default reports "Host
  setup is ready" (mirroring `podman.machine-list`'s "already running" default) so an unconfigured
  light spec doesn't land on a setup screen.
- `src-tauri/src/lib.rs`: `msb_runner` construction alongside `podman_runner`; `MachineRunners` managed
  as state; three new commands — `machine_setup_steps`, `machine_readiness`, `prepare_machine` — all
  taking a `ProviderId` parameter, registered in `generate_handler![]`.
- New `src/lib/machine/types.ts`, `src/lib/machine/readiness.ts` (the `invoke()` wrappers), and
  `src/lib/machine/MachineSteps.svelte` (a checklist renderer mirroring `$lib/models/RuntimeSteps.svelte`,
  with a hardcoded id→English-copy map per the i18n-deferral convention). `src/lib/welcome/CreateVmStep.svelte`
  is rewritten against these — accepting an unused-for-now `provider` prop that defaults to `"podman"`,
  ready for Phase J's picker to pass through — while `src/lib/podman/setup.svelte.ts` is **not**
  touched: it keeps backing the projects-page gate and `DownloadNecessitiesStep.svelte` (base-image
  build always needs Podman regardless of provider, per Resolved Decision 2) unchanged.
- Verify: `cargo check`, `cargo test --lib` (13/13 passing), `./build e2e light` (11/11 specs passing,
  unchanged). Manual welcome-flow walkthrough for both providers on the real Windows host is still
  outstanding, as originally scoped — readiness probing is inherently not `light`-tier material, and no
  e2e spec drives `CreateVmStep.svelte` today (confirmed by grep) so there was nothing existing to keep
  green here beyond the `light` tier's general layout/route coverage.

## Implemented: Phase J — provider selection: `MachineRef` threading, settings, frontend picker

- `src-tauri/src/provider/mod.rs`: `ProviderRegistry` (`default_registry(podman_runner, msb_runner)`)
  replaces the single managed `Arc<dyn MachineProvider>`. It caches the local `PodmanProvider` and the
  `MsbProvider` — caching matters, not just for allocation: a fresh `PodmanProvider` per call would
  each hold its own base-image `build_lock` and race the build. `provider_for(&MachineRef)` resolves a
  ref (a non-local Podman id builds a target-pinned provider, as `deploy` always did); `local_ref`,
  `local`, `all`, and `provider_of(name)` cover the rest. `provider_of` asks each provider's
  `list_instances` in turn, Podman first, so the common case short-circuits before microsandbox's
  per-instance `msb inspect`.
- `MSB_LOCAL_ID` is `"local"`: microsandbox runs on the host and has no connection registry, but
  `MachineRef` still needs an id for its `provider:id` string form.
- `src-tauri/src/backends/`: `Sandbox` gains `provider: ProviderId`, set by each provider's own
  `list_instances` rather than remapped by the caller. `SandboxBackend::create`/`ensure_base_image`/
  `base_image_ready` take a `ProviderId`; `list()` unions both providers, treating **only Podman's
  failure as fatal** — a host with no `msb` binary must still see its Podman sandboxes rather than an
  empty list. `start`/`stop`/`destroy` resolve the owner through `provider_of`. `deploy` gained a
  `capabilities().remote_targets` guard (new `BackendError::RemoteTargetsUnsupported`), the backend
  half of the hidden picker.
- **`Sandbox::supports_exec_transport` is now provider-first**: a microsandbox sandbox is
  exec-tunnelled by construction, independent of `image_version` — this is what makes the whole
  transport/alias/port-lookup chain fall out correctly for it with no further branching. `create`'s
  `publish_model` choice is gated the same way, closing the gotcha this document already recorded.
- **Name uniqueness is cross-provider** (`ensure_name_available`, new `BackendError::NameTaken`)
  rather than an extension of `validate_name()` as originally sketched: `validate_name` is a pure
  character-class check reused by `deploy`, and making it async/`&self` would have dragged a list call
  into every caller that only wanted the path-safety gate.
- `src-tauri/src/runtime/`, `src-tauri/src/network/`: `MiseRuntime` and `MutagenNetworkManager` hold
  the registry instead of a pinned provider plus a runner; `reproduce_remote`, `prepare_terminal`,
  `prepare_agent_web`, `expose_port`, and `watch_ports_file` take `&MachineRef`. The name-only runtime
  methods (`install`, `uninstall`, `registry`, `tool_dir`, `exec_in_container`) resolve through
  `provider_of`, which is what makes `mise` work inside a microsandbox sandbox at all.
- `RuntimeManager::local_connection()` is **gone** rather than converted to a `MachineRef` return: the
  local machine depends on the sandbox's provider, which the runtime cannot know, so `lib.rs` derives
  it from `ProviderRegistry::local_ref` instead. `NetworkManager::open_forward`'s third argument was
  renamed `connection` → `alias`, which is what it always held (an SSH alias, not a machine).
  `ForwardInfo.connection` keeps its name but its documented meaning is now split: an alias for
  Terminal/AgentWeb, a `MachineRef` string for `Exposed` — only the latter is ever resolved back to a
  provider (to kill a `socat` relay), and only `expose_port` writes it.
- `src-tauri/src/lib.rs`: `sandbox_exec_capable` became `sandbox_facts` (provider **and**
  exec-capability in one `list()` pass); new `resolve_machine` replaces `resolve_connection`;
  `ensure_forward_alias` branches to `ensure_msb_sandbox_alias` for a microsandbox sandbox. New
  `provider_capabilities` command. The credentials/agents handlers resolve the sandbox's provider from
  the registry instead of pulling one managed provider from state. `start_inference` reads
  `connection_name()` directly now that `local_connection()` is gone — the inference reverse forward
  targets the Podman VM regardless of which provider a sandbox runs on.
- **Microsandbox create path** (Phase H's pieces, finally wired): `create` authorizes the sandbox's
  public key with the new `provider::msb::authorize_ssh_key` free function and writes the
  `ensure_msb_sandbox_alias` ProxyCommand alias. Authorization is **host-wide, not per instance** —
  `msb ssh authorize` accepts no sandbox positional (hands-on finding, `HANDOVER.md`) — and it is a
  free function rather than a `MachineProvider` method because it addresses the microsandbox host, not
  an instance. `create_sandbox` (`lib.rs`) then **starts the instance and creates the Mutagen session**,
  because a microsandbox workspace lives in a disk volume that only exists once synced, and
  `msb ssh serve` cannot answer for a stopped instance. A microsandbox sandbox is therefore *running*
  after create, where a Podman one is `created` — an intended difference, not an oversight.
- **The workspace session is named `workspace-<name>`** (`backends::msb_sync_session`), not the bare
  sandbox name: a session named for the sandbox means "deployed to a remote host" everywhere else
  (`SessionInfo`, `sandbox_deployments`, `undeploy_sandbox`), and a microsandbox sandbox is never
  deployed. `destroy_sandbox` terminates it best-effort.
- `src-tauri/src/settings/`: `AppSettings.default_provider: ProviderId` (`ProviderId` derives
  `Default` → `Podman`, so an existing settings file deserializes unchanged). `settings::load` applies
  the `MIO_MACHINE_PROVIDER` override deferred from Phase F — process-scoped, never written back, and
  documented in the root `AGENTS.md` env table.
- Frontend: new `src/lib/providers.ts` is the single definition of `ProviderId` (`src/lib/machine/
  types.ts` now re-exports it) plus `ProviderCapabilities`, labels, and the `provider_capabilities`
  wrappers. `Project` gains `provider`. `CreateSandboxItem.svelte` has the picker, seeded from
  `defaultProvider` and overridable per sandbox without persisting; `+page.svelte` probes both
  providers' capabilities once and passes the matching one down; `SandboxItem.svelte` **hides** Deploy
  when `remoteTargets` is false (hidden, not disabled — there is nothing the user could do to enable
  it) and shows it while capabilities are still loading, so the common Podman case never flickers.
  Settings gains a default-provider selector. All new copy is hard-coded English with `TODO(i18n)`.
- `src-tauri/src/mockctl/`: `msb.ssh-authorize` added to `KNOWN_CALLS`, with `ssh` joining
  `volume`/`image` as a depth-2 root in `derive_msb_key`. `TrackedMsbInstance` now records the image
  `msb create` was given so `msb ls` reports one that parses into `Sandbox::image_version`.
- New `e2e/test/specs/light/microsandbox-provider-ui.spec.ts` — the first spec to drive `MsbProvider`
  at all, which Phase G's verify note said was impossible until a UI surface reached it. Creates a
  sandbox on each provider and asserts the deploy action is gated per sandbox, not per list.
- **`close_sandbox` gained an ordering fix this phase** (`src-tauri/src/network/mod.rs`), for a
  regression Phase J surfaced rather than introduced. `expose_port` creates the Mutagen forward and
  only *then* records it in the tracked map; `close_sandbox` used to abort the `watch_ports_file`
  poll task first, so an abort landing in that window cancelled an in-flight exposure and stranded a
  live forward the map never learned about — one nothing could terminate afterwards. `close_sandbox`
  now takes `expose_lock` *before* aborting, so an in-flight expose finishes and registers, and a
  queued one is cancelled while still waiting on the lock, before it creates anything. Confirmed by
  instrumenting both ends: the failing run logged `close_sandbox drained=0` half a second after an
  `expose_port` that never reached its record step, while Mutagen still held the forward.
  `e2e/test/specs/standard/expose-port.spec.ts`'s final assertion is what caught it.
- Verified: `cargo check` (clean), `cargo test --lib` (16/16), `pnpm check` (0 errors),
  `./build e2e light` (12/12 including the new spec), `./build e2e` (standard, **15/15**).
- **Outstanding, unresolved: an intermittent `agent-web.spec.ts` failure.** Its first assertion —
  the `Host <sandbox>` block `create` writes to `~/.ssh/sandbox-mio/config` — was absent in 2 of 5
  standard runs, passing in the other 3 and in the one baseline run measured without this phase's
  changes. One baseline sample is too weak to attribute it to Phase J or to clear it; see
  `04_improvements.md` for the evidence gathered and what to instrument next.
- **Also outstanding: the manual microsandbox create→use→destroy walkthrough on the real Windows
  host** — the light tier proves the wiring against `ScriptedMsbRunner`, not against real `msb`, and
  no `standard`-tier spec covers microsandbox yet (`03_e2e.md` does not exist for this feature).

Phase ordering rationale: E and F are additive and risk-free (new types, new mock plumbing, nothing
wired to production paths yet). G and H are where the feature becomes real but stay behind no
user-facing switch, so they are reviewable and independently revertable without touching Podman's path.
I is welcome-flow polish that only matters once G/H work. J is sequenced last because it is the widest,
riskiest change (every `connection: &str` call site) and should only happen once both providers are
independently proven.

## Cross-cutting concerns

- **No new host-global state.** This phase adds a `provider/` module and one managed `Arc<dyn
  MachineProvider>` but no new path, port, bundled binary, environment variable, or external resource,
  so it introduces nothing that could collide across parallel worktree runs. Standard-tier
  serialisation is unchanged.
- **Single argv builder.** After the refactor, exactly one place (`PodmanProvider`) constructs
  `podman` argv for container operations; `podman_command()` stays as the low-level bundled-binary
  resolver it is today, called by the provider and by the still-Podman-only `listen_events` /
  `machine` / `transport` paths.
- **`BackendError` / `RuntimeError` / `NetworkError` / `CredentialError` / `AgentError` unchanged.**
  `ProviderError` converts into each caller's existing error type, so the IPC boundary and
  `translateStatus()` see no new values.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A light spec that exercised `create`→`list` breaks after Phase A | `PodmanProvider` did not route one-shot calls through the injected `Arc<dyn ProcessRunner>`, so `ScriptedPodmanRunner` no longer diverts them | Keep every no-stdin provider call on the runner path; only `exec_piped` / `exec_command` may spawn `podman_command()` directly |
| `network` calls hit the developer's host `podman` instead of the bundled binary | A leftover `Command::new("podman")` was not moved onto `provider.exec` | Route all of `network`'s exec calls through the provider; the bundled binary is resolved inside `podman_command()` |
| `deploy` regresses because the provider targets the wrong machine | `PodmanProvider` is pinned to one connection, but `deploy` / `reproduce_remote` / `remove_remote` operate on an arbitrary target connection | Construct a second `PodmanProvider` pinned to the target connection for remote-target operations, not the app's default provider |
| The interactive agent-auth session no longer streams | `start_auth` was moved onto `exec` / `exec_piped`, which await to completion and cannot keep a live stdin handle | Use `exec_command` to get an unspawned `tokio::process::Command`, attach piped stdio, and spawn it in `agents` as before |
| A future `MsbProvider` cannot express a create flag | `InstanceSpec` leaked Podman argv (`--network pasta`, `--publish`) into the shared shape | Keep `InstanceSpec` provider-neutral (name, image, mount, labels, `PublishModel`); all Podman argv belongs inside `PodmanProvider::create_instance` |
| A backgrounded `zellij`/`paseo` process inside a microsandbox sandbox dies when the triggering `exec` returns | `msb exec` has no `-d`; the guest shell backgrounding wrapper (Resolved Decision 4) has no engine-level supervision, unlike Podman's `-d` | Verify the shell wrapper's survival directly against the real `msb` binary before relying on it in `prepare_terminal`/`prepare_agent_web`; if it proves fragile, `msb create --script` (undecided candidate in `01_ideas.md`) is the fallback |
| `MsbProvider::create_instance` is called with `PublishModel::PublishRelay` and produces nonsense argv | `publish_model(exec_capable)` (`src-tauri/src/backends/mod.rs`) derives the model from image version, not provider, and a microsandbox sandbox has no `image_version` concept the same way | Once Phase J adds per-sandbox provider selection, gate `publish_model` on provider first — microsandbox is unconditionally `ExecTunnelled` — and have `MsbProvider::create_instance` reject `PublishRelay` with a `ProviderError` rather than emitting wrong argv |
| Creating a microsandbox sandbox fails at `msb volume create` with `volume already exists`, or the name is taken by a registered sandbox whose `msb start` fails with `volume not found`, and both keep failing on every retry of that name | A create that failed or was interrupted left leftovers behind: the volume it made a step earlier, and the sandbox record `msb create` registers before the step that fails; `msb volume create` rejects a name that is already taken | `MsbProvider::create_instance` rolls both back when `msb create` fails — record first via `msb rm --force`, then the volume, mirroring `MachineProvider::remove`'s order — and `ensure_volume` treats an `already exists` failure as success; keep both — the rollback is best-effort, so tolerating a leftover is the backstop for an interrupted process |
| Creating a microsandbox sandbox fails with `unable to connect to beta: ... unable to install agent: unable to copy agent binary: unable to run SCP process: exit status 1`, even though `ssh <alias> <cmd>` into the same sandbox works | The alias tunnelled to microsandbox's own `msb ssh serve <name> --stdio` server, which answers exec and pty channels but silently drops `subsystem sftp`; Mutagen installs its agent with `scp`, which speaks SFTP on OpenSSH 9+ | Point the `ProxyCommand` at the base image's own `sshd` instead — `msb exec --stream <name> -- mio-sshd` — which already configures `Subsystem sftp`, and install the pubkey with the provider-neutral `install_sandbox_key` rather than `msb ssh authorize` (which only authorizes against the now-unused microsandbox server). A sandbox created before this fix has neither the new alias nor the key in `/etc/ssh/authorized_keys.container`, so recreate it rather than debugging the auth failure |
| A light spec's microsandbox create hangs or never lists the sandbox after the key install moved onto `copy_in` | `MsbProvider::exec_piped` spawns `msb_command()` directly to attach stdin, so `ScriptedMsbRunner` cannot divert it and the spec drives the host's real `msb` (or an `MSB_PATH` mock that speaks no exec protocol) | Gate `exec_piped` on `mock_backend_enabled()` and fall back to the one-shot runner path, per `src-tauri/AGENTS.md`'s rule for any spawning method; the payload goes nowhere under mock, which is correct — there is no guest to receive it |
| An ~8 MB `.mutagen-agent*` dotfile appears in the user's laptop workspace | A failed agent install over the old `msb ssh serve` transport, whose login cwd was the synced `/root/work`, left the partial copy inside the synced tree for Mutagen to propagate back to alpha | Fixed by the same transport switch — `mio-sshd` logs in at `/root` and installs to `/root/.mutagen/`, outside the synced tree. Delete any leftover blob by hand from both sides; it is inert debris, not a live agent |
| A microsandbox status badge renders untranslated or wrong | `MsbProvider::list_instances` did not map the seven PascalCase `msb` states onto `translateStatus()`'s five existing lowercase keys (Resolved Decision 5) | Apply the mapping inside `MsbProvider::list_instances`, not in the frontend — `Sandbox.status` must already be one of `running`/`paused`/`exited`/`created`/`removed` by the time it crosses IPC |
