---
type: ideas
title: "Local LLM Reachability Ideas"
feature: local-llm-reachability
---
# Local LLM Reachability Ideas

This document covers making the host inference server reachable from every sandbox, and keeping the local-model agents' provisioned config pointed at the server. Today only Podman sandboxes can reach the server. A microsandbox sandbox with pi or OpenCode installed cannot connect, because of four independent faults (Current State). The proposal replaces the single listener on the Podman machine with one Mutagen reverse forward per sandbox, listening on the sandbox's own loopback. It also merges the two agent-provisioning paths into one that covers every catalogue agent. The relevant code is in `src-tauri/src/commands/inference.rs`, `src-tauri/src/sync/mod.rs`, `src-tauri/src/network/`, `src-tauri/src/proxy/mod.rs`, `src/lib/agents/` and `src/routes/models/+page.svelte`.

## Current State

- **One machine-level listener.** `start_inference` in `src-tauri/src/commands/inference.rs` starts the engine on an ephemeral host loopback port. It then calls `sync::forward_create_reverse` (`src-tauri/src/sync/mod.rs`) to create the Mutagen session `inference-reverse` (`inference::INFERENCE_FORWARD`). That session listens on `0.0.0.0:51199` inside the Podman machine named by `backends::connection_name()`. The fixed port is `INFERENCE_VM_PORT` in `src-tauri/src/sync/mod.rs`, mirrored in `src/lib/agents/local-llm.ts`. A comment in `start_inference` claims microsandbox instances reach the same listener; that claim is false.
- **One base URL.** `LOCAL_LLM_BASE_URL` (`src/lib/agents/local-llm.ts`) is `http://host.containers.internal:51199/v1`. Both `buildLocalLlmProvider` (`src/lib/agents/opencode.ts`) and `buildPiLocalLlmProvider` (`src/lib/agents/pi.ts`) write that URL into workspace files.
- **Proxy bypass.** `proxy_env` (`src-tauri/src/proxy/mod.rs`) lists `host.containers.internal` in `NO_PROXY` so Podman sidecar-proxied sandboxes skip the proxy for inference traffic.
- **microsandbox egress.** `msb_policy::render` (`src-tauri/src/provider/msb_policy.rs`) leaves microsandbox's implicit `allow@public` in place. The `host` group is reachable only on port 53. A traffic-viewer sandbox also chains all egress through a SOCKS5 sidecar.
- **Forward bookkeeping.** `NetworkManager` (`src-tauri/src/network/mod.rs`, `ServiceKind` in `src-tauri/src/network/types.rs`) tracks the forwards for terminals, agent web UIs and exposed ports. It tears them down with their sandbox. Its port poller does not auto-expose ports that a tracked forward already uses. `sweep_stale_forwards` (`src-tauri/src/sync/mod.rs`) removes orphaned sessions by their name prefix. The inference session is outside all three.
- **Provisioning, split in two.** Install-time provisioning branches on `provision.kind` inside `AgentManager.svelte` (`src/routes/projects/AgentManager.svelte`). `reprovisionInstalledAgents` (`src/lib/agents/reprovision.ts`) runs after a model starts on the Models page (`src/routes/models/+page.svelte`) and handles OpenCode only. A pi config therefore never changes after install. `PI_DEFAULT_MODEL` (`src/lib/agents/pi.ts`) is declared and never used. `provision_opencode` (`src-tauri/src/agents/config.rs`) inserts `model` only when absent, so OpenCode's default model does not follow the running server either.

### Failures observed in `rrr` (2026-11-09)

| # | Fault | Evidence |
|---|---|---|
| 1 | `host.containers.internal` does not resolve inside a microsandbox guest; microsandbox names the host `host.microsandbox.internal` | `Could not resolve host` |
| 2 | Port 51199 exists only inside the Podman machine, not on the Windows host or the microsandbox gateway | `curl host.microsandbox.internal:51199` fails |
| 3 | The effective microsandbox policy allows the `host` group on port 53 only | `msb inspect rrr` |
| 4 | Pi's `models.json` is never re-provisioned, so its URL and model list stay frozen at install time | stale catalogue ids, no `qwen3.5-2b` |

Spike record: `docs/features/local-llm-reachability/spike/01_mutagen-reverse-msb.md`.

