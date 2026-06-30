# OpenCode Agent Build Plan

Build plan for adding **OpenCode** as a second installable coding agent (`docs/features/opencode-agent/01_ideas.md`), pre-wired on install to the local-LLM host inference server. It continues the ideas document's two halves — an OpenCode catalogue entry that installs via mise, and the realization of local-llm's Phase G in-sandbox endpoint wiring — and supersedes one ideas-stage assumption: the reverse forward is **app-wide**, not per-sandbox, because the inference server is a single app-wide process and the Mutagen listener lives on the shared Podman-machine VM (so every local container reaches it through `host.containers.internal`). It also supersedes the ideas document's hedge between a `127.0.0.1` and a `host.containers.internal` container address: the 2026-06-29 spike settled on `host.containers.internal:<FIXED_VM_PORT>` with the listener on the VM's `0.0.0.0`. **Status: nothing in this plan is implemented yet — no OpenCode code exists in the codebase.** Every phase below is `Remaining`. The local-llm host inference server (its Phases A–F) is already implemented and is a dependency, not part of this plan.

## The Concrete Model

OpenCode is a descriptor in the existing static agent catalogue (`src/lib/agents/catalogue.ts`), installed per-sandbox through the same `install_agent` → `agents::install_packages` path Claude Code uses, with one mise package (`opencode`) and **no** npm-postinstall step (the spike confirmed mise ships a self-contained `opencode` binary). The descriptor declares **no auth** and instead a **provider-provisioning** step. After the binary install, the frontend calls a new `provision_opencode` command that writes an `opencode.json` into the sandbox declaring a single custom provider (`local-llm`, loader `@ai-sdk/openai-compatible`) whose `baseURL` is a fixed, stable in-container address.

That address is served by a single **app-wide reverse Mutagen forward**: when the local-llm inference server starts, the app creates one forwarding session (`inference-reverse`) that listens on the Podman-machine VM's `0.0.0.0:INFERENCE_VM_PORT` and dials the inference server's live (ephemeral) host loopback port. Every local sandbox container reaches the inference server at `http://host.containers.internal:INFERENCE_VM_PORT/v1` — a constant address, because `INFERENCE_VM_PORT` is fixed and the forward absorbs the changing host port. The forward's lifecycle is bound to the inference server's: created in `start_inference`, torn down in `stop_inference` and on app exit. OpenCode config is written once at provisioning time and never rewritten when inference restarts.

The OpenCode `models` map is generated from the same local-llm model catalogue (`src/lib/models/catalogue.ts`) the inference server serves, so the two cannot drift. Because the spike proved the llama engine tolerates a `model` field that differs from its reported id, the map lists catalogue aliases. v1 wires OpenCode to **local sandboxes against the llama engine only**; Foundry's strict resolved-id requirement and remote sandboxes are deferred.

## Resolved Decisions

Each Open Question from `01_ideas.md`, with its build-time resolution:

