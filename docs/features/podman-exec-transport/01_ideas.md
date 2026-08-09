# Podman Exec Transport: Reaching Containers Without Publishing Ports

This document covers replacing Podman's publish-then-forward port model with an SSH session
tunneled through `podman exec`, so Mutagen is the *only* mechanism standing between the Windows
host and a sandbox's services — no `--publish`, no `pasta:--host-lo-to-ns-lo`, no relay-pool
`socat`. It follows directly from `docs/features/mutagen-transport/02_dev.md`, which extracted the
`PortTransport` trait (`src-tauri/src/transport/`) and split `SshAlias` into `Direct`/`Proxied`
(`src-tauri/src/ssh/types.rs`) specifically so a second reachability model could slot in without
touching call sites again. That feature deliberately kept Podman's own behavior unchanged; this one
proposes actually changing it. Relevant files: `src-tauri/src/backends/mod.rs` (`create`, the
`--publish` argv at lines 422–433), `src-tauri/src/network/mod.rs` (`expose_port`'s relay-pool
logic, `RELAY_PORT_POOL_START`/`RELAY_POOL_SIZE`), `src-tauri/src/transport/mod.rs`
(`PodmanPortTransport`), `src-tauri/src/ssh/types.rs` (`SshAlias`), `src-tauri/resources/Containerfile`.

## Current State

**Podman reachability is a two-hop path today.** Hop one, container → Podman machine's own
loopback, is `--publish 127.0.0.1::<port>` at `create` time (`src-tauri/src/backends/mod.rs:422-433`)
plus the `pasta:--host-lo-to-ns-lo` network flag, without which pasta forwards published ports to
the container's `eth0` rather than its loopback (comment at `backends/mod.rs:410-422`). Podman
cannot publish a new port without recreating the container, so a small pool of 8 ports
(`RELAY_PORT_POOL_START`/`RELAY_POOL_SIZE`, `src-tauri/src/network/mod.rs:21-23`) is pre-published
at every `create`, and `NetworkManager::expose_port` bridges an arbitrary user-chosen port to a free
slot via an in-container `socat` relay (`network/mod.rs:206-265`). Hop two, Windows host → machine
loopback, is a Mutagen forward dialing `{machine-alias}:tcp:127.0.0.1:{published_port}`
(`crate::sync::forward_create`, `src-tauri/src/sync/mod.rs:80`) over the real sshd on the Podman
machine, reached via the `SshAlias::Direct` alias `ensure_connection_alias`
(`src-tauri/src/backends/mod.rs:729`) writes to `~/.ssh/sandbox-mio/config`.

**The seams this feature needs already exist, unconstructed.** `PortTransport`
(`src-tauri/src/transport/types.rs`) resolves a container port to a host-reachable port; its only
implementation, `PodmanPortTransport` (`src-tauri/src/transport/mod.rs`), runs `podman port` and
parses the published-loopback line. `SshAlias::Proxied { alias, command }`
(`src-tauri/src/ssh/types.rs`) renders a `ProxyCommand` OpenSSH stanza instead of
`HostName`/`Port`/`User`/`IdentityFile`; nothing constructs it yet. Both were built in
`mutagen-transport/02_dev.md` anticipating microsandbox's guest-direct SSH model
(`docs/features/microsandbox-provider/01_ideas.md`) — this feature is the discovery that Podman can
use the identical shape, via a different `ProxyCommand`.

**`ensure_key`/identity management precedent.** `PodmanCliBackend::ensure_key`
(`src-tauri/src/backends/mod.rs:222`) already generates an ed25519 keypair with `ssh-keygen` for a
remote Podman connection's identity, registered with `podman system connection add --identity`.
This is the existing pattern for "app-managed SSH identity backing a `Direct` alias"; this feature
needs the equivalent for a per-container identity backing a `Proxied` alias.

## Proposed Model

### `ProxyCommand` execs `sshd -i` inside the container over `podman exec`, no network path at all

`ProxyCommand`'s contract only requires a process whose stdin/stdout carry raw bytes to something
speaking the SSH protocol — `podman exec -i` is exactly that pipe, and unlike a TCP-based approach
it needs no listening port, no `--publish`, no network namespace change. Concretely:
`SshAlias::Proxied { alias: <sandbox>, command: "podman --connection <machine> exec -i <sandbox>
/usr/sbin/sshd -i -f /etc/ssh/sshd_config.container" }`, constructed by a new sibling to
`ensure_connection_alias` (`src-tauri/src/backends/mod.rs:729`), one alias per sandbox rather than
one shared per machine connection. Once the outer `ssh`/Mutagen client completes its handshake
through that pipe, its remote environment *is* the container's own netns — `127.0.0.1:<port>` from
that point on is the container's real loopback, with no lookup needed.

### `PortTransport` for Podman becomes the same passthrough microsandbox was going to need