## Proposed Model

### One In-Sandbox Endpoint for Every Provider

Every sandbox, Podman or microsandbox, reaches the server at `http://127.0.0.1:<guest port>/v1`, which is the sandbox's own loopback. `LOCAL_LLM_BASE_URL` in `src/lib/agents/local-llm.ts` becomes that URL. The paired constants keep their two-file comment and are renamed from `INFERENCE_VM_PORT` to a guest-port name in both `src-tauri/src/sync/mod.rs` and `src/lib/agents/local-llm.ts`. Guest loopback traffic never leaves the guest kernel. That means faults 1 to 3 disappear without touching DNS, the microsandbox egress policy or either proxy.

### Per-Sandbox Reverse Forward

Each local sandbox gets its own Mutagen reverse forward. The source is `<sandbox alias>:tcp:127.0.0.1:<guest port>` and the destination is the host server's loopback port. The forward hangs off `NetworkManager` as a new `ServiceKind::Inference` (`src-tauri/src/network/types.rs`), alongside a reverse-direction counterpart of `expose_port` in `src-tauri/src/network/mod.rs`. The session is named `inference-<sandbox>`. Being a tracked forward gives three things for free:

- the sandbox teardown paths (destroy, re-apply, undeploy) close it;
- the port poller excludes the guest port, so the forward's own listener is never auto-exposed back to the host;
- `sweep_stale_forwards` can sweep the `inference-` prefix.

### Inference Reach Service

A new service in `src-tauri/src/services/` keeps the set of per-sandbox forwards in line with two facts: whether a server is running, and which sandboxes are running locally. `start_inference` and `stop_inference` in `src-tauri/src/commands/inference.rs` stay thin and delegate to the service.

- **Server start** ensures a forward for every running, non-deployed sandbox. A restart changes the host port, so this step recreates every forward.
- **Server stop** terminates every `inference-` forward.
- **Sandbox start** ensures the sandbox's forward if a server is running (hook point: Open Question 2).
- **Sandbox stop** terminates the sandbox's forward (Open Question 1).
- **Startup** sweeps the legacy `inference-reverse` session along with orphaned `inference-` sessions.

### Unified Agent Provisioning

One helper in `src/lib/agents/` provisions a single agent into a single sandbox, given its catalogue descriptor and the running model. Both the install path in `AgentManager.svelte` and the re-provision loop call that helper, so neither branches on `provision.kind` any more. `reprovisionInstalledAgents` (`src/lib/agents/reprovision.ts`) covers every catalogue agent that declares `provision`, pi included. It runs after every successful inference start and once at app startup. The startup pass migrates existing workspace configs off the `host.containers.internal` URL and refreshes pi's model list (fault 4). `provision_pi` (`src-tauri/src/agents/config.rs`) already replaces the `local-llm` provider entry by id, so re-running it is safe.

### Retire the Machine-Level Listener

`start_inference` stops creating the `inference-reverse` session and stops calling `ensure_connection_alias`. Remote deploy still uses `ensure_connection_alias`, so the function stays. The false microsandbox comment goes. `proxy_env` drops `host.containers.internal` from `NO_PROXY`, because `127.0.0.1` already covers the new URL. The inference invariant "the reverse forward listens on all interfaces of the Podman machine" (`docs/architecture/inference.md`) is replaced when the feature ships.

## Technology Choices

### Reachability path: per-sandbox guest-loopback reverse forward

- **Chosen:** a Mutagen reverse forward per sandbox, listening on guest loopback. The forward travels the existing SSH alias over the engine's exec channel, which is not guest egress. Default-deny microsandbox egress, `strictHostnames` and the chained SOCKS proxy therefore never see the traffic (`docs/features/microsandbox-network-policy/spike/01_egress-baseline.md`, Item 4). No policy edit means no VM recreate for existing sandboxes.
- **Rejected: allow the `host` group in the microsandbox policy and dial `host.microsandbox.internal`.** llama-server and Foundry bind host loopback, and whether the microsandbox gateway relays to host loopback is unverified. The rule is fixed at create, so every existing sandbox would need a recreate. With the traffic viewer on, the dial would also be chained through the SOCKS sidecar. Finally, the server's port is ephemeral, so the rule would have to allow the whole `host` group rather than one port.
- **Rejected: route microsandbox through the Podman-machine listener.** The Podman machine's address is in microsandbox's denied `private` group. The route would also force Podman to be running for a user who chose microsandbox.
- **Rejected: microsandbox port publishing (`-p`).** Publishing is ingress-only (host to guest), the wrong direction.
- **Rejected: an in-sandbox relay process.** A relay still dials out through guest egress, and the transport invariant forbids relays inside a sandbox (`docs/architecture/transport.md`).

