---
type: ideas
title: "Proxy Remote Deploy: A Bounded Sandbox on a Remote Podman Host"
feature: proxy-remote-deploy
---
# Proxy Remote Deploy: A Bounded Sandbox on a Remote Podman Host

This document scopes letting a sandbox with an iron-proxy sidecar be deployed to a remote Podman host, which today is refused for every sandbox. It replaces an earlier ideation written for the removed mitmproxy sidecar; git history holds that version, and nothing in it about `addon.py`, `rules.json`, `apply_rules` or `MIO_PROXY_IMAGE` applies any more. The bounded-sandbox design being carried to the remote host is `docs/features/podman-egress-boundary/02_dev.md` (current behaviour: [network.md](../../architecture/network.md)); the deploy flow it extends is [remote.md](../../architecture/remote.md). Relevant code: `CliBackend::deploy` and `resolve_workspace` in `src-tauri/src/backends/mod.rs`, the sidecar lifecycle in `src-tauri/src/proxy/mod.rs` and `src-tauri/src/proxy/iron.rs`, the audit ingest in `src-tauri/src/proxy/audit.rs` and `src-tauri/src/proxy/flowlog.rs`, the orchestration in `src-tauri/src/services/sandbox.rs`, routing in `src-tauri/src/services/routing.rs`, and the deploy UI in `src/routes/projects/RemoteManager.svelte`.

## Current State

This section records the state before the feature was implemented; current behaviour is in [remote.md](../../architecture/remote.md) and [network.md](../../architecture/network.md).

- **Deploy is refused.** `CliBackend::deploy` (`src-tauri/src/backends/mod.rs`) returns `BackendError::ProxyDeployUnsupported` whenever `proxy::exists` finds a `<name>-proxy` sidecar, and every sandbox created since commit `bd8f92a` has one. `RemoteManager.svelte` shows a disabled Deploy button (`data-remote-deploy-unavailable`); the host picker and its strings (`projects_no_hosts`, `projects_deploy_confirm`) were deleted in `bd8f92a` and can be restored from that commit's parent.
- **What deploy does today.** `CliBackend::deploy` resolves the target's SSH endpoint (`resolve_workspace`, `remote_connection_info`), stops the local container, creates the remote workspace directory over raw `ssh` (`remote_mkdir`), writes a `Direct` SSH alias for the target, builds the base image remotely (`ensure_base_image` on a target-pinned provider from `crate::provider::default_provider`), and creates the remote container with `network: None` (`pasta`) and no environment. The `deploy_sandbox` command (`src-tauri/src/commands/deploy.rs`) then runs `RuntimeManager::reproduce_remote` (copies `mise.toml`/`mise.lock` with `podman cp`, runs `mise install`) and creates the Mutagen session. Nothing rolls back a failed deploy.
- **Undeploy and destroy.** `services::sandbox::undeploy` flushes, removes the remote container (`SandboxBackend::remove_remote`), terminates the session and starts the local container. The deployed branch of `services::sandbox::destroy` never calls `proxy::remove_proxy` or `net_policy::remove_applied`, a gap that is unreachable today only because no proxied sandbox can deploy.
- **Every sidecar operation is local.** `proxy::sidecar_provider` (`src-tauri/src/proxy/mod.rs`) and `iron::apply_policy`, `iron::refresh_github_binding` (`src-tauri/src/proxy/iron.rs`) all use `registry.local(ProviderId::Podman)`. Proxy commands (`src-tauri/src/commands/proxy.rs`, `src-tauri/src/commands/net_policy.rs`) take only `ProviderRegistry`, not `SandboxRouter`.
- **Sidecar state is host bind mounts.** `create_podman_proxy` mounts `<app_data_root>/proxy/<sandbox>/{certs,config,secrets}` read-only into the sidecar. Those paths exist only on the laptop, so they cannot be mounted by a remote Podman.
- **Proven remote transports.** `podman --connection <target> cp <laptop file> <container>:<path>` already carries files from the laptop into remote containers (`install_sandbox_key` in `src-tauri/src/backends/mod.rs`, `reproduce_remote` in `src-tauri/src/runtime/mod.rs`). `MachineProvider::exec_piped` streams stdin into a container but goes through `run_with_stdin_file()` (`src-tauri/src/process/mod.rs`), which stages stdin in a temp file (`docs/improvement-backlog.md`, T5a). `remote_mkdir` is the only raw-SSH shell use, and it bypasses the `ProcessRunner` seam.
- **No sidecar starts with its sandbox.** `start_sandbox` (`src-tauri/src/commands/sandbox.rs`) starts only the sandbox container, and sidecars have no restart policy, so after a machine reboot a sandbox has no network until the sidecar is restarted from the traffic panel or a policy apply starts it.
- **Remote specs are skipped.** `e2e/test/specs/standard/deploy-remote.spec.ts`, `base-image-remote.spec.ts` and three `it` blocks of `runtime-management.spec.ts` carry `describe.skip`/`it.skip`. The `standard` harness still provisions the second WSL Podman machine `mio-tests-remote` (`e2e/wdio.conf.ts`, `onPrepare`).