- **Reverse-forward in-container address** — Resolved by the spike: the Mutagen listener binds the VM's `0.0.0.0:INFERENCE_VM_PORT` and containers dial `http://host.containers.internal:INFERENCE_VM_PORT/v1`. The "which fixed VM port" sub-question is resolved by reserving the app-owned constant **`INFERENCE_VM_PORT = 51199`** (a high, uncommon port unlikely to collide with a user's published ports), declared once in Rust (`src-tauri/src/sync/mod.rs`) and mirrored in one frontend module (`src/lib/agents/opencode.ts`), with both sites cross-referenced so they stay in sync.
- **`models` map contents per engine** — Resolved for v1: the provider `models` map lists **catalogue aliases** (proven to work against the llama engine). The Foundry engine is **not** wired to OpenCode in v1 (its device-specific resolved id would require rewriting the map at inference-start time); this is recorded as deferred, not built.
- **Provisioning timing vs. inference availability** — Resolved: `provision_opencode` runs **unconditionally at agent install**, independent of whether inference is running. The config is valid immediately; the endpoint simply has nothing to answer until inference starts and the app-wide forward exists. The forward is managed separately by the inference lifecycle, so there is no configured-but-permanently-dead window — starting inference activates the already-written config.
- **Config file location in the container** — Resolved: write the project-root **`<workspace>/opencode.json`** host-side (the workspace is bind-mounted, so the write is a plain host `std::fs::write`, exactly like `open_terminal` writes `<workspace>/.mio/zellij.json`; no `podman exec` JSON-escaping). It is OpenCode's native auto-discovered project config and round-trips to the user via sync so it is visible and editable. Deferred alternative (recorded so it is not lost): relocating to `<workspace>/.mio/opencode.json` plus an `OPENCODE_CONFIG` env var injected via `mise.toml [env]`, if writing a file into the user's project root proves unwelcome.
- **Auth-strategy shape** — Resolved: make `AgentDescriptor.auth` **optional** (`auth?: AuthStrategy`) rather than adding a `kind: "none"` variant, because the natural reading of "no auth" is the absence of an auth strategy, and every `auth`-reading site in `AgentManager.svelte` already needs a presence check once a second agent exists.
- **Multiple agents per sandbox** — Resolved: Claude Code and OpenCode coexist. The existing `installedAgents` derivation already supports multiple descriptors; their mise packages (`npm:@anthropic-ai/claude-code` + `node@lts` vs `opencode`) do not collide, and their configs live in different files (`/root/.claude*` vs `/root/work/opencode.json`). No structural change is needed beyond the auth-optionality of Phase A.

## Remaining: Phase A — Auth-model generalization and the provision step type (`src/lib/agents/types.ts`)

Make an agent able to *configure* instead of *authenticate*.

- `AgentDescriptor.auth` becomes optional: `auth?: AuthStrategy`. Every consumer that reads `auth` (currently only `AgentManager.svelte`) must guard on its presence — see Phase F.
- Add a provider-provisioning descriptor field: `provision?: ProviderProvisionConfig`, where

  ```ts
  export interface ProviderProvisionConfig {
    kind: "opencode-provider";   // discriminant for future provisioners
    providerId: string;          // "local-llm"
    relativeConfigPath: string;  // "opencode.json" (relative to the workspace root)
  }
  ```

  The descriptor carries only the *shape* of the provisioning; the concrete provider JSON and default model are generated at call time from the model catalogue by the helper in Phase B (so the catalogue is the single source of truth, not a frozen copy in the descriptor).
- No change to `AgentInstallEvent` / `AgentAuthStatus` types; provisioning reports through the install action's existing busy-tracking, not a new event stream (the write is a single fast host-side operation).

## Remaining: Phase B — OpenCode catalogue entry and provider-config generator

### `src/lib/agents/opencode.ts` (new)

Holds the OpenCode-specific constants and the provider-config generator, kept out of the generic `catalogue.ts` so the catalogue stays a plain descriptor list:

- `export const INFERENCE_VM_PORT = 51199;` — **must equal** the Rust `INFERENCE_VM_PORT` in `src-tauri/src/sync/mod.rs`; comment both sites with a mutual reference.
- `export const LOCAL_LLM_BASE_URL = \`http://host.containers.internal:${INFERENCE_VM_PORT}/v1\`;`
- `export function buildLocalLlmProvider(): { providerId: string; providerJson: unknown; defaultModel: string }` — generates the provider block from `MODELS` (`src/lib/models/catalogue.ts`): `npm: "@ai-sdk/openai-compatible"`, `name: "Local LLM"`, `options: { baseURL: LOCAL_LLM_BASE_URL, apiKey: "sk-local" }`, and a `models` map keyed by each catalogue `id` (all three are llama-capable) with `{ name: label }`. `defaultModel` is `local-llm/phi-4-mini-reasoning` (the recommended pick, matching `recommendModels`). The dummy `apiKey` is required because `@ai-sdk/openai-compatible` sends an `Authorization` header; the spike used `sk-local` and llama-server ignored it.