With no publish step, there is nothing for `PodmanPortTransport::resolve_remote_port`
(`src-tauri/src/transport/mod.rs`) to look up — `resolve_remote_port(sandbox, connection,
container_port)` returns `Ok(container_port)` unconditionally, identical to the passthrough
`mutagen-transport/01_ideas.md` sketched for a microsandbox implementation of the same trait. This
is the strongest evidence the `PortTransport` extraction was cut at the right seam: two unrelated
providers converge on the identical trivial implementation once neither publishes ports.

### `expose_port` loses the relay pool entirely

`NetworkManager::expose_port` (`src-tauri/src/network/mod.rs:206`) currently: finds a free
relay-pool slot, starts an in-container `socat` bridging the slot to the target port, looks up the
slot's published host port, then forwards to that. With direct netns reachability, this collapses
to one `crate::sync::forward_create(&self.app, &name, host_port, <sandbox-alias>, target_port)`
call — no slot tracking, no `socat`, no `NoFreeSlot` error path. `RELAY_PORT_POOL_START`/
`RELAY_POOL_SIZE`, `free_slot`, and the relay-specific branch of `close_sandbox`/`unexpose_port`
(the `pkill -f TCP-LISTEN:<port>` cleanup) are deleted.

### A per-container identity, generated at `create` time

Each container needs its own SSH host key (for `sshd -i` to present) and its own client keypair (for
the outer `ssh` process's `IdentityFile`, since `Proxied` carries no `IdentityFile` field today per
`mutagen-transport/02_dev.md` Phase B — this feature would need to fold the identity path into the
`ProxyCommand` string itself, e.g. `sshd -i ... ` reading an `authorized_keys` baked in at build
time against a keypair `create` generates and writes host-side, mirroring `ensure_key`'s
`ssh-keygen -t ed25519 -f <path> -N ""` shape). The private key lives under app data
(`crate::paths::app_data_root`), not `~/.ssh`, since it is per-sandbox, per-worktree-isolatable
state rather than the shared machine-level identity `ensure_key` manages today.

### A capability flag for sandboxes created before this feature ships

`--publish`/`pasta:--host-lo-to-ns-lo` are baked into `podman create`'s argv
(`backends/mod.rs:402-438`); an already-created sandbox cannot gain `sshd`-based reachability
without being recreated, the same constraint the relay pool exists to route around for new ports on
an existing container. `Sandbox` (`src-tauri/src/backends/types.rs`) needs a stored marker — most
naturally the existing `image_version` field already used for `BASE_IMAGE_VERSION` gating
(`ensure_base_image`, `backends/mod.rs`) — so port resolution and alias construction branch per
sandbox: an old sandbox keeps `SshAlias::Direct` + `PodmanPortTransport`'s real lookup, a new one
gets `SshAlias::Proxied` + the passthrough. See Technology Choices for why this could not be
designed away.

## Technology Choices with Tradeoffs

### `sshd -i` over `podman exec` (chosen) vs. a hand-written minimal SSH server vs. keeping publish+relay

Chosen: install `openssh-server` in the base image and exec it in inetd mode (`-i`) through `podman
exec -i`.

- **A hand-written minimal SSH server** (e.g. via a Rust crate like `russh`, run as a local helper
  process that accepts the `ProxyCommand` pipe and turns `direct-tcpip` channel-open requests into
  `podman exec` calls) — rejected: this is the shape `msb ssh serve --stdio` needs for microsandbox
  because there is no existing sshd to reuse inside a microVM guest by default, but Podman containers
  *can* just run real `sshd`. Building and maintaining a hand-rolled SSH server implementation to
  reproduce behavior a battle-tested one already provides is strictly more code and more security
  surface for no benefit specific to Podman's situation.
- **Keep the publish+relay model** (status quo, what `mutagen-transport` deliberately left
  unchanged) — the safe baseline: no base-image change, no per-container identity, no migration
  problem. Revisit if the migration cost (below) turns out to dominate the simplification gained.

### Per-container keypair, generated at `create` (chosen) vs. one keypair baked into the base image

Chosen: generate a fresh ed25519 keypair per sandbox at `create` time, mirroring `ensure_key`
(`backends/mod.rs:222`).

- **One keypair baked into the image, shared by every container built from it** — rejected: a single
  private key compromised from any one container (or from the image layer itself, which is
  world-readable on the host) would authenticate against every sandbox's `sshd`, whereas Podman's
  own connection-level identity is already one key per machine connection, not one for the whole
  fleet of containers on it. The generation cost is one `ssh-keygen` call per `create`, already paid
  for the machine-level identity today.

### Storing the capability flag on `image_version` (chosen) vs. a new dedicated field vs. probing the running container

Chosen: reuse `Sandbox.image_version` (`src-tauri/src/backends/types.rs`), bumped alongside
`BASE_IMAGE_VERSION` when this feature's Containerfile change lands.

- **A new dedicated boolean/enum field** (e.g. `transport: PortTransportKind`) — rejected as
  premature: `image_version` already exists precisely to answer "was this sandbox built from an
  image new enough to have capability X" (`ensure_base_image`'s short-circuit-on-image-exists gotcha,
  root `AGENTS.md` Common Gotchas), and this feature's capability question is exactly that shape. A
  dedicated field would duplicate what `image_version` thresholding already proves, for no
  behavioral difference.
