# OpenCode Agent Ideas

This document covers the ideation for adding **OpenCode** as a second installable coding agent in sandbox-mio, alongside the existing Claude Code agent, and — the distinguishing requirement — for **pre-configuring OpenCode on install** so that a local-LLM provider is already wired up out of the box, pointing at the host inference server delivered by the `local-llm` feature (`docs/features/local-llm/`). The agent-installation half builds on an existing, working seam: the static agent catalogue (`src/lib/agents/catalogue.ts`), the per-sandbox install flow (`install_agent` / `agents::install_packages` in `src-tauri/src/lib.rs` and `src-tauri/src/agents/mod.rs`), and the `AgentManager.svelte` UI. The pre-configuration half is, in effect, the concrete realization of the local-llm feature's **unimplemented Phase G** (in-sandbox endpoint wiring; `docs/features/local-llm/02_dev.md` lines 333–339), which already names OpenCode as its first intended consumer. Nothing for OpenCode exists in the codebase today; the local-llm host inference server (Phases A–F) is implemented but its sandbox-facing wiring is not. The focus is Windows, matching the local-llm feature's current scope.

## Current State

The codebase has exactly one agent today. The agent model is descriptor-driven and per-sandbox:

- `src/lib/agents/catalogue.ts` — a static `AGENTS: AgentDescriptor[]` array with a single `claude-code` entry. Each descriptor carries `misePackages` (installed into the container), `postInstallSteps` (the npm-postinstall workaround for mise's `--ignore-scripts=true`), and a **mandatory** `auth` field.
- `src/lib/agents/types.ts` — `AgentDescriptor` requires `auth: AuthStrategy`, and `AuthStrategy` is currently a single-variant union: `ExecInteractiveAuth` (kind `"exec-interactive"`), which assumes an interactive OAuth/PKCE login that writes a credential JSON file the app later reads back. This is the first seam that does not fit OpenCode (see below).
- `src-tauri/src/agents/mod.rs` — `install_packages` runs `mise install` per package inside the named container via `RuntimeManager`, streaming `AgentInstallEvent`s; `run_postinstall_npm` runs the postinstall script; `start_auth` / `send_auth_input` drive the interactive credential flow. All operations are scoped to one sandbox container.
- `src-tauri/src/lib.rs` — `install_agent` (line 393) and the `start_agent_auth` family (line 412+) are the registered Tauri commands; `AgentManager.svelte` is the consumer.
- `src/routes/projects/AgentManager.svelte` — renders installed/available agents per `SandboxItem`, derives "installed" by checking the workspace package manifest against each descriptor's `misePackages` (via `miseKey`), runs installs with a progress log, and opens the auth window. There is no notion of an agent that needs *configuration* rather than *authentication*.

The local-llm seams this feature consumes:

- The **host inference server** (`src-tauri/src/inference/`, implemented Phases A–F) exposes an OpenAI-compatible endpoint on a host loopback port; `inference_status` returns its live `base_url` (e.g. `http://127.0.0.1:<port>/v1`). The port is **ephemeral** — allocated per start via the bind-and-release trick in `ephemeral_port` (`src-tauri/src/lib.rs` line 465), so it changes between runs.
- The **frontend model catalogue** (`src/lib/models/catalogue.ts`) lists the models the inference server can serve, each with an `id`. OpenCode's provider config needs a matching model list.
- **Phase G is unimplemented** (`docs/features/local-llm/02_dev.md`): the planned `forward_create_reverse` helper in `src-tauri/src/sync/mod.rs` (a host-port-into-container Mutagen forward) and a `wire_sandbox_to_inference` command that writes `OPENAI_BASE_URL` + a dummy `OPENAI_API_KEY` into the workspace. Its gating spike — confirming a container process can reach a host loopback server through a reverse Mutagen forward, and that OpenCode honours the injected base URL — was **run and cleared on 2026-06-29 as part of this feature's ideation** (see Validation), so the `02_dev.md` build can proceed against a proven topology rather than an assumption.

## Proposed Model

### OpenCode catalogue entry (`src/lib/agents/catalogue.ts`)

A second `AgentDescriptor` with `id: "opencode"`. The install mechanism differs from Claude Code: OpenCode is distributable through mise's own registry (`mise use opencode`, confirmed available — see Validation), which fetches a self-contained platform binary and therefore needs neither a `node@lts` dependency nor the npm-postinstall workaround that Claude Code requires. The descriptor's `misePackages` is a single `opencode` entry and `postInstallSteps` is empty for the install-the-binary part — but gains a **new** post-install step kind that writes the default provider config (next section). Installation otherwise reuses `install_packages` unchanged.

### Auth-model generalization: agents that configure instead of authenticate (`src/lib/agents/types.ts`)

OpenCode pointed at a *local* provider requires **no login** — the OpenCode docs confirm a local OpenAI-compatible provider is fully defined by `opencode.json` and does not go through OpenCode's `/connect` credential flow (see Validation). This breaks the current invariant that `AgentDescriptor.auth` is mandatory and always `exec-interactive`. The proposed change makes `auth` optional (or adds an `AuthStrategy` variant `kind: "none"`), and `AgentManager.svelte`'s status logic treats a config-only agent as "ready" once installed and provisioned, rather than showing the "not authenticated" amber state. Cloud OpenCode providers (which *do* need keys) are out of v1 scope; the descriptor is shaped so they can be added later as a separate `AuthStrategy` variant.

### Default provider provisioning (new post-install step + `provision_opencode` command)

This is the feature's headline requirement: *on installation, a provider is already available*. A new post-install step kind — provisional name `write-provider-config` — runs after the binary install and writes an `opencode.json` into the sandbox declaring a provider (id `local-llm`) that uses the `@ai-sdk/openai-compatible` npm loader, a `baseURL` pointing at the in-sandbox inference endpoint, a dummy `apiKey`, and a `models` map mirroring the local-llm catalogue (`src/lib/models/catalogue.ts`). Because OpenCode merges config files and later configs override earlier ones only for conflicting keys (Validation), writing a focused provider block does not clobber any config the user later adds. The seam is a new Tauri command (e.g. `provision_opencode`) in `src-tauri/src/lib.rs`, invoked by `AgentManager.svelte` immediately after a successful `install_agent`, that materializes the file in the container via `RuntimeManager::exec_in_container` (the same path `get_auth_status` uses to `cat`/write container files).

### Stable in-sandbox endpoint via a fixed-port reverse forward (realizes local-llm Phase G — spiked 2026-06-29)

The host inference port is ephemeral, but a config file committed into a sandbox needs a **stable** address. The spike (Validation) proved the working topology, which corrects an earlier wrong assumption that the container could reach the VM's `127.0.0.1`: it cannot. The resolution that works is: the reverse Mutagen forward (`forward_create_reverse`, the Phase G helper) listens on the Podman-machine VM's **`0.0.0.0:<FIXED_VM_PORT>`** (all VM interfaces, not VM loopback) and dials through to whatever ephemeral host port the inference server currently holds. A process inside the sandbox container then reaches it at **`http://host.containers.internal:<FIXED_VM_PORT>/v1`** — `host.containers.internal` is pasta's gateway address into the VM (it resolved to `169.254.1.2` on the spike machine). Because the VM port is fixed, the container-side address is constant across inference restarts, so `opencode.json` carries a literal `baseURL` and never needs rewriting when the host port changes — the forward indirection absorbs the dynamic host port. `wire_sandbox_to_inference` (re)establishes the forward whenever inference is running and a sandbox wants it; the OpenCode config is written once at install time. This sidesteps any reliance on OpenCode interpolating `{env:...}` inside `baseURL`.

Two pasta facts make the `0.0.0.0`-listener mandatory (both spiked): the container is created with `--network pasta:--host-lo-to-ns-lo` (`src-tauri/src/backends/mod.rs` line 326), which bridges only the **published** container port to the namespace loopback — it does **not** expose arbitrary VM-loopback services on the container's `127.0.0.1`. And a Mutagen listener bound to the VM's `127.0.0.1` is invisible to the container (the container has its own netns); only a listener on a VM interface the gateway routes to (`0.0.0.0`) is reachable via `host.containers.internal`.

### Model list synchronization (`src/lib/models/catalogue.ts` → provider `models`)

OpenCode requires the served model IDs to be enumerated in the provider's `models` map; an unknown model ID is unusable. The provider block written at provision time is generated from the same local-llm catalogue the inference server serves, so the two never drift. One subtlety carried over from local-llm: the **Foundry** engine serves a device-specific resolved ID (e.g. `qwen2.5-0.5b-instruct-openvino-npu:5`), not the catalogue alias (`docs/features/local-llm/02_dev.md` line 163). Whether the OpenCode `models` map should list catalogue aliases or resolved IDs depends on which engine is active and is an open question.

### UI flow (`src/routes/projects/AgentManager.svelte`)

OpenCode appears in the existing "Add agent" picker. Installing it runs the binary install (progress log, as today) and then the provisioning step, after which the agent row shows a "local-llm provider configured" ready state instead of the auth prompt. If the local-llm inference server is not yet set up, the row surfaces a hint linking to the `/models` route rather than failing — the provider config is still valid; it simply has nothing to talk to until inference is started and the sandbox is wired.

## Technology Choices with Tradeoffs

### Install mechanism: mise `opencode` registry tool (not `npm:opencode-ai`, not the curl script)

Chosen: install OpenCode through mise's native `opencode` tool, reusing `install_packages` with no npm-postinstall step.

Alternatives evaluated:

- **`npm:opencode-ai` (mirror the Claude Code descriptor exactly)** — rejected as the primary path. It would force a `node@lts` dependency and the npm-postinstall workaround into the descriptor, and the `opencode-ai` npm package is itself a thin wrapper that downloads a platform binary on postinstall, duplicating work mise's registry entry already does cleanly. Retained as a fallback if the mise registry entry proves unavailable on a target platform.
- **The official `curl -fsSL https://opencode.ai/install | bash` script** — rejected because it installs outside mise's tool management, so `AgentManager.svelte`'s "installed?" derivation (which checks the workspace mise manifest) would not see it, and uninstall/version pinning would fall outside the app's existing model.

### Provider wiring: explicit `opencode.json` provider block (not the built-in `openai` provider via env vars)

Chosen: write an explicit custom provider (id `local-llm`, loader `@ai-sdk/openai-compatible`) into `opencode.json`.

Alternatives evaluated:

- **Rely on OpenCode's built-in `openai` provider plus `OPENAI_BASE_URL` / `OPENAI_API_KEY` env vars** (what local-llm Phase G literally proposed) — rejected as the primary mechanism because it overloads the *real* OpenAI provider with a local endpoint, which is surprising if the user later adds genuine OpenAI credentials, and because relying on `OPENAI_BASE_URL` being honoured is exactly the unvalidated assumption flagged in Phase G. An explicit named provider is self-describing in the UI ("local-llm") and cannot collide with a future real OpenAI setup. The env-var approach is kept as the fallback the spike will compare against.
- **A global `~/.config/opencode/opencode.json`** instead of a project-level file — rejected for v1 because the app's whole model is per-sandbox isolation; a global config would leak the provider into every project and every OpenCode invocation on that container image. Project-level config matches the sandbox boundary. (OpenCode merges both, so a future shared default could still live globally.)

### Endpoint addressing: fixed container port via reverse forward (not literal dynamic URL, not `{env:}` interpolation)

Chosen: a fixed in-container loopback port fronting the ephemeral host port through the reverse Mutagen forward, so the config carries a stable literal `baseURL`.

Alternatives evaluated:

- **Write the live host `base_url` literally into `opencode.json`** — rejected because the host inference port is ephemeral (`ephemeral_port`, `src-tauri/src/lib.rs` line 465) and changes every restart, so the committed config would silently go stale and require rewriting on each inference start.
- **`{env:OPENAI_BASE_URL}` interpolation in `baseURL`** — rejected as the baseline because the OpenCode docs confirm `{env:}` substitution for provider options in general but do **not** explicitly confirm it inside `baseURL` (Validation), making it an unproven dependency; it also reintroduces the per-start rewrite of the env var. The fixed-port approach needs neither.

### Auth model: make `auth` optional / add a `none` strategy (not force a fake auth flow)

Chosen: generalize `AgentDescriptor` so an agent can be config-provisioned with no auth step.

Alternative evaluated: **keep `auth` mandatory and model the provider write as a degenerate `exec-interactive` flow** — rejected because the credential-readback machinery (`get_auth_status`, `credentialFile`, `expiresAtJsonKey`) has no meaning for a keyless local provider, and shoe-horning it would make `AgentManager.svelte` display a perpetual "not authenticated" state for a perfectly working agent.

### Default served model: the local-llm catalogue's recommended pick

Chosen: the provider `models` map mirrors the local-llm catalogue, with OpenCode's `default_agent`/model defaulting to the same model `recommendModels` highlights (Phi-4-mini-reasoning per `docs/features/local-llm/02_dev.md` line 26). Alternative — **hard-code a single model** — rejected because it would drift from the local-llm catalogue the inference server actually serves; generating from one source keeps them consistent.

## Validation

A full end-to-end spike was run on 2026-06-29 on the developer's Windows 11 machine and **cleared the central runtime risk** (in-sandbox reachability) end to end, before any feature code was written. A separate set of facts was established by documentation research the same day. Both are recorded below.

### Spike: OpenCode-in-container → reverse Mutagen forward → host inference (2026-06-29, cleared)

The spike reused the implemented local-llm host inference server, the bundled `llama-server.exe` (`src-tauri/binaries/llama/`), an already-downloaded GGUF (`qwen2.5-1.5b-instruct-q4_k_m.gguf`), the user-installed Mutagen (`~/.local/bin/mutagen`, same CLI the app bundles), and a running sandbox container (`gg`, `sandbox-mio/base:2`, Debian 12). Each risk and the command that cleared it:

- **Host inference serves completions.** Started `llama-server.exe --model <gguf> --port 18080 --host 127.0.0.1 -ngl 99`; `GET /health` → `200`; `POST /v1/chat/completions` returned `choices[0].message.content` = `"pong"`.
- **A VM-loopback reverse forward is NOT reachable from the container.** `mutagen forward create --name=… "podman-machine-default:tcp:127.0.0.1:18099" "tcp:127.0.0.1:18080"` connected fine and the VM itself returned `200` over SSH, but from inside `gg` both `curl http://127.0.0.1:18099/health` and `curl http://host.containers.internal:18099/health` returned `000` (no connection). This **disproved the original "fixed `127.0.0.1` container port" design.**
- **A VM-`0.0.0.0` reverse forward IS reachable from the container via `host.containers.internal`.** Recreating the forward with source `podman-machine-default:tcp:0.0.0.0:18099` (verified with `ss -tln` on the VM showing `*:18099`), from inside `gg`: `curl http://host.containers.internal:18099/health` → `200`, and a full `POST …/v1/chat/completions` → `"pong"`. `host.containers.internal` resolved to `169.254.1.2` (`getent hosts` in the container). `127.0.0.1:18099` still returned `000`, confirming the pasta `--host-lo-to-ns-lo` flag bridges only the published port.
- **OpenCode installs in the container via mise.** `mise use -g opencode` downloaded and installed `opencode@1.17.11` (a self-contained `opencode-linux-x64.tar.gz` binary — no Node/npm-postinstall needed), confirming the mise-install choice over `npm:opencode-ai`.
- **OpenCode drives the local provider with no auth, end to end.** Wrote a project `opencode.json` declaring provider `local-llm` (`npm: "@ai-sdk/openai-compatible"`, `options.baseURL: "http://host.containers.internal:18099/v1"`, `options.apiKey: "sk-local"`, one `models` entry, top-level `model: "local-llm/qwen2.5-1.5b-instruct"`). `opencode run "Reply with exactly one word: ping"` printed `build · qwen2.5-1.5b-instruct` and `pong`, exit `0` — no login step. OpenCode auto-installed the `@ai-sdk/openai-compatible` loader on first run.
- **Model-id mismatch is tolerated by the llama engine.** `llama-server` reports its model name as the full GGUF path, yet OpenCode sent the config alias `qwen2.5-1.5b-instruct` and the completion still succeeded — llama.cpp's single-model server does not 400 on a mismatched `model` field. (This resolves the llama half of the "models map contents" open question; the Foundry half — which serves a strict device-specific id — remains open.)

Spike artifacts were cleaned up afterward: the forward was terminated, the host `llama-server` stopped, and the test `opencode.json` removed from the bind-mounted workspace. (The container retains `opencode` in its global mise config, which is the desired installed state anyway.)

### Cleared by research (2026-06-29):

- **OpenCode defines local providers purely in config, with no login.** The OpenCode providers docs confirm a custom OpenAI-compatible provider is declared in `opencode.json` via `npm: "@ai-sdk/openai-compatible"` with `options.baseURL` + `options.apiKey` and a `models` map, and that local providers do not require the `/connect` credential flow. Source: <https://opencode.ai/docs/providers/>. This is the evidence behind the auth-model generalization.
- **OpenCode is installable via mise.** `mise use -g opencode` is a documented install path (npm package name is `opencode-ai`; latest `1.17.11`). Sources: <https://www.npmjs.com/package/opencode-ai>, search-confirmed mise registry entry. This is the evidence for the mise-install choice over the npm/curl alternatives.
- **OpenCode config files merge rather than replace, project over global.** "Configuration files are merged together… Later configs override earlier ones only for conflicting keys." Source: <https://opencode.ai/docs/config/>. This is why writing a focused `local-llm` provider block is safe and does not clobber later user config.
- **`{env:}` and `{file:}` interpolation exist, but `baseURL` support is unconfirmed.** The config docs document `{env:VARIABLE}` and `{file:path}` substitution for provider options generally; they do not explicitly state it works inside `baseURL`. Source: <https://opencode.ai/docs/config/>. This uncertainty is moot for v1 because the spike's fixed-port design writes a literal `baseURL`, but it remains relevant if a future iteration prefers env-var indirection.

## Open Questions

- **Reverse-forward in-container address** *(resolved by the 2026-06-29 spike; recorded here as the decision's origin)*: The container reaches the forward at `host.containers.internal:<FIXED_VM_PORT>`, and the Mutagen listener must bind the VM's `0.0.0.0` (not `127.0.0.1`). The `02_dev.md` `forward_create_reverse` helper must therefore take a VM-`0.0.0.0` source, and `wire_sandbox_to_inference` must write `http://host.containers.internal:<FIXED_VM_PORT>/v1` into the config. Remaining sub-question: which fixed VM port to reserve, and whether it can collide with a user's own published ports.
- **`models` map contents per engine** *(llama half resolved by the spike)*: The spike confirmed the **llama** engine tolerates a config alias that differs from its reported model id, so the provider `models` map can list catalogue aliases for llama. The **Foundry** engine still serves a strict device-specific resolved id (`…-openvino-npu:5`) that requests must match (`docs/features/local-llm/02_dev.md` line 163), so a config written at install time predates knowing that id. Resolve whether the Foundry path rewrites the OpenCode `models` map when inference starts, or whether v1 wires OpenCode only to the llama engine.
- **Provisioning timing vs. inference availability**: Should `provision_opencode` run unconditionally at agent install (provider configured, endpoint dead until inference starts), or be deferred/re-run by `wire_sandbox_to_inference` when the user first starts a model for that sandbox? The first is simpler; the second avoids a configured-but-dead provider window.
- **Config file location in the container**: Project-level `/root/work/opencode.json` (inside the synced workspace, so it round-trips to the user's machine via Mutagen) versus a container-only path like `~/.config/opencode/opencode.json` (not synced, invisible to the user's repo). Syncing it documents the setup but adds a file to the user's tree; keeping it container-only hides it. 
- **Auth-strategy shape**: Make `AgentDescriptor.auth` optional, or add an explicit `AuthStrategy` variant `kind: "none"`? The choice affects every `auth`-reading site in `AgentManager.svelte` and the `02_dev.md` type changes.
- **Multiple agents per sandbox**: With two agents now installable, does the UI/derivation assume Claude Code and OpenCode can coexist in one container, and do their mise packages and configs conflict? The current `installedAgents` derivation already supports multiple, but this is untested with two real entries.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| OpenCode shows "not authenticated" forever despite working | `AgentManager.svelte` assumes every agent has an `exec-interactive` auth and a credential file to read back | Generalize `AgentDescriptor.auth` to optional / a `none` variant; treat a config-provisioned agent as ready once the provider file is written |
| OpenCode provider points at a dead/stale endpoint after an inference restart | The host inference port is ephemeral (`ephemeral_port`, `src-tauri/src/lib.rs` line 465) and a literal host URL in `opencode.json` goes stale | Front the ephemeral host port with a **fixed VM port** via the reverse Mutagen forward; write the stable `http://host.containers.internal:<FIXED_VM_PORT>/v1` into the config so it survives host-port changes |
| Container cannot reach the host inference server at all | The container has its own netns: the VM's `127.0.0.1` is invisible to it, and pasta `--host-lo-to-ns-lo` bridges only the published port — a reverse forward bound to VM `127.0.0.1` returns connection-refused from the container (spiked 2026-06-29) | Bind the reverse Mutagen forward to the VM's `0.0.0.0` and have the container dial `host.containers.internal:<port>` (proven reachable), never `127.0.0.1` |
| `{env:OPENAI_BASE_URL}` in `baseURL` silently fails | OpenCode documents `{env:}` for provider options but not specifically inside `baseURL` | Do not depend on `baseURL` interpolation; write a literal stable URL (fixed-port design) until a spike proves interpolation works |
| OpenCode rejects requests with an unknown-model error | The served model ID is not listed in the provider `models` map, or the Foundry resolved ID (`…-openvino-npu:5`) differs from the catalogue alias | Generate the `models` map from `src/lib/models/catalogue.ts`; reconcile alias-vs-resolved-ID per the open question before relying on Foundry |
| Provisioned provider block clobbers the user's later OpenCode config | Assuming config files replace rather than merge | OpenCode merges configs (later overrides only conflicting keys); keep the written block scoped to the `local-llm` provider id only |
| OpenCode install not detected as installed | Installing via the curl script or a raw binary bypasses mise, so the workspace manifest check in `AgentManager.svelte` never sees it | Install through the mise `opencode` tool so the existing `miseKey`-based "installed?" derivation works unchanged |
</content>
</invoke>
