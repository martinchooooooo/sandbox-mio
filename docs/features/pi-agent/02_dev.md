---
type: dev-plan
title: "Pi Agent Build Plan"
feature: pi-agent
---
# Pi Agent Build Plan

Build plan for adding **pi** (`pi.dev`, mise tool `pi`, GitHub `earendil-works/pi`) as a third installable coding agent (`docs/features/pi-agent/01_ideas.md`), pre-wired on install to the local-LLM host inference server. It continues the ideas document unchanged in its core decisions — a pi catalogue entry installed via mise, a provider block generated from the model catalogue, and reuse of the already-built app-wide `inference-reverse` Mutagen forward and fixed VM port `51199` — and settles the ideas document's open questions with the resolutions below. The one mechanism new to pi (absent from the OpenCode feature it mirrors) is redirecting pi's config directory into the bind-mounted workspace via `PI_CODING_AGENT_DIR`, set in the workspace `mise.toml [env]`, so provisioning stays host-side and container-state-independent. Both pi-specific risks were cleared by the 2026-07-01 spike (`01_ideas.md` Validation): mise installs `pi@0.80.2` as a self-contained binary, and pi honours `PI_CODING_AGENT_DIR` from a mise `[env]` export and loads the redirected `models.json`. **Status: nothing in this plan is implemented yet — no pi code exists in the codebase. Every phase below is `Remaining`.** The OpenCode agent feature (`docs/features/opencode-agent/`) is implemented and is the direct template.

## The Concrete Model

Pi is a third descriptor in the static agent catalogue (`src/lib/agents/catalogue.ts`), installed per-sandbox through the same `install_agent` → `agents::install_packages` path Claude Code and OpenCode use, with one mise package (`pi`) and **no** npm-postinstall step (the spike confirmed mise ships a self-contained `pi` binary, no `node@lts`). The descriptor declares **no auth** and a **provider-provisioning** step whose `kind` is the new discriminant `"pi-provider"`. After the binary install, the frontend calls a new `provision_pi` Tauri command that performs **two host-side writes** into the bind-mounted workspace: (1) a `models.json` scoped-merge declaring a single custom provider (`local-llm`, `api: "openai-completions"`) whose `baseUrl` is the fixed in-container address, and (2) an idempotent `mise.toml [env]` entry setting `PI_CODING_AGENT_DIR = "/root/work/.mio/pi-agent"` so pi reads that `models.json`.

The provider `baseUrl` is the exact stable address OpenCode uses — `http://host.containers.internal:51199/v1` — served by the existing app-wide reverse Mutagen forward (`inference-reverse`, created by the inference lifecycle in `start_inference`, torn down in `stop_inference` and on app exit). **No new forward, port, or Rust `sync` change is required**; pi reuses OpenCode's bridge verbatim. The pi `models` array is generated from the same local-llm catalogue (`src/lib/models/catalogue.ts`) so pi's model map and the inference server's served catalogue cannot drift. Per the spike, minimal `{ id, name }` entries suffice (pi supplies context/max-out defaults). v1 wires pi to the **llama engine only**; the Foundry engine's strict resolved-id requirement is deferred, matching OpenCode's v1 scope.

Pi's config schema differs from OpenCode's and the generator is therefore pi-specific: the top-level key is `providers` (a map) not `provider`; the address field is `baseUrl` (camelCase) not `baseURL`; the provider declares `api: "openai-completions"` not an npm loader; and `models` is an **array** of `{ id, name }`, not a map keyed by id. Pi selects its model with `--model provider/id` at launch, so provisioning writes **no** default-model field; the recommended pick (`local-llm/phi-4-mini-reasoning`) is surfaced as UI guidance.

## Resolved Decisions

Each Open Question from `01_ideas.md`, with its build-time resolution:

- **Confirming spike for pi in-container** — Resolved by the 2026-07-01 spike (`01_ideas.md` Validation): both risks cleared. `mise use pi` installs `pi@0.80.2` (self-contained `pi-linux-x64.tar.gz`, no node); `PI_CODING_AGENT_DIR` from a workspace `mise.toml [env]` is exported into both `mise exec` and interactive-bash panes, and `pi --offline --list-models` lists `local-llm/phi-4-mini-reasoning` from the redirected file (negative control: absent without the redirect). No further spike is a build prerequisite.
- **Provision command: new `provision_pi` vs. generalized `provision_opencode`** — Resolved: add a **new `provision_pi` command** rather than generalizing `provision_opencode`. Pi needs a different top-level JSON key (`providers` vs `provider`) *and* a second write (`mise.toml [env]`) that OpenCode does not; folding both into one command would make the OpenCode path carry unused parameters. Keeping two commands keeps each simple and leaves the working `provision_opencode` untouched. The `provision` descriptor's `kind` field routes between them in the UI.
- **Shared constants ownership** — Resolved: extract `INFERENCE_VM_PORT`, `LOCAL_LLM_BASE_URL`, and `LOCAL_LLM_PROVIDER_ID` into a new `src/lib/agents/local-llm.ts`; both `opencode.ts` and the new `pi.ts` import them. This avoids a `pi.ts → opencode.ts` import (which reads as pi depending on OpenCode) and keeps the port-constant single-sourced. OpenCode is uncommitted, so moving its constants now is low-risk.
- **`models.json` merge scope and `[env]` idempotency** — Resolved: the `models.json` write is a scoped merge setting only `providers[local-llm]` (every other provider key preserved), exactly like `provision_opencode`'s `provider[local-llm]` merge. The `mise.toml [env]` write parses the manifest with the `toml` crate (already used by `runtime::parse_installed`), sets only `env.PI_CODING_AGENT_DIR`, and re-serialises — preserving the `[tools]` table and any other `[env]` keys. Both writes are idempotent under re-install.
- **Default/launch model injection** — Resolved for v1: provisioning writes **only** `models.json`, no default-model/settings file. The spike found no default-model key in `models.json`, so writing one is unproven; pi selects with `--model local-llm/phi-4-mini-reasoning`, surfaced as UI guidance. (Observed, not relied upon: when `local-llm` is the sole configured provider, pi auto-selects it, so bare `pi` likely targets it — recorded but not guaranteed.)
- **Model `name`/`id` fields per engine** — Resolved for v1: the `models` array lists **catalogue aliases** (the spike drove `local-llm/phi-4-mini-reasoning` by alias successfully; the llama engine is alias-tolerant per the OpenCode spike). The Foundry engine's device-specific resolved id is **not** wired to pi in v1 and is recorded as deferred.

## Remaining: Phase A — Provisioner discriminant (`src/lib/agents/types.ts`)

Extend the provision descriptor to route pi.

- `ProviderProvisionConfig.kind` gains the value `"pi-provider"` alongside `"opencode-provider"` (the field is already documented as "discriminant for future provisioners"; pi is the second).
- No other type change: `AgentDescriptor.auth` is already optional, `provision?` already exists, and `relativeConfigPath` already carries the workspace-relative config path (for pi, `.mio/pi-agent/models.json`). No new `AgentInstallEvent`/`AgentAuthStatus` variants — provisioning reports through the existing install action's busy-tracking, as OpenCode's does.

## Remaining: Phase B — Shared constants, pi generator, and catalogue entry

### `src/lib/agents/local-llm.ts` (new)

Holds the constants shared by both local-LLM-provisioned agents, moved out of `opencode.ts`:

- `export const INFERENCE_VM_PORT = 51199;` — **must equal** the Rust `INFERENCE_VM_PORT` in `src-tauri/src/sync/mod.rs`; keep the mutual cross-reference comment.
- `export const LOCAL_LLM_BASE_URL = \`http://host.containers.internal:${INFERENCE_VM_PORT}/v1\`;`
- `export const LOCAL_LLM_PROVIDER_ID = "local-llm";`