- **Probe the running container** (e.g. `podman exec <sandbox> which sshd`) — rejected: adds a
  round-trip `podman exec` to every port-resolution call this feature is trying to make cheaper (a
  no-op passthrough), and duplicates information `image_version` already carries statically.

### Dropping `pasta:--host-lo-to-ns-lo` for sandboxes on the new transport (chosen, contingent) vs. keeping it unconditionally

Chosen, pending validation: since nothing publishes an inbound loopback port anymore, the flag's
only documented purpose (forwarding a published port to the container's loopback instead of `eth0`,
comment at `backends/mod.rs:410-422`) no longer applies. Outbound connectivity (DNS, package
installs via `mise`) is expected to be unaffected since the flag only changes inbound-forward
behavior, but this is asserted, not yet verified — see Validation.

## Validation

No spike run yet. Needed before committing to `02_dev.md`:

- Confirm `podman exec -i <sandbox> sshd -i -f <config>` actually completes an OpenSSH handshake and
  successfully carries a `mutagen forward create` session end-to-end (proves the core mechanism).
- Confirm outbound container network access (DNS resolution, `mise install` fetching over HTTPS)
  is unaffected by dropping `pasta:--host-lo-to-ns-lo` in favor of default `pasta` networking.
- Concurrency: open several simultaneous forwards for one sandbox (terminal, agent-web, an exposed
  port) and confirm concurrent `podman exec -i sandbox sshd -i` invocations don't serialize, race on
  a shared resource, or destabilize the container — lower-risk than the equivalent microsandbox
  question (`mutagen-transport/01_ideas.md`'s gating spike) since concurrent `podman exec` into one
  container is already exercised today by simultaneous terminal panes, but not yet proven for `sshd
  -i` specifically.
- Confirm `sshd -i`'s process lifecycle under `podman exec` teardown (container stop/destroy, a
  killed forward) leaves no orphaned `sshd` child process the way the relay pool's `pkill -f
  TCP-LISTEN:<port>` cleanup exists to prevent for `socat` today.

## Open Questions

- **Whether the migration capability flag is worth the complexity it reintroduces.** This feature
  exists partly to *remove* a capability-flag-shaped design (the thing `mutagen-transport/01_ideas.md`
  rejected for microsandbox's publish-based plan), but backward compatibility with already-created
  sandboxes forces one back in on the Podman side. `02_dev.md` should decide whether this is
  acceptable as a temporary migration aid (removed once no supported sandbox predates it) or whether
  it's a sign this feature should ship as opt-in per new sandbox rather than a blanket cutover.
- **Where the per-container private key lives and how its path reaches the `ProxyCommand` string.**
  `SshAlias::Proxied` (`src-tauri/src/ssh/types.rs`) has no `identity` field; whether to add one, fold
  `-i <path>` into the `command` string directly, or bake the key reference into the `sshd_config`
  server-side (host-based trust, no client key) is undecided.
- **Whether `sshd_config.container` needs to be baked into the image or generated per sandbox.** A
  static config baked at build time is simpler but can't parameterize per-sandbox authorized keys
  without a second mounted/copied file; `02_dev.md` should resolve which.
- **Whether this replaces or coexists with the relay pool long-term.** If the capability-flag
  migration period is long, `expose_port` may need to support both code paths simultaneously rather
  than deleting the relay pool outright in the same change that adds the new transport.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A forward opens but nothing answers, even though the in-container service is confirmed listening | `SshAlias::Proxied`'s `command` targets the wrong sandbox name, or the sandbox predates this feature and still needs `SshAlias::Direct` + `PodmanPortTransport`'s real lookup | Confirm the sandbox's stored `image_version` (`src-tauri/src/backends/types.rs`) is new enough to have `sshd` installed before constructing a `Proxied` alias for it |
| `podman exec -i <sandbox> sshd -i` hangs instead of completing a handshake | `sshd_config.container` was not installed at the exact path the `ProxyCommand` string references, or `openssh-server` was not installed in the image that built the container | Verify the Containerfile change installs `openssh-server` and bump `BASE_IMAGE_VERSION` per the root `AGENTS.md` Common Gotchas entry for Containerfile changes |
| A sandbox loses outbound network access (DNS, `mise install` failures) after this feature lands | `pasta:--host-lo-to-ns-lo` was dropped without confirming default `pasta` networking still provides egress | Re-add the flag, or a narrower replacement, if the Validation section's outbound-connectivity check was skipped or failed |
| Two forwards for the same sandbox interfere with each other | Concurrent `podman exec -i sandbox sshd -i` invocations were assumed safe without running the concurrency spike in Validation | Run the concurrency spike before relying on more than one simultaneous forward per sandbox |
| A pre-existing sandbox (created before this feature) fails to open any forward after an app upgrade | Its `image_version` predates `sshd` being baked into the image, but alias construction unconditionally chose `Proxied` | Branch alias construction and `PortTransport` choice on the sandbox's stored `image_version`, not on whether the feature flag is globally enabled |
