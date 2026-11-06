---
type: ideas
title: "Proxy Remote Deploy: Letting a Proxy-Enabled Sandbox Survive Deploy"
feature: proxy-remote-deploy
---
# Proxy Remote Deploy: Letting a Proxy-Enabled Sandbox Survive Deploy

This document covers lifting `CliBackend::deploy`'s refusal of a proxy-enabled sandbox
(`src-tauri/src/backends/mod.rs`, `BackendError::ProxyDeployUnsupported`) so a sandbox that has
traffic inspection enabled keeps it after being deployed to a remote Podman host. This is the
"Remote-deployed sandboxes keep their proxy" idea `docs/features/network-proxy/04_improvements.md`
deferred out of `network-proxy`'s v1 scope. The `network-proxy` feature itself is **fully
implemented** in code today — despite `docs/features/network-proxy/02_dev.md`'s phase headings
still reading "Remaining" for Phases B through E, every file that document planned exists and is
wired up: `src-tauri/src/proxy/mod.rs`, `src-tauri/src/proxy/rules.rs`, `src-tauri/src/proxy/types.rs`,
`src-tauri/src/commands/proxy.rs` (all six commands registered in
`src-tauri/src/commands/mod.rs`), and the `proxy: bool` parameter threaded through
`src-tauri/src/services/sandbox.rs::create` and `src-tauri/src/commands/sandbox.rs`. Those stale
headings are not authoritative; the actual code under `src-tauri/src/proxy/` is. Relevant existing
files beyond those: `src-tauri/src/backends/mod.rs` (`CliBackend::deploy`, `CliBackend::create`,
`resolve_workspace`, `remote_mkdir`, `ConnectionInfo`/`WorkspacePlan`/`SyncStrategy`),
`src-tauri/src/provider/mod.rs` and `types.rs` (`PodmanProvider`, `MachineProvider`,
`InstanceNetwork`, `SidecarSpec`/`SidecarMount`), `src-tauri/src/services/routing.rs`
(`SandboxRouter`, `resolve_machine`), `src-tauri/src/services/sandbox.rs` (`create`, `destroy`,
`undeploy`), `src-tauri/src/paths.rs` (`app_data_root`), and `docs/features/remote-deploys/01_ideas.md`
(the base-image-on-remote precedent this document's proxy-image analog follows).

## Current State

**The proxy subsystem's low-level primitives already take a provider parameter, not a hardcoded
connection.** `crate::proxy::create_proxy`, `inject_ca`, `remove_proxy`, `apply_rules`, and
`restart_proxy_container` (all in `src-tauri/src/proxy/mod.rs`) each accept `provider: &Arc<dyn
MachineProvider>` as an argument rather than resolving one internally. Every *caller* of these
functions, however, hardcodes `registry.local(ProviderId::Podman)` — `src-tauri/src/services/sandbox.rs`
(`create`, `remove_proxy_if_present`) and `src-tauri/src/commands/proxy.rs` (every command). The
provider parameter already being an injected argument, not a baked-in assumption, is what makes this
feature look tractable at the call-site layer: no signature change is needed on the functions
themselves, only on what gets passed in.

**The provider layer's network/sidecar/pull primitives are already connection-generic.**
`PodmanProvider::create_network`, `remove_network`, `create_sidecar`, and `pull_image`
(`src-tauri/src/provider/mod.rs`) all route through `PodmanProvider::run`, which prefixes every
argv with `--connection <self.connection>` — the same mechanism `deploy` already uses today to
build a second, target-pinned provider (`crate::provider::default_provider(self.runner.clone(),
target_connection.to_string())`, `src-tauri/src/backends/mod.rs`). Nothing in the provider layer
assumes "the local machine"; pointing these four methods at a remote connection needs no new code
there.

**The proxy's file-backed state is unconditionally laptop-local, and that is the actual gap.**
`state_dir`, `certs_dir`, `ca_pem_path`, `meta_path`, and `secrets_path` (all in
`src-tauri/src/proxy/mod.rs`) derive every path from `crate::paths::app_data_root(app)` — the
Tauri app's own AppData directory on the machine running the desktop app. `create_proxy` bind-mounts
three of those paths (`certs/`, `addon.py`, `rules.json`) into the sidecar via
`SidecarMount.host_path`. `docs/features/remote-sync/01_ideas.md` already states the load-bearing
fact this collides with: "a Podman bind mount always resolves on the machine where the container
runs, not on the machine running the Tauri app." For a proxy sidecar created on a remote connection,
`SidecarMount.host_path` must name a path on the *remote* host's filesystem, and nothing today
computes one — every existing caller assumes the laptop and the Podman machine are the same
filesystem (true for the local WSL machine, false for a genuine remote host).

**A parallel remote-vs-local split already exists for the sandbox workspace and is the template to
follow.** `resolve_workspace` (`src-tauri/src/backends/mod.rs`) returns a `WorkspacePlan` carrying a
`bind_mount_source` (resolved on the Podman host) distinct from `laptop_dir`, plus a `SyncStrategy`
that is `Local` (equal paths, no sync) or `Remote(ConnectionInfo)`. `remote_mkdir` creates the
remote-side directory over a raw `ssh ... mkdir -p` invocation (`detached_command("ssh")`,
`-o StrictHostKeyChecking=accept-new`, the connection's identity/port/user) before `podman create`
ever runs — this is the shape a remote proxy state directory needs, since it too must exist on the
remote filesystem before the sidecar container that mounts it is created.

**Rule *definitions* already travel to a remote host for free; materialised *secrets* do not.**
`.mio/proxy-rules.json` (`src-tauri/src/proxy/rules.rs`, `load_rules`/`save_rules`) lives inside the
sandbox's own workspace directory, which the existing Mutagen sync session already mirrors to the
remote once deployed (`docs/features/remote-sync/01_ideas.md`). The *materialised* `rules.json`
with real secret values, by contrast, is written only to `<app_data_root>/proxy/<sandbox>/rules.json`
(`write_materialised_rules`, `src-tauri/src/proxy/mod.rs`) — a laptop-only path, and specifically
*not* synced (Resolved Decision 4, `docs/features/network-proxy/02_dev.md`: rule definitions carry a
secret *id* only; the workspace is bind-mounted into the sandbox and must never hold a real value).
The raw value store, `<app_data_root>/proxy/secrets.json` (`secrets_path`, same file), is app-global
across every sandbox, not per-sandbox — it holds every proxy-enabled sandbox's secret values in one
`HashMap<String, String>` keyed by `secret_id`.

**Proxy teardown is not wired into every path that removes a sandbox.** `services::sandbox::destroy`
(`src-tauri/src/services/sandbox.rs`) has two branches: a sandbox with no Mutagen session (never
deployed) calls `remove_proxy_if_present` before returning; a sandbox *with* a session (deployed)
calls `store.remove_remote` then `store.destroy` and never calls `remove_proxy_if_present` at all.
Today this is harmless — `deploy` refuses a proxy-enabled sandbox outright, so no deployed sandbox
can have a proxy sidecar to clean up. The moment `deploy` stops refusing, this branch becomes a real
leak: destroying a deployed, proxy-enabled sandbox would strip the container and Mutagen session but
never call `remove_proxy`, leaving both the local (stopped) and any remote proxy sidecar orphaned.

**`SandboxRouter` already resolves which connection a sandbox's operations should address, deployed
or not.** `SandboxRouter::locate`/`resolve` (`src-tauri/src/services/routing.rs`) computes a
`MachineRef` via `resolve_machine`: it checks the sync engine's live sessions for a deploy target
matching the sandbox's name and falls back to the local machine when none exists. `commands/proxy.rs`'s
commands do not go through this router today — they call `registry.local(ProviderId::Podman)`
directly — but the router is the existing, already-correct answer to "which connection is
authoritative for sandbox X's containers right now," which is exactly the question proxy commands
need answered once a proxy can live on either side of a deploy.

**The mitmproxy image is pulled, never built, so remote provisioning does not need
`docs/features/remote-deploys/01_ideas.md`'s Containerfile-build workaround.** `ensure_proxy_image`
(`src-tauri/src/proxy/mod.rs`) calls `provider.pull_image(&image)`, and
`docker.io/mitmproxy/mitmproxy` is a public image on a public registry — unlike the sandbox base
image (`sandbox-mio/base:mise`), which `remote-deploys/01_ideas.md` documents as private and
therefore requiring an on-remote build (`ensure_image_on`). A remote connection's `podman pull` of
the mitmproxy image needs only outbound registry access, which the remote host already needs for
`mise install` during `reproduce_remote` (`src-tauri/src/runtime/mod.rs`).

**The WSL-specific fact that shaped the local network design does not obviously carry over, and
should not be assumed either way.** `docs/features/network-proxy/02_dev.md`'s Spike Results found
`aardvark-dns` cannot start on this project's local Podman machine because its WSL distro runs no
`systemd --user` session for the `user` account (`/run/user/1000/bus` absent) — the reason every
proxy network is created `--disable-dns` with a statically pinned IP instead of relying on
container-name resolution. A genuine remote host (per `docs/features/cloud-sandboxes/01_ideas.md`,
a bare Ubuntu/Debian cloud VM or any BYO SSH-reachable Linux box) is a different situation on paper:
`prepare_host` (`src-tauri/src/backends/mod.rs`) already runs `sudo loginctl enable-linger "$USER"
&& XDG_RUNTIME_DIR=/run/user/$(id -u) systemctl --user enable --now podman.socket` on every remote
host this app registers, which is precisely the systemd-user/lingering setup the WSL machine lacks.
This does *not* mean DNS-enabled bridge networking would work remotely — it has not been tested, and
the design below does not depend on it either way (see Validation). It does mean the local spike's
finding is a fact about this project's specific local WSL machine, not a general fact about
rootless Podman, and should not be read as a blanket "bridge networking is broken under rootless
Podman."

## Proposed Model

### Deploy passes an explicit target provider into the existing proxy primitives, unchanged

`CliBackend::deploy` (`src-tauri/src/backends/mod.rs`) already builds `target_provider` via
`crate::provider::default_provider(self.runner.clone(), target_connection.to_string())` for the
sandbox container itself. The same value is the provider argument `create_proxy`/`inject_ca` already
accept — deploy's guard changes from refusing whenever `crate::proxy::exists(&self.registry, name)`
is true to instead reading that as "this sandbox needs a proxy sidecar created on
`target_provider` too," mirroring the sequencing `services::sandbox::create` already uses locally
(`ensure_proxy_image` → `create_proxy` → create the sandbox container → `inject_ca`), just aimed at
the target connection's provider instead of `registry.local(ProviderId::Podman)`.

### A remote-aware proxy state root, mirroring `WorkspacePlan`/`resolve_workspace`

`crate::proxy::state_dir` and its dependents (`certs_dir`, `ca_pem_path`, `meta_path`) gain a
remote-side counterpart the same way `workspace_path` (laptop) gained `resolve_workspace`'s
`bind_mount_source` (remote). A remote proxy's `certs/`, `addon.py`, and `rules.json` live under a
directory on the remote host's own filesystem — analogous to `<home>/work/<name>` for the sandbox
workspace — created before the sidecar container via the same raw-SSH pattern `remote_mkdir`
established, not via `std::fs::create_dir_all`. Writing `addon.py`'s embedded contents and the
materialised `rules.json` to that remote directory is a small-payload SSH write (pipe the content to
`ssh ... 'cat > <path>'`, the same transport shape `remote_mkdir` uses for a command, generalised to
carry stdin). Reading the CA cert back after `mitmdump` writes it — today `read_ca_pem` is a plain
`std::fs::read` — becomes an `ssh ... cat <path>` read for a remote target, since the file exists
only on the remote disk.

### Rule definitions need no new transport; materialised secrets get a narrower one

`.mio/proxy-rules.json` already reaches the remote host through the sandbox's own Mutagen sync
session — no proxy-specific work is needed for rule *definitions* to survive a deploy. The
materialised `rules.json` (real secret values, written by `write_materialised_rules`) is instead
written directly to the remote proxy's state directory over the same SSH-write path described above,
generated fresh from the laptop's `secrets.json` at the moment of deploy (and at every subsequent
`set_proxy_rules`/`apply_rules` call against a deployed sandbox) — never by syncing the raw
`secrets.json` store itself to the remote (see Technology Choices).

### A remote proxy gets its own fresh CA, re-injected through the existing transport

`inject_ca` (`src-tauri/src/proxy/mod.rs`) already pipes a CA PEM into a sandbox container over
`provider.exec_piped`, and `exec_piped` already routes through whichever `PodmanProvider` it is
called on — so once the sandbox container exists on `target_provider`, `inject_ca(target_provider,
name, &pem)` needs no new mechanism, only the new remote-side `pem` bytes (read back per the previous
section) instead of the local CA. `mitmdump` writing into an empty remote confdir produces a CA
distinct from the local instance's, consistent with the existing per-sandbox CA isolation the local
design already established (Resolved Decision, `docs/features/network-proxy/02_dev.md`) — this
document extends "per sandbox" to "per sandbox per location" rather than trying to carry one CA
keypair across the deploy boundary (see Technology Choices).

### The local proxy sidecar is stopped, not removed, exactly like the local sandbox container

`deploy` already keeps the local sandbox container around, stopped rather than removed, specifically
"so undeploy can restart it" (comment in `CliBackend::deploy`, `src-tauri/src/backends/mod.rs`). The
local proxy sidecar follows the identical rule: `deploy` stops it (a new thin wrapper reusing
`restart_proxy_container`'s stop half, or a dedicated `stop_proxy`) rather than calling `remove_proxy`
against it. This is load-bearing, not merely conservative: the local sandbox container's `--network`
attachment to `mio-proxy-<sandbox>` is baked in at `podman create` and cannot be changed without
recreating the container (Cross-Cutting Concerns, `docs/features/network-proxy/02_dev.md` — "toggling
means recreating the sandbox"). Deleting the local `mio-proxy-<sandbox>` network while the stopped
local container still references it by name would leave that container unable to restart cleanly on
a later `undeploy` (`services::sandbox::undeploy` calls `store.start(name)` on the very container
`deploy` left stopped). Stopping, not removing, sidesteps that failure mode entirely — the same
reasoning that already justifies leaving the sandbox container itself in place.

### Proxy commands resolve their connection through `SandboxRouter`, not a hardcoded local provider

`src-tauri/src/commands/proxy.rs` and `remove_proxy_if_present`
(`src-tauri/src/services/sandbox.rs`) call `registry.local(ProviderId::Podman)` directly today. Once
a proxy can live on a deploy target, "which connection is this sandbox's proxy actually on right
now" needs the same answer `SandboxRouter::locate` (`src-tauri/src/services/routing.rs`) already
computes for every other transport-sensitive command — checking the sync engine's live sessions for
a deploy target and falling back to local. Proxy command call sites resolve a route first and pass
`registry.provider_for(&route.machine)` into the proxy functions instead of the hardcoded local
provider.

### `deploy`'s refusal becomes conditional on a live spike having run, not removed outright

This document does not resolve whether `BackendError::ProxyDeployUnsupported` should actually be
lifted yet — see Validation. The model above is what implementing it would look like; whether it
ships is gated on the two-machine spike this document could not perform.

## Technology Choices with Tradeoffs

### Remote proxy state transport: raw SSH write/read mirroring `remote_mkdir` (chosen) vs. routing through Mutagen sync vs. proxying back to the laptop's local sidecar

Chosen: create the remote proxy's state directory and write its small text payloads (`addon.py`,
materialised `rules.json`) over a raw SSH command, and read the CA PEM back the same way — the exact
transport shape `remote_mkdir` (`src-tauri/src/backends/mod.rs`) already uses for the remote
workspace directory.

- **Mount the proxy state directory as a second Mutagen sync target, alongside the workspace sync
  session** — rejected: Mutagen sync is live and asynchronous (`docs/features/remote-sync/01_ideas.md`'s
  own validation measured a real, if small, propagation lag), but `wait_for_ca`
  (`src-tauri/src/proxy/mod.rs`) polls for the CA PEM to exist and `inject_ca` must never run before
  it does — a sync-lag race is a correctness bug here, not a UX nuisance. A continuously-live
  bidirectional sync session also holds the materialised secret file in a channel that stays open for
  the sandbox's whole lifetime, a larger exposure window than a one-shot deploy-time write.
- **Keep the proxy sidecar and its state entirely on the laptop's local Podman machine, and have the
  *remote* sandbox reach back to it over a tunnel** — rejected: this is the same shape
  `docs/features/network-proxy/01_ideas.md`'s Technology Choices already rejected for the local case
  (a `0.0.0.0`-published listener reachable via `host.containers.internal`, wider exposure than a
  private bridge network), and it is backwards for a deploy specifically — a deployed sandbox is
  meant to keep working when the laptop is off or asleep, and a laptop-resident proxy would make the
  sandbox's entire network path depend on the laptop staying online and reachable, defeating the
  point of deploying it at all.

### Secret transport at deploy time: ship only this sandbox's materialised `rules.json` (chosen) vs. replicate the whole `secrets.json` store vs. keep refusing (status quo)

Chosen: only the resolved values for rules the deploying sandbox's own `.mio/proxy-rules.json`
actually references leave the laptop, written straight into the remote proxy's state directory —
extending the existing local/materialised split (Resolved Decision 4, `docs/features/network-proxy/02_dev.md`)
across the deploy boundary rather than introducing a new exposure model.

- **Sync the whole `<app_data_root>/proxy/secrets.json` store to the remote** (e.g. as a third
  Mutagen target, or a one-shot copy) — rejected: `secrets.json` is app-global across every
  proxy-enabled sandbox (Current State), so this would hand a remote host every other sandbox's
  secret values too, not just the ones the deploying sandbox's rules reference — a strictly larger
  blast radius for no benefit, and it would need its own revocation story on undeploy that a
  per-sandbox materialised artifact does not.
- **Continue refusing deploy for a proxy-enabled sandbox** — not rejected, but explicitly the
  fallback if the live spike in Validation surfaces a blocker; kept as the safe default until that
  spike runs (see Open Questions).

**Security implications of shipping resolved secret values off the laptop at all.** Today every
secret a proxy rule can inject already sits in plaintext on the laptop's own disk
(`secrets.json`, unencrypted — the same posture `docs/features/cloud-sandboxes/01_ideas.md`'s Current
State documents as this app's existing norm for every other stored secret, not a new gap this
feature introduces). Shipping a sandbox's resolved values to a remote host's disk changes *whose*
disk the plaintext lives on: a laptop under the user's own physical control becomes a third-party or
cloud-provider-operated machine whose host-level administrators, a hypervisor-level compromise, or
(for a shared-tenancy VM) a co-tenant isolation failure could in principle read it — a materially
different threat model than "an attacker who already has code execution on the user's own laptop,"
which is the threat model the existing local `secrets.json` already accepts. SSH transit is encrypted
in flight, so the exposure is specifically about at-rest custody on a machine the user does not fully
control, not about network interception. This is a real, not merely theoretical, cost of supporting
remote proxy at all, and is the reason the narrower "only this sandbox's resolved rules" scope above
matters — it is the smallest version of that cost this feature can ship, not an elimination of it.

### CA identity across the deploy boundary: fresh remote CA, re-injected (chosen) vs. carrying the local CA private key to the remote

Chosen: the remote `mitmdump` instance generates its own CA keypair into its own (remote, empty)
confdir on first run, exactly as the local instance does; `inject_ca` re-runs against the sandbox
container once it exists on the target connection.

- **Copy the local CA keypair (including its private key) to the remote so the sandbox's
  already-trusted certificate authority carries over with no re-injection step** — rejected: shipping
  a CA *private key* to sit on a third-party-controlled disk is a strictly worse instance of the same
  exposure this document already accepts for resolved secret values above, for a benefit
  (skipping one `update-ca-certificates` re-run) that costs nothing to skip in the first place —
  `inject_ca` is already an idempotent, cheap `podman exec` round trip the deploy flow budgets for
  regardless, since the sandbox container itself is freshly created on the target connection every
  time.

### Local proxy sidecar fate during deploy: stop, don't remove (chosen) vs. full teardown vs. a distinct "paused" state

Chosen: stop the local `<sandbox>-proxy` container on deploy, leaving its network and state directory
(CA key included) intact — see Proposed Model for the network-attachment reasoning.

- **Remove the local proxy sidecar, its network, and its state directory entirely (`remove_proxy`)
  during deploy** — rejected after tracing the consequence through to `undeploy`: the local sandbox
  container's `--network mio-proxy-<sandbox>` attachment is fixed at `podman create` time and survives
  only if the network it names still exists; deleting that network while the stopped local container
  still references it risks `undeploy`'s `store.start(name)` (`services/sandbox.rs`) failing to
  restart the container at all. A dangling CA private key on local disk (the concern that motivated
  considering removal) is no worse, in practice, than the local sandbox's own SSH identity key already
  sitting on disk for the same stopped-not-removed container between deploy and undeploy — an
  existing, already-accepted pattern, not a new one.
- **A distinct third lifecycle state ("paused") between "container present and running" and "entirely
  absent"** — rejected: `proxy::exists`/`status` (`src-tauri/src/proxy/mod.rs`) already derive proxy
  enablement purely from container *presence* (Resolved Decision 3, `docs/features/network-proxy/02_dev.md`),
  not running state, so a stopped-but-present local container already reads correctly as
  "proxy-enabled" with no new status value needed — "paused" would be a redundant concept layered on
  top of a distinction (stopped vs. removed) the model above already makes for free.

## Validation

No live spike was performed for this feature, and none should be claimed. The core unconfirmed
assumption — rootless bridge-network behaviour on a genuine non-WSL remote Podman host — requires a
real two-machine topology: a laptop (or any machine driving the app) and a second, actual Linux host
reachable over SSH with rootless Podman installed, that is *not* itself a WSL-hosted Podman machine.
`docs/features/remote-sync/01_ideas.md`'s own validation used a second local WSL Podman machine
(`mio-tests`) standing in for "remote" specifically because it needed no real server — but that
stand-in is the wrong substitute for this feature's specific risk, since it would inherit the same
WSL `systemd --user`/`aardvark-dns` gap the local network-proxy spike already found, rather than
testing the genuinely different environment a bare Linux host presents. `docs/features/cloud-sandboxes/01_ideas.md`
records that this project has not yet stood up or validated against a real cloud VM at all — Oracle
Cloud and Google Cloud free-tier candidates are identified there but the two-stage spike that
document proposes has not been run.

What existing evidence *does* support, short of that live spike:

- The provider layer's `create_network`/`create_sidecar`/`pull_image` (`src-tauri/src/provider/mod.rs`)
  already route through the same `--connection <name>`-prefixed command builder `deploy` uses today
  for the sandbox container itself — no new provider-layer code path is required to aim them at a
  remote connection; only the state-directory/file-transport layer above them (this document's
  Proposed Model) is genuinely new.
- `prepare_host` (`src-tauri/src/backends/mod.rs`) already provisions `systemd --user` plus lingering
  on every remote host this app registers as a connection — the specific precondition the local WSL
  spike found missing — so the WSL-specific failure mode that forced `--disable-dns` plus static IP
  addressing locally is not guaranteed to recur remotely. This is not evidence that DNS-enabled
  bridge networking would work remotely (untested either way), only evidence that the *reason* it
  fails locally is plausibly WSL-specific rather than a general rootless-Podman limitation.
- The chosen design does not depend on the answer to the previous point either way: it reuses the
  already-implemented `--disable-dns` plus static-IP model unconditionally (Proposed Model), the same
  mechanism the local implementation already ships, rather than trying to detect or opt into
  DNS-enabled networking remotely. Whatever the remote host's `aardvark-dns` situation turns out to
  be, this feature's design does not read from it.

What remains genuinely unconfirmed and needs the live spike before implementation begins:

- Whether `podman network create --disable-dns --subnet <cidr>` plus a `--network <name>:ip=<addr>`-pinned
  attachment behaves identically on a real remote rootless Podman installation, which may run a
  different Podman/netavark/pasta version than the bundled Windows-machine Podman — specifically,
  whether a bridge-only container still gets outbound internet access the way the local WSL spike
  observed, since a bare cloud VM's kernel/sysctl defaults (e.g. IP forwarding) are not guaranteed to
  match the WSL machine's.
- Whether `docker.io/mitmproxy/mitmproxy` pulls and starts cleanly under rootless Podman on that same
  remote host, and whether the uid-1000 bind-mount-writes-CA-to-host behaviour the local spike
  observed (`docs/features/network-proxy/02_dev.md`, Spike Results) reproduces identically on a
  different host filesystem/user-namespace mapping.
- Whether the proposed raw-SSH write/read path for `addon.py`/materialised `rules.json`/the CA PEM
  behaves correctly under real network latency, as opposed to the effectively-zero latency of the
  local WSL-to-WSL validation `remote-sync/01_ideas.md` performed.

## Open Questions

- **Whether to lift `ProxyDeployUnsupported` before the live spike clears, or hold it.** This
  document's own recommendation is to keep the refusal in place — and therefore not implement the
  Proposed Model above — until a real two-machine spike against a genuine non-WSL host (Validation)
  confirms the bridge-network assumptions. Whether a future agent proceeds with implementation behind
  a flag anyway, accepting the risk, is left to that agent's judgment at the time, informed by
  whatever cloud-host access (`docs/features/cloud-sandboxes/01_ideas.md`) exists by then.
- **Undeploy symmetry: does undeploying a sandbox that gained a remote proxy restore the local
  one automatically, or require the user to re-enable it?** The stop-don't-remove design (Proposed
  Model) means the local proxy container and its baked-in network attachment survive a deploy/undeploy
  round trip mechanically, but whether `services::sandbox::undeploy` should also restart the local
  proxy container (mirroring `store.start(name)` for the sandbox itself) or leave it stopped until
  some other trigger is undecided.
- **The remote proxy's own fate on `undeploy`.** Symmetric to the previous question from the other
  direction: does `undeploy` remove the remote proxy sidecar (and its remote state directory,
  including its CA private key) as part of tearing down the remote container, or leave it in place the
  way the local instance is left stopped rather than removed during deploy? Leaving it in place risks
  the same class of orphaned-secret-material concern this document's Security Implications discussion
  raises for deploy itself, now on the undeploy path.
- **`services::sandbox::destroy`'s missing proxy cleanup on the deployed-sandbox branch is a
  pre-existing gap this feature must close regardless of the previous questions' answers.** Confirmed
  in Current State: the deployed-sandbox branch of `destroy` never calls `remove_proxy_if_present`.
  Today this is unreachable (a proxy-enabled sandbox cannot be deployed), so this is scoped as an
  in-scope fix for whichever build plan implements this feature, not a deferred improvement — but the
  exact cleanup (remove local only, remote only, or both, and in what order relative to
  `store.remove_remote`/`store.destroy`) is undecided pending the two questions above.
- **Progress reporting for a remote proxy's image pull and CA wait.** `ensure_proxy_image` already
  reports through `activity::TaskRegistry` locally (Resolved Decision 10,
  `docs/features/network-proxy/02_dev.md`) specifically because a first pull is large enough to look
  like a hung create. A remote pull over a possibly slower or higher-latency link than the local
  machine's own registry access is at least as likely to need the same treatment, and `deploy`
  (`docs/features/remote-deploys/01_ideas.md`'s own Open Questions) already flags remote base-image
  build progress reporting as undecided for the same reason — whether the two should share one
  progress-reporting answer once either is resolved is open.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A deployed, proxy-enabled sandbox's `undeploy` fails to restart the local container, or the container starts with the wrong proxy IP | The local `mio-proxy-<sandbox>` network was removed while the stopped local sandbox container's baked-in `--network` attachment still named it | Never call `remove_proxy` against the local connection during `deploy`; stop the local proxy sidecar instead, exactly as the local sandbox container itself is only stopped, not removed (Proposed Model, "local proxy sidecar is stopped, not removed") |
| A proxy rule's real secret value shows up on a remote host's disk that was never supposed to see it | `secrets.json` (`src-tauri/src/proxy/mod.rs`) was synced or copied to the remote wholesale instead of writing only the deploying sandbox's own materialised `rules.json` | Ship only the resolved output of `write_materialised_rules` for the one sandbox being deployed; never transfer the raw `<app_data_root>/proxy/secrets.json` store itself (Technology Choices, "Secret transport at deploy time") |
| `inject_ca` fails or the sandbox's HTTPS requests fail with a certificate-trust error immediately after a proxy-enabled deploy | The CA PEM read back after deploy was the *local* proxy's CA (stale, already trusted before deploy) rather than the freshly generated remote CA | Read the CA PEM from the remote proxy's own state directory (over SSH) after the remote `mitmdump` has started, not from the laptop-local `certs/mitmproxy-ca-cert.pem` path `ca_pem_path` resolves by default |
| A remote proxy sidecar never starts, and `podman create` on the target connection reports a subnet or DNS-related failure that never happens locally | The local WSL spike's `--disable-dns`/static-IP finding (`docs/features/network-proxy/02_dev.md`, Spike Results) was assumed to generalize to every rootless Podman host without being tested against one | Treat this as the exact risk the Validation section's live two-machine spike exists to clear before shipping; do not assume the local spike's specific WSL failure mode, or its absence, on an untested remote host |
| Destroying a deployed, proxy-enabled sandbox leaves an orphaned proxy container or network on the local machine, the remote machine, or both | `services::sandbox::destroy`'s deployed-sandbox branch (`src-tauri/src/services/sandbox.rs`) does not call `remove_proxy_if_present` today | Add proxy cleanup to that branch as part of implementing this feature (Open Questions) rather than assuming the existing non-deployed branch's call covers every case |
| A proxy command (`proxy_status`, `set_proxy_rules`, `restart_proxy`) acts on the wrong sandbox's containers after it has been deployed | The command resolved `registry.local(ProviderId::Podman)` directly instead of routing through `SandboxRouter` to find which connection is currently authoritative for that sandbox | Resolve a `SandboxRoute` via `SandboxRouter::locate` (`src-tauri/src/services/routing.rs`) first, and pass `registry.provider_for(&route.machine)` into the proxy function instead of a hardcoded local provider |