`src/lib/agents/opencode.ts` is updated to import these three from `local-llm.ts` instead of declaring them; its `buildLocalLlmProvider` is otherwise unchanged. `AgentManager.svelte` imports `buildLocalLlmProvider` from `opencode.ts` unchanged (it does not import the constants directly).

### `src/lib/agents/pi.ts` (new)

Mirrors `opencode.ts` for pi's schema:

- `export const PI_CONFIG_DIR_CONTAINER = "/root/work/.mio/pi-agent";` — the container-absolute config dir written into `PI_CODING_AGENT_DIR`.
- `export const PI_CONFIG_ENV = "PI_CODING_AGENT_DIR";`
- `export const PI_CONFIG_REL_PATH = ".mio/pi-agent/models.json";` — workspace-relative `models.json` path (matches the descriptor's `relativeConfigPath`).
- `export const PI_DEFAULT_MODEL = \`${LOCAL_LLM_PROVIDER_ID}/phi-4-mini-reasoning\`;` — the recommended `--model` argument, surfaced as guidance (not written to config).
- `export function buildPiLocalLlmProvider(): { providerId: string; providerJson: unknown }` — generates pi's provider block from `MODELS` (`src/lib/models/catalogue.ts`): `{ baseUrl: LOCAL_LLM_BASE_URL, api: "openai-completions", apiKey: "sk-local", models: MODELS.map(m => ({ id: m.id, name: m.label })) }`. Returns `providerId: LOCAL_LLM_PROVIDER_ID` and the provider JSON; `provision_pi` nests it under `providers[providerId]`. The dummy `apiKey` is required for the same reason OpenCode needs one (an OpenAI-compatible client sends an `Authorization` header llama-server ignores).

### `src/lib/agents/catalogue.ts`

Add a third `AgentDescriptor`:

```ts
{
  id: "pi",
  label: "Pi",
  description: "Minimal terminal coding agent, pre-wired to your local LLM",
  misePackages: ["pi"],
  postInstallSteps: [],
  // no `auth`
  provision: { kind: "pi-provider", providerId: "local-llm", relativeConfigPath: ".mio/pi-agent/models.json" },
}
```

`miseKey("pi")` returns `"pi"` (no scheme, no `@version`), so the existing `installedAgents` derivation in `AgentManager.svelte` detects it from the workspace `mise.toml [tools]` with no change. Pi's mise package (`pi`) collides with neither Claude Code's (`node@lts` + `npm:@anthropic-ai/claude-code`) nor OpenCode's (`opencode`), and its config file lives at a distinct path, so all three agents coexist.

## Remaining: Phase C — `provision_pi` command (`src-tauri/src/lib.rs`)

A host-side, merge-preserving writer that does two writes into the bind-mounted workspace. Mirrors `provision_opencode`'s host-side JSON write, plus a `mise.toml [env]` edit.

```rust
/// Provision pi's local-llm provider host-side into the bind-mounted workspace, container-state
/// independent. (1) Merge <workspace>/<relative_path> (models.json): set only providers[provider_id],
/// preserving other providers. (2) Set mise.toml [env] PI_CODING_AGENT_DIR = config_dir_container,
/// preserving [tools] and any other [env] keys, so pi reads the redirected models.json.
#[tauri::command]
async fn provision_pi(
    workspace_path: String,
    relative_path: String,            // ".mio/pi-agent/models.json"
    provider_id: String,              // "local-llm"
    provider_json: serde_json::Value, // the provider block from buildPiLocalLlmProvider()
    config_env: String,               // "PI_CODING_AGENT_DIR"
    config_dir_container: String,     // "/root/work/.mio/pi-agent"
) -> Result<(), AppError>;
```

Behavior:

1. **`models.json` merge.** Read `<workspace_path>/<relative_path>` if present (`{}` if absent/empty; a genuine parse error of non-empty content returns an `AppError` rather than overwriting, exactly as `provision_opencode` does). Ensure the parent directory exists (`.mio/pi-agent/`) with `std::fs::create_dir_all` — unlike `opencode.json` at the workspace root, pi's file is nested. Set `obj["providers"][provider_id] = provider_json` (creating the `providers` map if absent). Write back pretty-printed. No `$schema` and no top-level `model` (pi has neither).
2. **`mise.toml [env]` write.** Read `<workspace_path>/mise.toml` (it always exists after `init`; if absent, start from `"[tools]\n"`). Parse with the `toml` crate into a `toml::Table`, set `table["env"]["PI_CODING_AGENT_DIR"] = config_dir_container` (creating the `[env]` table if absent, preserving existing `[tools]`/`[env]` entries), and re-serialise with `toml::to_string`. Write back.

Register `provision_pi` in the `generate_handler!` list alongside `provision_opencode` (`src-tauri/src/lib.rs` line 859). Reuse `AppError` via the same `BackendError::CommandFailed` mapping `provision_opencode` uses for read/parse/write failures; a new error variant is not required. Note the `toml` crate is already a `Cargo.toml` dependency (used by `runtime::parse_installed`) — confirm it exposes serialization (`toml::to_string`); the `toml` crate does. If only a read-side feature is enabled, add the serialization feature in `src-tauri/Cargo.toml`.

## Remaining: Phase D — Reverse forward and inference lifecycle (no change)

Pi requires **no** change to `src-tauri/src/sync/mod.rs` or the inference lifecycle in `src-tauri/src/lib.rs`. The app-wide `inference-reverse` forward and `INFERENCE_VM_PORT = 51199` that OpenCode introduced already make `http://host.containers.internal:51199/v1` reachable from every local container while inference runs. Pi writes the same `baseUrl`; the existing forward serves it. This phase exists only to record that the reuse is deliberate and that no second forward should be added.

## Remaining: Phase E — AgentManager UI (`src/routes/projects/AgentManager.svelte`)

Render and provision pi through the existing config-only (auth-absent) path, dispatching on `provision.kind`.

- **Provision on install.** In `installAgent`, after a successful `install_agent`, the existing block runs when `descriptor.provision` is set. Branch on `descriptor.provision.kind`:
  - `"opencode-provider"` → the current `buildLocalLlmProvider()` + `provision_opencode` call, unchanged.
  - `"pi-provider"` → `buildPiLocalLlmProvider()` (from `pi.ts`) + `invoke("provision_pi", { workspacePath, relativePath: descriptor.provision.relativeConfigPath, providerId, providerJson, configEnv: PI_CONFIG_ENV, configDirContainer: PI_CONFIG_DIR_CONTAINER })`. Append the same "✓ provider configured" line to `installLog`, inside the existing `action.run(...)` busy scope.
- **Ready/needs-inference state.** The existing `{:else if agent.provision}` row branch already shows "local-llm provider configured" when `inferenceRunning`, else the `/models` hint. Pi reuses it verbatim — the branch keys on `agent.provision`, not on the agent id.
- **i18n.** Reuse `projects_agent_provider_configured` ("local-llm provider configured") and `projects_agent_provisioning` ("Configuring local-llm provider…") unchanged — both are agent-agnostic. **Change** `projects_agent_start_model_hint` from the OpenCode-specific "Start a model to use OpenCode" to the agent-agnostic "Start a model to use this agent" in `messages/en.json`, then mirror the edit to `messages/de.json` and `messages/ru.json` (per AGENTS.md). No new keys are required.

## Lifecycle / Cross-Cutting Concerns

**No new forward, one shared bridge.** Pi and OpenCode both point at the single app-wide `inference-reverse` forward. It is created in `start_inference`, re-created against the new host port on every inference restart, terminated in `stop_inference`, and terminated defensively in the `RunEvent::Exit` hook. Pi adds no lifecycle code; it only writes a config that names the forward's stable address.

**Config redirect via `mise.toml [env]`, honoured in the terminal.** Pi reads `models.json` from `$PI_CODING_AGENT_DIR`. The base image runs `eval "$(mise activate bash)"` in `/root/.bashrc` with `WORKDIR /root/work` (`src-tauri/resources/Containerfile` lines 41, 45), so the workspace `mise.toml [env]` entry is exported into every interactive zellij pane before the user runs `pi`. The spike confirmed both the `mise exec` and interactive-bash export paths. A user who launches pi outside mise (e.g. a raw `podman exec` without `mise exec`) would not get the redirect — the documented launch path is a terminal pane, where mise is active.

**Two host-side writes, both merge-preserving and container-state-independent.** `provision_pi` writes only into the bind-mounted workspace on the host: `models.json` (scoped to `providers[local-llm]`) and `mise.toml [env].PI_CODING_AGENT_DIR`. Both succeed whether the container is running, stopped, or being created — no `podman exec`, no JSON/TOML shell-escaping. The files sync into the container (and to a remote, if deployed) through the existing Mutagen sync session, and are visible/editable in the user's workspace tree alongside `.mio/zellij.json`.

**Install detection unchanged.** Pi is detected as installed by the existing `installedAgents` derivation (`misePackages.every(p => packages.includes(miseKey(p)))`) because `mise use pi` writes `pi` to the workspace `mise.toml [tools]`, which `list_packages` reads. No "is provisioned?" persistence is added; the presence of `models.json` with a `local-llm` provider plus the `[env]` entry is the provisioned signal, and re-provisioning is idempotent.

**Local-only in v1.** Like OpenCode, the forward is created over `runtime.local_connection()` only. A sandbox deployed to a remote host cannot reach the host inference server; provisioning still writes the (harmless) config, but the endpoint is unreachable there. Remote inference wiring is out of scope.

**Foundry deferred.** v1 wires pi to the llama engine only; the `models` array lists catalogue aliases (llama-tolerant). Foundry's device-specific resolved id would require rewriting the `models` array at inference-start time and is deferred, matching OpenCode.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Pi ignores the provisioned provider | Wrote OpenCode's schema — `provider` map, `baseURL`, npm loader, `models` as a map | Use pi's schema: top-level `providers`, `baseUrl` (camelCase), `api: "openai-completions"`, `models` as an **array** of `{ id, name }` |
| Pi cannot find `models.json` | `PI_CODING_AGENT_DIR` not written to `mise.toml [env]`, or pi launched outside `mise` | `provision_pi` must write the `[env]` entry; launch pi from a terminal pane where `mise activate` exports it |
| `provision_pi` wipes the `[tools]` table or other providers | Rewrote `mise.toml`/`models.json` from scratch instead of merging | Parse both with their typed libraries (`toml`, `serde_json`), set only `env.PI_CODING_AGENT_DIR` and `providers[local-llm]`, re-serialise |
| `provision_pi` fails to write `models.json` | Parent dir `.mio/pi-agent/` does not exist (nested, unlike root `opencode.json`) | `std::fs::create_dir_all` the parent before writing |
| Cargo build fails on `toml::to_string` | The `toml` dependency has serialization disabled | Enable the serialization feature for `toml` in `src-tauri/Cargo.toml` |
| Pi shows "not authenticated" | Treated pi like Claude Code and read a credential file | Pi has no `auth`; the auth-absent rendering branch (added for OpenCode) already treats it as ready once provisioned |
| Pi errors `Connection error` in the terminal | The `inference-reverse` forward exists only while inference runs | Start a model at `/models` first; the forward is created by the inference lifecycle, not pi install |
| `baseUrl` and forward port disagree | `INFERENCE_VM_PORT` edited in Rust but not in `src/lib/agents/local-llm.ts` (or vice versa) | Keep both `51199` constants equal; each comments the other |
| Pi install not detected | Installed via curl/PowerShell script or raw binary, bypassing mise | Install through the mise `pi` tool (`misePackages: ["pi"]`) so the `miseKey` derivation detects it |