### Scope: both providers, one URL

- **Chosen:** move Podman sandboxes to the per-sandbox forward as well.
  - The workspace files carry one provider-neutral URL.
  - The `0.0.0.0` listener on the shared Podman machine, which every container and the WSL network can reach, goes away.
  - The feature loses its dependency on host-global state: a fixed port on the default Podman machine. Inference specs can therefore isolate per worktree instead of being pinned to the host-locked tier (`e2e/AGENTS.md`).
  - The special `NO_PROXY` entry goes.
- **Rejected: microsandbox only, with a provider-dependent URL.** It keeps two reachability paths and two URLs to test. It also keeps the host-global Podman listener. It writes a provider-specific URL into workspace files, which are meant to describe the project, not its engine.
- **Cost accepted:** one Mutagen session per running sandbox instead of one in total, and a lifecycle to manage per sandbox.

### Forward ownership: `NetworkManager`, not raw `sync` calls

- **Chosen:** `ServiceKind::Inference` in `NetworkManager`. Teardown, poller exclusion and frontend visibility come with it.
- **Rejected: the new service calling `sync::forward_create_reverse` directly.** That would duplicate teardown and leave the port poller to auto-expose the guest listener as a user port.

### Host-port changes: recreate forwards on every server start

- **Chosen:** recreate all per-sandbox forwards when the server starts, because Mutagen fixes a forward's destination at create. Server restarts are user-initiated and rare.
- **Rejected for now: a stable in-app TCP relay on a per-run host port** that absorbs engine restarts. It adds an in-process proxy for a rare event (`docs/features/local-llm-reachability/04_improvements.md`).

### Guest listen address: `127.0.0.1`

- **Chosen:** guest loopback. The agent shares the guest's network namespace, so nothing else needs to reach the listener.
- **Rejected: `0.0.0.0`.** It would expose the model server to whatever else can reach the guest. The existing `0.0.0.0` requirement in `forward_create_reverse`'s comment applied to the machine-level listener only, which containers dialled across a network boundary.

### Migrating existing configs: re-provision at startup and server start

- **Chosen:** re-provision every provisioned agent at app startup and after each server start. The provisioners are host-side file merges, so they need no running sandbox.
- **Rejected: an environment override such as `OPENAI_BASE_URL`.** pi and OpenCode take a provider's base URL from their config files, not from the environment.
- **Rejected: leave existing configs alone.** Existing sandboxes would keep the broken URL, as `rrr` did.

## Validation

- The 2026-11-09 diagnosis in `rrr` established faults 1 to 4 (Current State; `spike/01_mutagen-reverse-msb.md`).
- The 2026-11-01 microsandbox-network-policy spike proved the topology with `ssh -R`. A guest loopback listener over the `mio-sshd` transport reached a host loopback server under deny-egress, strict hostnames and TLS interception. A direct dial to `host.microsandbox.internal` failed (`docs/features/microsandbox-network-policy/spike/01_egress-baseline.md`, Item 4).
- On 2026-11-10 the app's bundled Mutagen created the proposed forward into the running, traffic-viewer-proxied `rrr` (`spike/01_mutagen-reverse-msb.md`).
  - The guest listener bound `127.0.0.1` only.
  - Models and chat requests returned HTTP 200 under deny-egress, TLS interception and the SOCKS chain.
  - pi 1.5.0, given a config whose `baseUrl` was the guest-loopback URL, answered a prompt from the host model.
  - The same spike confirmed two design inputs: the app's port poller auto-exposes an untracked guest listener, and the host port changes between server runs.