### `src/lib/agents/catalogue.ts`

Add a second `AgentDescriptor`:

```ts
{
  id: "opencode",
  label: "OpenCode",
  description: "Open-source terminal coding agent, pre-wired to your local LLM",
  misePackages: ["opencode"],
  postInstallSteps: [],
  // no `auth`
  provision: { kind: "opencode-provider", providerId: "local-llm", relativeConfigPath: "opencode.json" },
}
```

`miseKey("opencode")` already returns `"opencode"` (no scheme, no `@version`), so the existing `installedAgents` derivation in `AgentManager.svelte` detects it from the workspace `mise.toml [tools]` with no change.

## Remaining: Phase C — `provision_opencode` command (`src-tauri/src/lib.rs`)

A host-side, merge-preserving JSON writer. Mirrors how `open_terminal` writes session JSON host-side into the bind-mounted workspace (`src-tauri/src/lib.rs` lines 149–160), so it needs no `podman exec` and works whether the container is running or stopped.

```rust
/// Write/merge the OpenCode provider config into <workspace_path>/<relative_path> host-side.
/// Preserves any existing opencode.json: sets provider[provider_id] and ensures $schema, and
/// sets the top-level `model` only when absent, so a user's own config is never clobbered.
#[tauri::command]
async fn provision_opencode(
    workspace_path: String,
    relative_path: String,            // "opencode.json"
    provider_id: String,              // "local-llm"
    provider_json: serde_json::Value, // the provider block from buildLocalLlmProvider()
    default_model: Option<String>,    // "local-llm/phi-4-mini-reasoning"
) -> Result<(), AppError>;
```

Behavior: read `<workspace_path>/<relative_path>` if present and parse it (`{}` if absent or unparseable-but-empty; a genuine parse error of non-empty content returns an `AppError` rather than overwriting). Set `obj["$schema"] = "https://opencode.ai/config.json"`, `obj["provider"][provider_id] = provider_json`, and `obj["model"] = default_model` only if `obj["model"]` is absent. Write back pretty-printed. The merge implements the ideas document's "scoped to the `local-llm` provider id only" guarantee. Register in the `generate_handler!` list alongside `install_agent` (`src-tauri/src/lib.rs` line 641). A new error variant is not required — reuse `AppError` via an `std::io`/`serde_json` mapping consistent with the file-write error handling already in `open_terminal`.

## Remaining: Phase D — Reverse forward primitive (`src-tauri/src/sync/mod.rs`)

Add the mirror of `forward_create` (which is host-listen → remote-connect). The reverse is **remote(VM)-listen → host-connect**, with the listener on the VM's `0.0.0.0` so it is reachable from container netns via `host.containers.internal` (spike-proven; a VM-`127.0.0.1` listener is invisible to containers).

```rust
/// App-owned VM port the inference reverse-forward listens on. Containers reach the host
/// inference server at http://host.containers.internal:<INFERENCE_VM_PORT>/v1. Must equal the
/// frontend INFERENCE_VM_PORT in src/lib/agents/opencode.ts.
pub const INFERENCE_VM_PORT: u16 = 51199;

/// Reverse of `forward_create`: listen on `remote_alias`'s 0.0.0.0:`remote_listen_port` and dial
/// the host's 127.0.0.1:`host_port`. The 0.0.0.0 bind is required so container netns can reach the
/// listener via host.containers.internal (a 127.0.0.1 bind is VM-loopback-only; spiked 2026-06-29).
/// Idempotent: terminates any prior session of `name` first.
pub async fn forward_create_reverse<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    remote_alias: &str,
    remote_listen_port: u16,
    host_port: u16,
) -> Result<(), SyncError>;
```

