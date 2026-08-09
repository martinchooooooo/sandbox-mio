# Podman Exec Transport: Build Plan

This is the build plan for `docs/features/podman-exec-transport/01_ideas.md`, replacing Podman's
publish-then-forward reachability model with an SSH session tunneled through `podman exec` for
sandboxes built from a new-enough base image, while leaving already-created sandboxes on today's
publish+relay-pool path indefinitely — there is no forced-recreate flow (`SandboxItem.svelte`'s
`imageOutdated` banner is dismissible, `dismissedVersion` persisted via `get_dismissed_image_version`),
so both reachability models must coexist as permanent product surface, not a temporary migration
shim. **Status: Phases A–F are implemented; the feature is complete.**
`01_ideas.md`'s Validation section blocked committing to a build plan on a spike this
document was first written without a live Podman machine to run — Phase A below records that spike's
actual results, including one real (if minor) finding it surfaced that Phase F now accounts for.

## The Concrete Model

Every sandbox created by a binary with this feature (`BASE_IMAGE_VERSION >= EXEC_TRANSPORT_MIN_IMAGE_VERSION`,
both in `src-tauri/src/backends/mod.rs`) gets, at `create`/`deploy` time: no `--publish` flags at all
(neither the zellij/paseo web ports nor the relay pool), no `pasta:--host-lo-to-ns-lo`, a fresh
per-sandbox ed25519 keypair under `crate::paths::app_data_root(app)/ssh-keys/<name>/`, and its public
key `podman cp`'d into the container as the fixed path `sshd_config.container`'s
`AuthorizedKeysFile` references. Reachability for that sandbox — terminal, agent-web, and
user-exposed ports alike — goes through one `SshAlias::Proxied` block keyed by the *sandbox name*
(`ProxyCommand` execs `podman --connection <connection> exec -i <sandbox> /usr/local/bin/mio-sshd`,
the image's `sshd -i` launcher — Phase B), and `PortTransport` for it is a trivial passthrough: the
container's own loopback *is* what the outer SSH session lands in once the exec pipe completes its
handshake, so `resolve_remote_port` returns `container_port` unchanged.

A sandbox created by an older binary keeps exactly today's path: `--publish`-based reachability,
`SshAlias::Direct` keyed by *connection name* (shared across every sandbox on that connection, as
today), and `PodmanPortTransport`'s real `podman port` lookup. `Sandbox::supports_exec_transport()`
(`src-tauri/src/backends/types.rs`) is the single predicate every call site branches on, backed by
the already-persisted `image_version` field — no new field, no runtime probe.

Because both paths persist indefinitely, `PortTransport` gains a second implementation
(`PassthroughPortTransport`) rather than replacing `PodmanPortTransport`, and `RuntimeManager`/
`NetworkManager` hold both as two named fields, chosen per call by a new `exec_capable: bool`
parameter. The relay pool (`RELAY_PORT_POOL_START`/`RELAY_POOL_SIZE`, `free_slot`,
`expose_port`'s socat relay, the `pkill -f TCP-LISTEN:<port>` teardown) is **not** deleted — it is
`01_ideas.md`'s "Proposed Model" as originally written assumed a one-time cutover, which the
`imageOutdated` banner's dismissibility rules out; see Resolved Decision 1.

Workspace file sync (Mutagen's `alpha`/`beta` directory mirroring for a deployed sandbox) is
untouched by this feature: `deploy`'s connection-level `SshAlias::Direct` write
(`backends/mod.rs:537`) authenticates against the real sshd on the podman machine itself, a
different concern from a *container's* reachability and out of this feature's scope.

## Resolved Decisions

Restating `01_ideas.md`'s Open Questions:

1. **Whether the migration capability flag is worth the complexity it reintroduces.** Yes, and it is
   not a temporary migration aid to be removed later — `SandboxItem.svelte`'s outdated-image banner
   is user-dismissible with no forced rebuild, so a sandbox can run on a pre-feature image
   indefinitely. Both reachability models (`Direct`+`PodmanPortTransport`+relay-pool and
   `Proxied`+`PassthroughPortTransport`+no-publish) ship as permanent, coexisting code paths gated by
   `Sandbox::supports_exec_transport()`. This also resolves Open Question 4 below.
2. **Where the per-container private key lives and how its path reaches the `ProxyCommand` string.**
   `SshAlias::Proxied` (`src-tauri/src/ssh/types.rs`) gains an `identity: String` field alongside
   `command`; `render_block`'s `Proxied` arm emits `IdentityFile`/`IdentitiesOnly yes` in addition to
   `ProxyCommand` — OpenSSH allows both together, since `ProxyCommand` only replaces the transport
   substrate, and the SSH protocol handshake carried over that pipe (including pubkey auth) still
   consults `IdentityFile` normally. The key lives under `crate::paths::app_data_root(app)/ssh-keys/<name>/id_ed25519`,
   not `~/.ssh` — per-sandbox, worktree-isolatable state, per the Parallel-worktree constraints
   checklist.
3. **Whether `sshd_config.container` needs to be baked into the image or generated per sandbox.**
   Baked into the image at build time (static, one path, one `AuthorizedKeysFile` reference for every
   container). Only the *contents* of that authorized-keys file are per-sandbox, written by `podman cp`
   right after `create`/`deploy` succeeds — mirrors the existing `seed_paseo_projects` temp-file +
   `podman cp` pattern in `src-tauri/src/runtime/mod.rs:102`.
4. **Whether this replaces or coexists with the relay pool long-term.** Coexists indefinitely — see
   Resolved Decision 1. `expose_port` (`src-tauri/src/network/mod.rs:199`) keeps its relay-pool
   branch for `!exec_capable` sandboxes and gains a second, simpler branch for `exec_capable` ones.

Two further decisions were forced by implementation rather than restated from `01_ideas.md`:

5. **How the `ProxyCommand` string survives the shell that runs it.** Phase A's spike recorded MSYS
   path mangling as a manual-shell-only concern; it is not. On this project's Windows hosts the
   shell OpenSSH resolves for `ProxyCommand` is MSYS/Git-Bash-derived, and it rewrites the
   POSIX-style in-container argument (`/usr/local/bin/mio-sshd`) into a host path before `podman
   exec` sees it. The suppression variables are `MSYS_NO_PATHCONV=1` and `MSYS2_ARG_CONV_EXCL="*"`.
   The **first attempt prefixed the command string itself** with `env MSYS_NO_PATHCONV=1 … <command>`
   (bare `VAR=val cmd` prefix-assignment does not survive: whatever wraps the string in `sh -c "exec
   <command>"` fails it with `exec: MSYS_NO_PATHCONV=1: not found`). That prefix worked from a Git
   Bash spike but **fails at app runtime**: the wrapping `sh` then tries to `exec env …`, and the
   Mutagen sidecar's PATH does not include the MSYS `/usr/bin`, so `env.exe` is unresolvable —
   `/bin/sh: line 1: exec: env: not found`, which surfaces as Mutagen's opaque `unable to receive
   server magic number: EOF` / `Connection closed by UNKNOWN port 65535`. The spike missed it only
   because a manual Git Bash shell *does* have `/usr/bin` on PATH.
   **The shipped fix sets the two variables as environment variables on the Mutagen sidecar process**
   (`crate::sync::command`, `sync/mod.rs`) instead of prefixing the command string. Mutagen shells
   out to `ssh`, which spawns the `sh -c "exec …"` that does the conversion; all three inherit the
   sidecar's environment, so the variables take effect with no dependency on `env` being locatable,
   and `ensure_sandbox_alias`'s `ProxyCommand` is now a bare
   `<podman.exe> --connection <c> exec -i <name> /usr/local/bin/mio-sshd`. The same MSYS shell also
   corrupts a quoted backslashed `C:\...` argument (observed mangled into a bogus `\?\C:\...`), so
   `podman_literal_path()` forward-slashes the bundled `podman.exe` path the way `workspace_path()`
   already does for mount arguments. **Scope limit:** this env-inheritance fix only reaches clients
   the app itself spawns (Mutagen). A third-party SSH client executing a `Proxied` alias — the
   host-tool-ssh-access case (Claude Desktop, VS Code Remote-SSH, JetBrains Gateway) — does not
   inherit these variables, so a `ProxyCommand` handed to such a client must instead be free of
   MSYS-manglable tokens (reference `mio-sshd` by bare name off the container `PATH`, not by its
   `/usr/local/bin/...` absolute path) rather than rely on the environment. See that feature's dev
   plan before reusing this alias shape.
6. **What else the `Proxied` block must carry beyond `ProxyCommand`/`IdentityFile`.** Two keywords,
   both load-bearing:
   - `User root` — Mutagen's `alias:path` URLs carry no user, so OpenSSH would otherwise offer the
     local Windows account name, which has no entry in the container's `/etc/passwd`. Spelled from
     `CONTAINER_USER` (`backends/mod.rs`), matching the base image's root-only layout.
   - `UserKnownHostsFile <identity dir>/known_hosts` — the sshd host key is baked into the image
     (Resolved Decision 3), so it changes across an image rebuild while the alias name stays fixed.
     Left in `~/.ssh/known_hosts` that reads as a host-key-changed attack and hard-fails a recreated
     sandbox that reuses a name. Pinning it per sandbox lets `create`/`deploy` drop the stale key
     (`forget_sandbox_host_key`) whenever they build a fresh container, and keeps the user's own
     `known_hosts` clean.

## Implemented: Phase A — Validation spike

Files: none (spike only; no production code changed). Run directly against the always-on local
`podman-machine-default` machine (`podman machine list`), reusing a pre-existing manually-built
`ssh-spike` test container (`localhost/sandbox-mio/base:5`, `openssh-server` already installed —
not created by this feature; its `/root/.ssh/authorized_keys` was extended with a throwaway test key
for the duration of the spike and restored to its prior contents afterward) plus one disposable
container (`spike-nopasta`, removed after the run) for the egress check. All four of `01_ideas.md`'s
Validation checks passed:

1. **Handshake, pass.** `ssh -o
   ProxyCommand="<podman.exe> --connection podman-machine-default exec -i ssh-spike /usr/sbin/sshd -i
   -f /etc/ssh/sshd_config" -i <test-key> root@<alias>` completed a full pubkey-authenticated OpenSSH
   handshake through the exec pipe and ran a remote command. Git Bash requires `MSYS_NO_PATHCONV=1`
   (and `MSYS2_ARG_CONV_EXCL="*"` for the inline `-o ProxyCommand=...` string) or it mangles the
   in-container `/usr/sbin/sshd` path into a host path before it ever reaches `podman exec` — worth a
   one-line note if a future spike or dev script is run from Git Bash on Windows again, though this
   only affects manual/dev shells, not the app's own `tokio::process::Command` invocations.
2. **Outbound egress without `pasta:--host-lo-to-ns-lo`, pass.** A container created with plain
   `--network pasta` and zero `--publish` flags resolved DNS (`getent hosts github.com`), fetched
   `https://github.com` (HTTP 200), and completed a real `mise install node@lts`. Confirms the flag's
   removal is safe for `exec_capable` sandboxes.
3. **Concurrency, pass.** Three simultaneous `ssh`-through-`podman-exec` sessions against one
   container completed independently with no serialization, error, or container instability.
4. **Teardown, pass with one finding.** `SIGKILL`-ing the outer `ssh` client cleanly killed the
   in-container `sshd: root@notty` process and its child command — no orphaned *running* process after
   an abrupt forward kill. However, `ps aux` inside the long-lived `ssh-spike` container showed a long
   tail of `[sshd] <defunct>` zombie processes accumulated from every prior graceful session close.
   Cause: the container's PID 1 is `/bin/sh` (`create`'s current argv, unchanged by this feature),
   which never `wait()`s on reparented children, so each completed `sshd -i` invocation's process
   entry lingers as a zombie for the container's lifetime. Zombies cost a process-table slot each but
   no CPU/memory, so this is not a functional blocker for the mechanism — but it is a real, previously
   unasserted accumulation this feature measurably increases the rate of (one `sshd -i` exit per
   terminal/agent-web/exposed-port connection-open, versus zero before). Carried into Phase F below
   rather than gating Phase B–E, since it doesn't invalidate the chosen approach.

## Implemented: Phase B — Containerfile and the capability constant

Files: `src-tauri/resources/Containerfile`, `src-tauri/src/backends/mod.rs`, `src-tauri/src/backends/types.rs`.

- Install `openssh-server` alongside the existing `apt-get install` line (`Containerfile:12-14`).
- Bake a static `/etc/ssh/sshd_config.container` into the image: `AuthorizedKeysFile
  /etc/ssh/authorized_keys.container`, no password auth, listens only on the exec pipe (`-i` mode
  needs no `Port`/`ListenAddress` at all). Generate the host key at build time (`ssh-keygen -A` or an
  explicit `-t ed25519` call) and bake it in too — it identifies the in-container sshd to the client,
  not the other way around, and there is no network path for a third party to intercept regardless of
  whether every container from one image shares a host key.
- Deviation from the original plan: the config also sets `StrictModes no`. The authorized-keys file
  arrives by `podman cp` from a Windows temp file and lands mode `0666` (measured), which sshd
  otherwise rejects as too open, refusing pubkey auth with nothing but a log line in the container.
- Deviation from the original plan: the image also bakes `/usr/local/bin/mio-sshd`, a three-line
  launcher that `mkdir -p /run/sshd` then `exec`s `sshd -i -f /etc/ssh/sshd_config.container`, and
  the `ProxyCommand` execs the launcher rather than `sshd` directly. Debian's sshd aborts with
  `Missing privilege separation directory: /run/sshd`, and `/run` is a fresh tmpfs at container
  start, so the directory cannot be baked in — it has to be created per container start. Phase A's
  spike missed this because its `ssh-spike` container had been created by hand with the directory
  already present. Symptom if it regresses: the client reports `Connection closed by UNKNOWN port
  65535` and Mutagen reports `unable to receive server magic number: EOF`, neither of which names
  the real cause; `podman exec <sandbox> /usr/sbin/sshd -t -f /etc/ssh/sshd_config.container` does.
- Bump `BASE_IMAGE_VERSION` (`backends/mod.rs:23`) to `6`. Add `pub(crate) const
  EXEC_TRANSPORT_MIN_IMAGE_VERSION: u32 = 6;` as its own named constant rather than reusing
  `BASE_IMAGE_VERSION` directly at call sites — `BASE_IMAGE_VERSION` will keep bumping for unrelated
  Containerfile changes, and the capability threshold this feature introduces must stay fixed at the
  value it shipped with.
- Add `Sandbox::supports_exec_transport(&self) -> bool` (`backends/types.rs`):
  `self.image_version.is_some_and(|v| v >= EXEC_TRANSPORT_MIN_IMAGE_VERSION)`.
- Verification: `cargo check`; rebuild the base image locally and confirm `sshd_config.container` and
  `authorized_keys.container`'s parent directory exist at the expected paths inside a fresh container
  (`podman exec <sandbox> ls -la /etc/ssh/`).

## Implemented: Phase C — Per-sandbox identity, alias construction, and the new `create`/`deploy` argv

Files: `src-tauri/src/backends/mod.rs` (`create` at line 402, `deploy` at line 505, a new
`ensure_sandbox_alias` sibling to `ensure_connection_alias` at line 729), `src-tauri/src/ssh/types.rs`
(`SshAlias::Proxied`), `src-tauri/src/ssh/mod.rs` (`render_block`).

- `SshAlias::Proxied` becomes `Proxied { alias, command, user, identity, known_hosts }` (the `user`
  and `known_hosts` fields beyond the original plan — Resolved Decision 6); drop its
  `#[allow(dead_code)]` now that this phase constructs it. `render_block`'s `Proxied` arm becomes:
  ```
  Host <alias>
      ProxyCommand <command>
      User <user>
      IdentityFile <identity>
      UserKnownHostsFile <known_hosts>
      IdentitiesOnly yes
      StrictHostKeyChecking accept-new
  ```
- New private helper `fn podman_literal_path() -> String` (`backends/mod.rs`, next to
  `podman_command()`): returns the quoted bundled-`podman.exe` path when `PODMAN_RESOURCE_DIR` is set,
  else the literal string `"podman"` — `podman_command()` itself returns a `tokio::process::Command`
  that can't be embedded in an OpenSSH `ProxyCommand` string, so this is the literal-path counterpart
  needed only for that string.
- New `PodmanCliBackend::ensure_sandbox_identity(name: &str) -> Result<String, BackendError>`
  (mirrors `ensure_key`, `backends/mod.rs:222`): `ssh-keygen -t ed25519 -f
  <app_data_root>/ssh-keys/<name>/id_ed25519 -N ""` if absent, returns the private key path. Needs the
  Tauri `AppHandle` to resolve `app_data_root` — threaded in from `create`/`deploy`, which don't have
  one today (their `SandboxBackend` methods take no `AppHandle`); add `app: &AppHandle` as a
  constructor field on `PodmanCliBackend` alongside `runner`, set once in `default_backend`
  (`backends/mod.rs:80`) from the same handle `lib.rs`'s state setup already holds.
- New `PodmanCliBackend::install_sandbox_key(connection: &str, name: &str, identity_path: &str) ->
  Result<(), BackendError>`: reads `<identity_path>.pub`, writes it to a temp file, `podman cp`s it to
  `<name>:/etc/ssh/authorized_keys.container` — same temp-file-then-`cp` shape as
  `seed_paseo_projects` (`runtime/mod.rs:102`), run right after the container is created (`create`)
  or created+started (`deploy`, which starts the container before this step can `exec`/`cp` into it).
- New `pub async fn ensure_sandbox_alias(name: &str, connection: &str, identity_path: &str) ->
  Result<(), BackendError>` (sibling to `ensure_connection_alias`, same file): writes
  `SshAlias::Proxied { alias: name.to_string(), command: format!("{podman_literal_path} --connection
  {connection} exec -i {name} /usr/local/bin/mio-sshd"), user, identity, known_hosts }` via
  `crate::ssh::write_alias`. Called from `create`/`deploy` once
  (identity + authorized_keys are stable for the container's lifetime, unlike `ensure_connection_alias`'s
  per-call refresh, which exists because a machine's SSH port changes on every boot — a sandbox's own
  exec-transport alias has no such moving part) and again from the `lib.rs` call sites in Phase D,
  since `write_alias` is safe to call repeatedly and the alias file is not guaranteed to survive
  between app runs the way the container itself does.
- `create`/`deploy` additionally call `forget_sandbox_host_key` before installing the key, dropping
  the alias's pinned host key because the container behind that name is new — Resolved Decision 6.
- `create` (`backends/mod.rs:402`): after computing `image`/`workspace` as today, branch the argv
  builder on `BASE_IMAGE_VERSION >= EXEC_TRANSPORT_MIN_IMAGE_VERSION` (always true post-Phase-B, but
  written as a comparison, not an `if true`, so a future revert of the constant falls back cleanly) —
  the `exec_capable` branch omits `--network pasta:--host-lo-to-ns-lo`, all `--publish` args
  (`ZELLIJ_WEB_PORT`, `PASEO_WEB_PORT`, and `relay_pool_publish_args()`), keeping only
  `--volume`/`--workdir`/`--label`/`--name`. After `check(...)` returns the id, call
  `ensure_sandbox_identity`, `install_sandbox_key`, `ensure_sandbox_alias` in sequence before
  returning the `Sandbox`.
- `deploy` (`backends/mod.rs:505`): identical argv branch for the remote `create_args` builder
  (`backends/mod.rs:554-566`); the identity/authorized-keys/alias sequence runs against
  `target_connection` after the remote `start` call (line 574), since `podman cp`/`exec` need the
  container running.
- Verification: `cargo check`; `./build e2e` (`standard`) — this phase's `create`/`deploy` changes are
  real-Podman-only, matching `mutagen-transport/02_dev.md` Phase A's framing for the same reason.

## Implemented: Phase D — `PassthroughPortTransport` and capability-aware selection

Files: `src-tauri/src/transport/mod.rs`, `src-tauri/src/runtime/mod.rs` (`PodmanMiseRuntime`,
`prepare_terminal`, `prepare_agent_web`, `types.rs`'s `RuntimeManager` trait), `src-tauri/src/lib.rs`
(`prepare_terminal_webview`, `prepare_agent_web_pane`).

- `PassthroughPortTransport` (`transport/mod.rs`): `resolve_remote_port` returns `Ok(container_port)`
  unconditionally, ignoring `sandbox`/`connection` — the passthrough `01_ideas.md`'s Proposed Model
  described. No trait change; it is a second implementor of the existing `PortTransport`.
- `PodmanMiseRuntime` (`runtime/types.rs`) replaces its single `port_transport` field with two:
  `published_port_transport: Arc<dyn PortTransport>` (today's `PodmanPortTransport`) and
  `exec_port_transport: Arc<dyn PortTransport>` (`PassthroughPortTransport`). `default_runtime`
  (`runtime/mod.rs:26`) constructs both from `transport::default_port_transport()` /
  a new `transport::default_exec_port_transport()`.
- `RuntimeManager::prepare_terminal`/`prepare_agent_web` (`runtime/types.rs`) each gain an
  `exec_capable: bool` parameter. Inside `PodmanMiseRuntime`'s impls, the trailing
  `self.port_transport.resolve_remote_port(...)` call picks `self.exec_port_transport` or
  `self.published_port_transport` based on the flag; everything before that (starting the container,
  `zellij web`/`paseo daemon` setup over `podman exec`) is unchanged — those calls already work
  identically for a container with no published ports, since they go over `podman exec`, not a
  published loopback port.
- `MockRuntime` (`runtime/mock.rs`) gains the new parameter on its trait impl and ignores it, per
  `mutagen-transport/02_dev.md` Phase A's precedent (the light tier never opens a terminal/agent-web
  pane today).
- `lib.rs`'s `prepare_terminal_webview`/`prepare_agent_web_pane`: both already resolve `connection`
  (the podman connection) before calling into `runtime`. Add a `store: &Arc<dyn SandboxBackend>`
  parameter to both (their callers — `open_terminal`, `open_terminal_pane`, `open_agent_web`,
  `lib.rs:302/328/376` — already hold `store: State<'_, Arc<dyn SandboxBackend>>`), look up the
  `Sandbox` via `store.list()` filtered by `name`, and compute `exec_capable =
  sandbox.supports_exec_transport()`. When `exec_capable`, call
  `crate::backends::ensure_sandbox_identity`/`ensure_sandbox_alias` (re-deriving the identity path
  deterministically from `name`, not persisting it anywhere new) instead of
  `ensure_connection_alias`, and pass `name` — not `connection` — as the `connection` argument to
  `network.open_forward` (that argument is purely the Mutagen `remote_alias`; `open_forward` itself
  never shells out to `podman`, so no `NetworkManager` signature change is needed here). The podman
  connection name keeps flowing into `runtime.prepare_terminal(name, &connection, ...)` unchanged,
  since `podman exec --connection <connection>` still needs it regardless of transport.
- Deviation from the original plan: the lookup and the alias write are two `lib.rs` helpers,
  `sandbox_exec_capable` and `ensure_forward_alias`, rather than inline blocks. Four command paths
  (terminal, terminal pane, agent-web, expose-port) plus `open_workspace` need the same predicate,
  and `ensure_forward_alias` returning the alias keeps "which model" and "which alias" from being
  answerable differently at any one call site.
- Verification: `cargo check`; `./build e2e light` (mocks ignore the new parameter, so light specs
  covering terminal/agent-web open should be unaffected — confirm); `./build e2e` (`standard`) for a
  real round trip on an `exec_capable` sandbox.

## Implemented: Phase E — `expose_port`'s second branch

Files: `src-tauri/src/network/mod.rs` (`MutagenNetworkManager`, `expose_port`, `watch_ports_file`,
`poll_ports_file`, `types.rs`'s `NetworkManager` trait), `src-tauri/src/network/mock.rs`,
`src-tauri/src/lib.rs` (`expose_port`, `open_workspace` commands).

- Deviation from the original plan: `MutagenNetworkManager` does **not** gain a second
  `exec_port_transport` field. Unlike `PodmanMiseRuntime`'s terminal/agent-web ports (which need a
  `PortTransport::resolve_remote_port` lookup even when it is a passthrough), `expose_port`'s
  exec-capable branch has no lookup to make at all — the `ForwardInfo` it inserts records
  `remote_port: target_port` directly. Adding an unused `exec_port_transport` field would be dead
  code; kept only `published_port_transport`, renamed from `port_transport` for parity with the
  `!exec_capable` branch it now exclusively serves.
- `NetworkManager::expose_port` (`network/types.rs`) gains an `exec_capable: bool` parameter.
  `MutagenNetworkManager::expose_port`'s existing body (slot selection, `socat` exec, relay-pool
  `PodmanPortTransport` lookup) becomes the `!exec_capable` branch, unchanged. The `exec_capable`
  branch skips slot selection and the `socat` exec entirely: `check(podman(connection, &["start",
  sandbox]).await?)?;` still runs (the container must be running), then `forward_create(&self.app,
  &name, host_port, sandbox, target_port)` directly — `sandbox`, not `connection`, is the Mutagen
  alias here, matching Phase D's `open_forward` change. The `ForwardInfo` inserted still records
  `remote_port: target_port` (no relay slot exists to record instead).
- Deviation from the original plan: `unexpose_port`/`close_sandbox`'s relay-specific `pkill -f
  TCP-LISTEN:<port>` teardown is gated explicitly rather than left to match nothing. The plan
  proposed a new `ForwardInfo` field if implicit gating proved insufficient; neither was needed —
  `is_relayed` (`network/mod.rs`) derives the answer from state already recorded, since only the
  relay branch stores a `remote_port` differing from the target port. Skipping the call outright
  beats relying on a `pkill` pattern happening not to match a user process in the container.
- Deviation from the original plan: `poll_ports_file`'s auto-detect exclusion of
  `RELAY_PORT_POOL_START..+RELAY_POOL_SIZE` is skipped for an `exec_capable` sandbox. Those ports
  are excluded because a relay `socat` listens on them, which never happens without a relay pool —
  keeping the exclusion would silently refuse to expose a user server that picked one.
- Deviation from the original plan: `expose_port` now returns the existing forward's host port when
  called with `auto_detected: true` for a port already in the registry. `poll_ports_file` chooses
  its scan candidates before taking `expose_lock`, so a manual expose landing in that gap used to be
  overwritten by the auto-detected entry — demoting the row to `Auto :<port>` and losing its "Stop
  exposing" button. The race predates this feature but only became reproducible once the
  `exec_capable` branch dropped the slot lookup and `socat` exec, shortening the window's far side.
- `MockNetworkManager` (`network/mock.rs`) gains the new parameter on its trait impl and threads it
  into its own internal `expose_port` call inside `watch_ports_file`'s poll loop (mirroring the real
  `poll_ports_file`).
- Addition beyond the original plan: `NetworkManager::watch_ports_file` also gains an
  `exec_capable: bool` parameter (both the trait and `poll_ports_file`), since its internal
  `expose_port` calls (file-declared and auto-detected ports) need to know which branch to take for
  the sandbox being watched — the plan's Phase E text did not cover this call path.
- `lib.rs`'s `expose_port` command: same `store.list()` + `supports_exec_transport()` lookup as
  Phase D, additionally calling `ensure_sandbox_identity`/`ensure_sandbox_alias` when `exec_capable`
  (mirroring `prepare_terminal_webview`/`prepare_agent_web_pane`) before threading `exec_capable`
  into `network.expose_port(...)`. `lib.rs`'s `open_workspace` command (the `watch_ports_file`
  caller) gains `store` and `app` parameters: it needs the same lookup, and it must ensure the alias
  itself, because the poll loop auto-exposes ports without passing through the `expose_port` command
  handler that would otherwise have written it.
- Verification: `cargo check`; `./build e2e light`; `./build e2e` (`standard`) — specifically
  `expose-port.spec.ts` against both an `exec_capable` sandbox and a pre-feature one (create one
  against a pinned older `BASE_IMAGE_VERSION` tag, or skip if no such fixture exists yet, and note the
  gap in `04_improvements.md`).

## Implemented: Phase F — Regression pass and doc handoff

- `agent-web.spec.ts`'s first two tests were rewritten for the new model: instead of asserting a
  `127.0.0.1` binding for the paseo port, they assert the container publishes *nothing*, that
  `create` wrote a `Proxied` alias carrying all four load-bearing keywords, and that the started
  container holds both halves of the handshake (`/etc/ssh/authorized_keys.container` with the
  sandbox's public key, `/etc/ssh/sshd_config.container` from the image). `sshAliasBlock`
  (`e2e/lib/ssh.ts`) reads the managed config, which is host-global — a spec must key on a
  run-unique alias, which `useSandboxWorkspace`'s timestamped project name already gives.
- Done: `cargo check`, `cargo clippy`, `cargo test`, `pnpm check`, `./build e2e light` (11/11), and
  `./build e2e` (`standard`, 15/15 spec files) — `agent-web.spec.ts`, `embedded-workspace.spec.ts`
  and `expose-port.spec.ts` included, per `mutagen-transport/02_dev.md` Phase D's precedent.
- Regression plan written to `03_e2e.md` (the doc handoff this phase owes the pipeline).
- **Resolved — `!exec_capable` path still works end to end (spiked 2026-08-10 against `mio-tests`).**
  A container created with the exact `create` `!exec_capable` argv (`--network
  pasta:--host-lo-to-ns-lo`, `--publish 127.0.0.1::8082/::6767`, the eight relay-pool publishes)
  serves zellij over the publish path: `zellij web --daemonize` binds `127.0.0.1:8082`,
  `podman port <name> 8082/tcp` (the `PodmanPortTransport` lookup) resolves the host-loopback port,
  and a curl to it from the machine returns HTTP 200 — pasta's `--host-lo-to-ns-lo` forwards the
  published loopback port into the container loopback where zellij binds. `expose_port`'s relay
  branch also verified: `socat TCP-LISTEN:<slot>,fork,reuseaddr TCP:127.0.0.1:<target>` on a pool
  slot, reached through the slot's published port (HTTP 200), and torn down by `pkill -f
  TCP-LISTEN:<slot>` (the `is_relayed` branch). The automated-fixture gap for this path in the
  `standard` tier remains, recorded in `04_improvements.md`; this spike is the manual proof the
  publish+relay code did not regress.
- **Resolved — no orphaned *running* `sshd` accumulates on the shipped teardown path (spiked
  2026-08-10).** Driving the bundled Mutagen sidecar
  (`src-tauri/binaries/mutagen-x86_64-pc-windows-msvc.exe`) through the app's exact
  `forward_create`/`forward_terminate` calls against a `Proxied` alias for a base-6 container, over
  five open→verify→close cycles: each forward reached zellij (HTTP 200), held exactly one
  `sshd: root` session while live, and dropped to **zero** running `sshd` after `mutagen forward
  terminate` — Mutagen closes the transport gracefully and the in-container `sshd -i` exits. No
  running-`sshd` accumulation across cycles; the defunct count stayed bounded at ~1, not growing, so
  the reaping-PID-1 mitigation is not needed for the normal path. Left in `04_improvements.md` as a
  candidate anyway, alongside a newly found edge case: an **abrupt** `kill -9` of a raw `ssh` client
  (no graceful disconnect) does *not* tear down a bare `-N` forwarding session's in-container `sshd`
  — `podman exec` does not propagate the client's death and `sshd_config.container` sets no
  `ClientAliveInterval`, so such an orphan persists until the container stops. This refines Phase A's
  too-strong "SIGKILL cleanly kills it" claim (Phase A's SIGKILL target was running a remote command,
  which ends on its own; a forwarding session has nothing to end it) and matters only when the app
  dies abnormally, not on a normal close.
- Parallel-worktree constraints checklist: the per-sandbox SSH key path
  (`app_data_root(app)/ssh-keys/<name>/`, which also holds the alias's pinned `known_hosts`) already
  routes through `crate::paths::app_data_root`, so it honours `MIO_APP_DATA_DIR` per-worktree
  overrides with no extra work. No new host port, no new
  host-global external resource, no new bundled binary. `sshd_config.container`'s host key is baked
  into the image (a new Containerfile artifact, not a `src-tauri/binaries/` bundled binary), so it
  needs no `build.rs` asset group — flag this explicitly if a future revision moves host-key
  generation to the app side instead of build time.

## Lifecycle / Cross-Cutting Concerns

Per-sandbox SSH keys are never cleaned up by `destroy`/`remove_remote` today's plan touches — they
are small, worktree-isolated, and harmless to leave behind the way `~/.ssh/sandbox-mio/config`
already accumulates stale `Direct` blocks for destroyed connections (no existing cleanup precedent
either). If this becomes a problem, add cleanup to `destroy`/`remove_remote`
(`backends/mod.rs:500`/`582`) removing `app_data_root(app)/ssh-keys/<name>/` and calling
`crate::ssh::remove_alias(name)` — note this as a candidate for `04_improvements.md` rather than
building it speculatively now, since nothing currently exercises it and `remove_alias` itself has no
caller yet either.

`Arc<dyn PortTransport>`'s lifecycle is unchanged from `mutagen-transport/02_dev.md`: constructed
once in `lib.rs` setup, held in Tauri-managed state, stateless. The only change is that two instances
now exist per manager instead of one.