- Against a stopped microsandbox sandbox, a Mutagen dial makes the alias `ProxyCommand` try to boot the VM. For a proxied sandbox the boot fails with `host environment variable MIO_SOCKS_PASSWORD is not set`, and Mutagen's reconnect loop repeats the attempt (Open Question 1).

## Open Questions

1. **Stopped-sandbox forwards.** The 2026-11-10 spike showed that a live forward makes Mutagen keep trying to boot a stopped microsandbox sandbox through `msb exec`, and that the boot fails for a proxied sandbox. The open decision is how sandbox stop handles the inference forward. Option one: terminate it on stop and recreate it on start. Option two: pause it with `mutagen forward pause` and resume it on start, which keeps the session's name and destination. Recommendation: terminate on stop and ensure on start, so the inference reach service owns a single "ensure" path. The same boot-on-dial behaviour may apply to the existing terminal and port forwards (`04_improvements.md`).
2. **Sandbox-start hook.** Where does "ensure the inference forward" run when a sandbox starts? The options are the start command in `src-tauri/src/commands/`, the routing service's alias refresh (`src-tauri/src/services/routing.rs`), or a periodic reconciler. Recommendation: the start command plus the routing refresh, with no reconciler.
3. **Deployed sandboxes.** Should a sandbox deployed to a remote host get an inference forward over its remote alias? Recommendation: exclude deployed sandboxes in this feature. Today the old URL resolves to the remote machine there and never worked either.
4. **Default model.** Should provisioning set pi's default provider and model (pi's `settings.json`, using the unused `PI_DEFAULT_MODEL`)? Should OpenCode's `model` follow the running server instead of being written only when absent? Both decide whether "the agents' default model follows the server" (`docs/architecture/agents.md`) becomes true.
5. **Guest port collision.** If a user process inside the sandbox already listens on the guest port, should forward creation fail visibly, or should the port vary per sandbox? A per-sandbox port would mean per-sandbox URLs in workspace files.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `Could not resolve host: host.containers.internal` inside a microsandbox sandbox | `host.containers.internal` is a Podman-only name; microsandbox guests only know `host.microsandbox.internal` | Point agents at the guest-loopback URL from `LOCAL_LLM_BASE_URL` in `src/lib/agents/local-llm.ts`, never at a host alias |
| `curl host.microsandbox.internal:<port>` fails although the inference server is up | The microsandbox egress policy allows the `host` group on port 53 only, and the host server binds loopback | Reach the server through the per-sandbox reverse forward, not through guest egress |
| Mutagen forward to a microsandbox alias fails with `host environment variable MIO_SOCKS_PASSWORD is not set` | The sandbox was stopped; the alias `ProxyCommand` (`msb exec`) tried to boot it without the start environment from `src-tauri/src/services/start_env.rs` | Create inference forwards only for running sandboxes, after the app has started them |
| The guest port shows up as an auto-exposed user port in the ports list | A forward's guest listener created outside `NetworkManager` is not in the port poller's exclude set (`src-tauri/src/network/mod.rs`) | Create inference forwards through `NetworkManager` as `ServiceKind::Inference` |
| pi keeps an old URL or model list after the server or catalogue changes | `reprovisionInstalledAgents` (`src/lib/agents/reprovision.ts`) handled OpenCode only | Route every agent with a catalogue `provision` through the shared provisioning helper |
| A stopped microsandbox sandbox flips between `stopped` and `starting`, or a forward reports `sandbox '<name>' is in state Starting and cannot be started` | A live Mutagen session, or a poller calling `msb exec`, against a stopped microsandbox sandbox makes the alias `ProxyCommand` try to boot the VM | Terminate the sandbox's inference forward when the sandbox stops; never leave an `inference-` session pointing at a stopped sandbox |
| pi ignores an exported `PI_CODING_AGENT_DIR` when run from the workspace | `provision_pi` (`src-tauri/src/agents/config.rs`) writes `PI_CODING_AGENT_DIR` into the workspace `mise.toml` env, and mise re-applies it inside the workspace | Change pi's config by editing `.mio/pi-agent/models.json` in the workspace, or run pi from outside the workspace when testing an alternative config |
| Agents lose the model after it is restarted on the Models page | The host port changed and Mutagen fixes a forward's destination at create | Recreate every `inference-` forward on server start in the inference reach service |