Implementation: `source = format!("{remote_alias}:tcp:0.0.0.0:{remote_listen_port}")`, `dest = format!("tcp:127.0.0.1:{host_port}")`, then `forward create --name=<name> <source> <dest>`, reusing the existing `run` helper and the best-effort `forward terminate` preamble that `forward_create` already uses. `forward_terminate` (already present) is reused to tear it down.

## Remaining: Phase E — Bind the forward to the inference lifecycle (`src-tauri/src/lib.rs`)

The app-wide forward exists exactly when the inference server is running and only over the local connection (remote sandboxes cannot reach the host's inference server; v1 is local-only).

- Add a constant forward name `const INFERENCE_FORWARD: &str = "inference-reverse";` (module scope in `lib.rs`).
- In `start_inference` (`src-tauri/src/lib.rs` line 515), after the server is `Ready` and its `ServerHandle` is stored, create the forward: resolve the local connection from the managed `Arc<dyn RuntimeManager>` (`runtime.local_connection()`), call `crate::backends::ensure_connection_alias(connection)` first (so Mutagen can resolve the SSH alias, exactly as `open_terminal` does at line 124), then `sync::forward_create_reverse(&app, INFERENCE_FORWARD, connection, sync::INFERENCE_VM_PORT, handle.port)`. The dialed host port is `ServerHandle.port` (`src-tauri/src/inference/types.rs` line 83). Forward-creation failure is logged but does **not** fail `start_inference` — the host-side `/models` chat surface still works without the in-sandbox bridge; a failed forward only blocks the OpenCode-in-container path.
- In `stop_inference` (line 542) and in the `RunEvent::Exit` hook (line 664, which already stops the active server then the Mutagen daemon), call `sync::forward_terminate(&app, INFERENCE_FORWARD)` before the daemon stop, so the reverse forward is never orphaned.
- Because `start_inference` already stops any prior server before starting the next (the one-model-at-a-time mutex), and it recreates the forward against the new `handle.port`, the forward automatically re-points at each new inference run. No separate "rewire" command is needed.

This supersedes the ideas document's per-sandbox `wire_sandbox_to_inference` command: there is one forward for the whole app, created by the inference lifecycle, not one per sandbox.

## Remaining: Phase F — AgentManager UI (`src/routes/projects/AgentManager.svelte`)

Render and provision the config-only agent without an auth flow.

- **Auth-presence guards.** `loadAgentStatuses` currently iterates `installedAgents` and calls `get_agent_auth_status` using `agent.auth.credentialFile`. Guard each call with `if (!agent.auth) continue;` so OpenCode (no `auth`) is skipped. The row-rendering block that shows the authenticated/not-authenticated state and the Authenticate button must branch: when `agent.auth` is undefined, render a provider-status line instead (next point).
- **Provision on install.** After a successful `install_agent` in `installAgent`, if `descriptor.provision` is set, build the provider with `buildLocalLlmProvider()` (Phase B) and call `provision_opencode({ workspacePath, relativePath: descriptor.provision.relativeConfigPath, providerId: descriptor.provision.providerId, providerJson, defaultModel })`. Append a "✓ local-llm provider configured" line to the existing `installLog`. Keep it inside the same `action.run(...)` busy scope so the row stays disabled until provisioning completes.
- **Ready/needs-inference state.** For a provisioned agent, the row shows "local-llm provider configured". To tell the user whether the endpoint is live, call `inference_status` (already a registered command) on mount: if `running` is false, show a secondary hint linking to the `/models` route ("Start a model to use OpenCode") rather than an error — the config is valid regardless. This realizes the ideas document's "surface a hint rather than failing" behavior.
- **i18n keys.** Add to `messages/en.json` (then mirror to `de.json`/`ru.json` per AGENTS.md): `projects_agent_provider_configured` ("local-llm provider configured"), `projects_agent_start_model_hint` ("Start a model to use OpenCode"), and a provisioning progress line `projects_agent_provisioning` ("Configuring local-llm provider…"). The existing `projects_agent_*` keys are reused for the Claude Code path unchanged.

## Lifecycle / Cross-Cutting Concerns

**Single app-wide forward, inference-scoped.** There is exactly one `inference-reverse` Mutagen forward for the whole app, not one per sandbox. It is created in `start_inference` (after the server is ready), re-created against the new host port on every inference restart (the start path always tears down and rebuilds), terminated in `stop_inference`, and terminated again defensively in `RunEvent::Exit`. This mirrors the inference server's own one-at-a-time, app-wide lifecycle.

**Port constant kept in two languages.** `INFERENCE_VM_PORT = 51199` lives in `src-tauri/src/sync/mod.rs` (Rust, drives the forward listener) and `src/lib/agents/opencode.ts` (TypeScript, drives the `baseURL` written into `opencode.json`). They must stay equal; both sites carry a comment naming the other, and the e2e plan (`03_e2e.md`) should assert the container reaches `host.containers.internal:51199`.

**Local-only in v1.** The forward is created over `runtime.local_connection()` only. A sandbox deployed to a remote host runs on a different machine than the host inference server and cannot reach it; provisioning still writes the config (harmless), but the endpoint will be unreachable there. Remote inference wiring is out of scope and would require a different topology (inference reachable from the remote, or a forward in the other direction).

**Config merge, never clobber.** `provision_opencode` merges into an existing `opencode.json` rather than overwriting: it sets only `provider[local-llm]`, ensures `$schema`, and sets `model` only when unset. A user who later edits the file keeps their changes; re-running install re-applies the same scoped block idempotently.

**Provisioning is host-side and container-state-independent.** The config write targets the bind-mounted workspace directory on the host, so it succeeds whether the container is running, stopped, or being created — no `podman exec`, no JSON shell-escaping. The file syncs into the container (and to remote, if deployed) through the existing Mutagen sync session.

**Install detection unchanged.** OpenCode is detected as installed by the existing `installedAgents` derivation (`misePackages.every(p => packages.includes(miseKey(p)))`) because `mise use opencode` writes `opencode` to the workspace `mise.toml [tools]`, which `list_packages` reads. No new "is provisioned?" persistence is added in v1; the presence of `opencode.json` with a `local-llm` provider is the provisioned signal, and re-provisioning is idempotent.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `loadAgentStatuses` throws for OpenCode | It reads `agent.auth.credentialFile` but OpenCode has no `auth` | Guard `if (!agent.auth) continue;` before any auth-status call; render a provider-status line for auth-less agents |
| Container gets connection-refused on the inference URL | The reverse forward was bound to the VM's `127.0.0.1`, or was never created because inference is stopped | Bind the listener to the VM's `0.0.0.0` via `forward_create_reverse`; the forward only exists while inference runs — start a model first |
| `baseURL` in `opencode.json` and the forward port disagree | `INFERENCE_VM_PORT` edited in Rust but not in `src/lib/agents/opencode.ts` (or vice versa) | Keep both constants equal; each comments the other; assert reachability in `03_e2e.md` |
| `provision_opencode` wipes a user's existing OpenCode config | Overwriting instead of merging | Merge: set only `provider[local-llm]` + `$schema`, set `model` only when absent; never replace the whole file |
| OpenCode 400s on the model id under the Foundry engine | Foundry serves a device-specific resolved id, not the catalogue alias the config lists | v1 wires OpenCode to the llama engine only; the alias-based `models` map is llama-proven; Foundry wiring is deferred |
| Reverse forward orphaned after closing the app or stopping a model | The forward outlives its inference server if not torn down | Terminate `inference-reverse` in both `stop_inference` and the `RunEvent::Exit` hook, before the Mutagen daemon stop |
| OpenCode install not detected | Installed outside mise, so the workspace manifest never lists it | Install via the mise `opencode` tool (`misePackages: ["opencode"]`); the existing `miseKey` derivation then detects it |
</content>