## Proposed Model

### The remote bounded sandbox mirrors the local one

A deploy recreates the three parts of a bounded sandbox on the target connection: the internal `mio-proxy-<sandbox>` network (`allocate_network` against a target-pinned provider, which walks `10.90.<index>.0/24` on the target independently), the dual-homed iron-proxy sidecar `<sandbox>-proxy` (pinned on the internal network, joined to the target's default `podman` network), and the sandbox container on the internal network with `--dns <remote sidecar ip>`, `proxy_env(<remote sidecar ip>)` and the placeholder variables from the snapshot's `guestEnv`. The remote sidecar runs the same pinned image (`iron_proxy_image()`, `src-tauri/src/proxy/iron.rs`) and the same renderer (`src-tauri/src/proxy/iron_config.rs`), rendered with the remote sidecar address.

### The remote sidecar holds its state inside the container, pushed over the Podman connection

A remote sidecar has no host bind mounts. Its `config/` and `certs/` are copied from a laptop staging directory into the created-but-unstarted container with `MachineProvider::copy_in`, and later config changes are copied in and swapped before a reload. Secret files are pushed into the running sidecar (see Open Questions, "Where remote secret values live"). The laptop stays the single source of truth: the remote sidecar's files are always a copy of what the app rendered, so `podman rm` of the sidecar is a complete remote teardown.

### Laptop-side remote state sits beside the local state

`<app_data_root>/proxy/<sandbox>/remote/` holds the remote sidecar's cache (`remote.json`: connection, network, sidecar address), its rendered `config/`, its `certs/` and its own `audit.cursor`. The `secrets/` directory is shared: an apply stages secret files once on the laptop and pushes the same set. `remote.json` is a cache like `meta.json`; whether a sandbox is deployed is still decided only by its Mutagen session ([remote.md](../../architecture/remote.md), Invariants).

### The local sidecar is stopped, not removed, during a deploy

The local sandbox container stays stopped and keeps its `--network mio-proxy-<sandbox>` attachment, so the local sidecar and network must survive. Deploy stops the local sidecar after the remote one serves traffic; undeploy starts it before the local sandbox. While deployed, every apply also writes the local sidecar's files offline (as `write_offline` in `src-tauri/src/proxy/iron.rs` already does for a stopped sidecar), so undeploy boots the local sidecar with the current policy and secrets.

### Proxy operations resolve the sidecar's site through `SandboxRouter`

A new `SidecarSite` (the machine, its provider, and whether it is local) is resolved by the caller from `SandboxRouter::locate` (`src-tauri/src/services/routing.rs`) and passed into the `proxy/` functions, which stay free of any `SyncEngine` dependency (`src-tauri/AGENTS.md`, subsystem independence). `proxy_status`, `restart_proxy`, `apply_sandbox_policy`, `watch_proxy_flows`, the GitHub binding refresh (`src-tauri/src/services/github_access.rs`) and teardown all route through it.

### Audit polling follows the active sidecar

`audit::ingest` reads `podman logs` from whichever sidecar is active, with a per-site cursor file, appending to the one laptop-side `flows.ndjson`, so the traffic history is continuous across deploy and undeploy. A running flow watch is retargeted by deploy and undeploy rather than re-resolving the route on every tick.

### Deploy moves into a service with rollback

Deploy composes the backend, the proxy, the runtime and the sync engine, so its orchestration moves to `services::sandbox::deploy` (`src-tauri/src/services/sandbox.rs`), and `deploy_sandbox` becomes a thin command. The service plans first without mutating anything (target reachable, target Podman able to host the boundary), creates the remote sidecar before the local sandbox is stopped, and on any failure before the Mutagen session exists removes what it created on the target and restores the local sandbox and sidecar.

## Technology Choices with Tradeoffs

### Remote sidecar state: in-container, pushed over the Podman connection (proposed)

- **In-container files via `podman cp`/`podman exec` over `--connection` (proposed).** Reuses the transport `install_sandbox_key` and `reproduce_remote` already prove against remote targets; needs no SSH shell, no remote directory, and no SELinux relabelling; crosses the `ProcessRunner` seam, so the `light` tier can script it; teardown is `podman rm`.
- **A remote host directory `<home>/mio-proxy/<sandbox>/` written over raw `ssh`, bind-mounted like the local one** (the old plan's choice). Rejected: it adds a second transport beside the Podman connection, bypasses the `ProcessRunner` seam like `remote_mkdir`, leaves a directory holding the CA key and secrets that only an explicit SSH `rm` removes, and a bind mount on an SELinux-enforcing host needs a `:Z` label the local path never needed.
- **A named Podman volume populated through a helper container.** Rejected: same reach as in-container files plus a volume lifecycle to manage, with no benefit, because the laptop re-pushes everything anyway.
- **Mutagen-syncing the state directory.** Rejected: asynchronous propagation races the sidecar boot, and it would keep a live channel carrying secret files for the sandbox's lifetime.

### Where the egress boundary runs: on the target (proposed) vs. back on the laptop

- **Sidecar on the target (proposed).** A deployed sandbox keeps working while the laptop sleeps, which is the point of deploying.
- **The remote sandbox tunnels back to the laptop's sidecar.** Rejected: the sandbox's whole network would depend on the laptop being online, and the tunnel would need an exposed listener the local design rejected (`docs/features/network-proxy/01_ideas.md`).

### CA for the remote sidecar: a fresh one per deploy (proposed) vs. reusing the local CA

- **Fresh CA generated in-process at deploy (proposed).** `iron::generate_ca` costs no container run; a key leaked from the remote host cannot be used against the local sandbox; undeploy needs no re-injection because the local container's trust store was never touched.
- **Reuse the local CA.** Rejected as a weaker isolation for no saving: the remote sandbox is a fresh container that needs a CA injected either way.

### Placeholders on the remote sandbox: the snapshot's `guestEnv` (proposed) vs. every current binding

- **`guestEnv` from `applied.json` (proposed).** Keeps one `guestEnv` valid for both containers, so the editor's export hint stays correct across deploy and undeploy.
- **Stamp every current binding on the fresh remote container.** Rejected: the remote and local containers would then hold different variable sets under one snapshot, which `AppliedPolicy` (`src-tauri/src/net_policy.rs`) cannot express without a per-site field.

### Image on the target: pull from the registry (proposed) vs. transfer from the laptop

- **`ensure_proxy_image` with the target-pinned provider (proposed).** Same activity-reported pull as create; needs the target to reach Docker Hub, which `mise install` on the target needs for its own downloads anyway.
- **`podman save`/`load` or `podman image scp` from the laptop.** Deferred to [04_improvements.md](04_improvements.md): only an air-gapped target needs it.

### Host-global state

The feature adds no host port, no bundled binary and no new environment variable. New laptop paths live under `<app_data_root>/proxy/<sandbox>/remote/`, which honours `MIO_APP_DATA_DIR`. The deploy flow already depends on host-global state (the `mio-tests-remote` Podman machine and the `~/.ssh` alias writes), so its specs stay in the host-locked `standard` tier, serialised against every other real-infra run; this feature adds no new host-global resource to that list.

## Validation

No spike has run for the iron-proxy design. The boundary was proven only on the local WSL Podman machine (`docs/features/podman-egress-boundary/spike/README.md`). The checks that gate the build, each runnable with the existing scripts in `docs/features/podman-egress-boundary/spike/` adapted to `SPIKE_CONNECTION` and to in-container state instead of `SPIKE_STATE` bind mounts:

1. On `mio-tests-remote`: internal network plus dual-homed sidecar created over `--connection`; `podman cp` of `config/` and `certs/` into the created container before its first start; iron-proxy boots and denies; a reload after a copied-in config swap works; the file ownership and mode that `podman cp` produces are readable by iron-proxy's user.
2. A secret pushed into the running sidecar (into a tmpfs mount or the container layer) is swapped on the next request after a reload.
3. A sandbox on the internal network over `--connection`: no route out, `podman --connection <target> exec -i <name> mio-sshd` (the exec-transport alias) still works, `mise install` succeeds through the sidecar, and `podman logs --timestamps` reads the audit stream over the connection.
4. The same checks on a real non-WSL Linux host, ideally one with SELinux enforcing and one with an older Podman, to fix the minimum version the deploy preflight requires.

## Open Questions

- **Where remote secret values live.** Real secret values (workspace bindings with a stored value, and the GitHub token behind the app-owned binding) must reach the remote host for the remote sidecar to swap them. Options: files in the sidecar's container layer (on the remote disk until the sidecar is removed); a tmpfs mount in the sidecar (RAM only, pushed again after every sidecar start); no secrets on the remote (bindings pass through unswapped there); or refusing deploy while bindings with values exist.
- **Whether the user is told before secrets leave the laptop.** Options: no notice; a one-line notice in the deploy picker naming the host and the number of secrets sent; a blocking confirmation.
- **Already-deployed and sidecar-less sandboxes.** A sandbox deployed before commit `bd8f92a` runs unbounded on its target, and a sandbox created before the sidecar became universal has no sidecar to carry. Settled: every sandbox is assumed to have a sidecar and a sidecar-less one is refused at deploy (`02_dev.md`, Resolved Decision 3).
- **How a policy change reaches the remote sidecar.** Options: push and reload live on every apply; apply only at deploy, requiring a redeploy; and, when the target is unreachable, either fail the apply or queue it for the next reachable moment.
- **Whether starting a sandbox also starts its sidecar.** Today it does not (Current State). A remote host reboot stops both containers, and a tmpfs secret store would make a start without a push useless, so the question is whether this feature makes `start_sandbox` start (and, for the remote, re-push) the sidecar first, locally and remotely.
- **The minimum target the deploy preflight accepts.** Which Podman version and network backend (netavark versus CNI) a target needs, decided by Validation check 4.
- **What the remote e2e must prove before the feature counts as done.** Options: the un-skipped `standard` specs against `mio-tests-remote` alone; those plus a manual run against a real Linux host.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `podman create` of the remote sidecar fails with a missing mount source | The remote sidecar was given the laptop's `<app_data_root>/proxy/<sandbox>/` bind mounts, which do not exist on the target host | Give the remote sidecar no host bind mounts; copy `config/` and `certs/` into the created container with `MachineProvider::copy_in` before its first start |
| `undeploy` fails to start the local sandbox, or the local sandbox has no network after undeploy | Deploy removed the local `mio-proxy-<sandbox>` network or sidecar, or undeploy started the sandbox before the sidecar | Deploy only stops the local sidecar; undeploy starts the local sidecar before `store.start` (`services::sandbox::undeploy`, `src-tauri/src/services/sandbox.rs`) |
| A policy saved while deployed has no effect on the remote sandbox, or is lost after undeploy | The apply wrote and reloaded only the local sidecar (`registry.local` in `iron::apply_policy`, `src-tauri/src/proxy/iron.rs`) | Resolve the sidecar's site through `SandboxRouter::locate`; push and reload the remote sidecar, and write the stopped local sidecar's files offline |
| The traffic panel stops showing rows after a deploy, or repeats old rows after an undeploy | The flow watch kept polling the stopped local sidecar, or reused the local `audit.cursor` against the new remote container | Retarget the flow watch on deploy and undeploy, and keep a separate cursor under `<app_data_root>/proxy/<sandbox>/remote/` |
| Destroying a deployed sandbox leaves a `<name>-proxy` container and `mio-proxy-<name>` network on the target, or the local sidecar and `applied.json` behind | The deployed branch of `services::sandbox::destroy` (`src-tauri/src/services/sandbox.rs`) removes only the two sandbox containers | Remove the remote sidecar and network after the remote sandbox, then the local sidecar, network, state directory and applied snapshot |
| HTTPS from the remote sandbox fails with "unable to get local issuer certificate" | The remote sandbox was given the local sidecar's CA, while the remote sidecar mints leaves from its own fresh CA | Inject the CA from `<app_data_root>/proxy/<sandbox>/remote/certs/ca.pem` into the remote sandbox |
