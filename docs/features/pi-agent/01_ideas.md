---
type: ideas
title: "Pi Agent Ideas"
feature: pi-agent
---
# Pi Agent Ideas

This document covers the ideation for adding **pi** (the coding agent from <https://pi.dev/>, npm package `@earendil-works/pi-coding-agent`, GitHub `earendil-works/pi`) as a **third** installable coding agent in sandbox-mio, alongside Claude Code and OpenCode, and — mirroring the OpenCode feature — for **pre-configuring pi on install** so a local-LLM provider is already wired to the host inference server (`docs/features/local-llm/`). Pi is structurally the same shape of problem as OpenCode: a config-only agent (no login for a local provider) installed through mise, provisioned once at install with a provider block generated from the model catalogue. The one material difference is *where* the config file lives — pi reads `models.json` from `$HOME/.pi/agent/` by default, which is **not** bind-mounted, whereas OpenCode reads `opencode.json` from the project root, which is. This feature resolves that difference by redirecting pi's config directory into the bind-mounted workspace via the `PI_CODING_AGENT_DIR` environment variable, set in the workspace `mise.toml [env]` table, so the config write stays host-side and container-state-independent exactly like OpenCode's. **Status: nothing for pi exists in the codebase today.** The OpenCode agent feature (`docs/features/opencode-agent/`) is implemented and is the direct template; the local-llm host inference server and the app-wide reverse Mutagen forward it introduced are dependencies, already built.

## Current State

The codebase has two agents today, both descriptor-driven and per-sandbox:

- `src/lib/agents/catalogue.ts` — a static `AGENTS: AgentDescriptor[]` array with `claude-code` (auth-based) and `opencode` (config-based) entries. Each descriptor carries `misePackages`, `postInstallSteps`, an optional `auth`, and an optional `provision`.
- `src/lib/agents/types.ts` — `AgentDescriptor.auth` is optional (an agent that configures instead of authenticates omits it); `ProviderProvisionConfig` carries `kind`, `providerId`, and `relativeConfigPath`. `kind` is currently the single value `"opencode-provider"`, described in its own comment as "discriminant for future provisioners" — pi is the second provisioner.
- `src/lib/agents/opencode.ts` — OpenCode-specific constants (`INFERENCE_VM_PORT`, `LOCAL_LLM_BASE_URL`, `LOCAL_LLM_PROVIDER_ID`) and `buildLocalLlmProvider()`, which generates the OpenCode provider block from `src/lib/models/catalogue.ts` (`MODELS`).
- `src-tauri/src/lib.rs` — `provision_opencode` (line 474) writes/merges `opencode.json` host-side into the bind-mounted workspace. `install_agent` (line 453), the `start_agent_auth` family, and the inference lifecycle (`start_inference`/`stop_inference`) that manages the app-wide `inference-reverse` forward live here.
- `src-tauri/src/sync/mod.rs` — `forward_create_reverse` and `INFERENCE_VM_PORT = 51199`: the app-wide reverse Mutagen forward that makes the host inference server reachable inside every local container at `http://host.containers.internal:51199/v1`.
- `src/routes/projects/AgentManager.svelte` — renders installed/available agents, installs them with a progress log, provisions config-only agents after install, and shows a "provider configured" / "start a model" state instead of an auth prompt.
- `src-tauri/src/runtime/mod.rs` — `init` writes the workspace `mise.toml` host-side (line 124); `parse_installed` (line 64) already parses `mise.toml` with the `toml` crate, so the crate is a proven dependency for reading and (by extension) writing the manifest.
- Base image (`src-tauri/resources/Containerfile`) — bakes `eval "$(mise activate bash)"` into `/root/.bashrc` (line 41) and sets `WORKDIR /root/work` (line 45). Consequently every interactive zellij terminal pane activates mise in the workspace, so a `mise.toml [env]` entry is exported into the shell environment before the user runs `pi`.

The local-llm seams this feature consumes are the same ones OpenCode consumes and are already built: the host inference server (`src-tauri/src/inference/`), the model catalogue (`src/lib/models/catalogue.ts`), and the `inference-reverse` app-wide forward bound to the inference lifecycle.

## Proposed Model

### Pi catalogue entry (`src/lib/agents/catalogue.ts`)

A third `AgentDescriptor` with `id: "pi"`. Pi is distributable through mise's own registry (`mise use pi`, backed by the aqua/github registry entry for `earendil-works/pi`), which fetches a self-contained platform binary — so, exactly like OpenCode and unlike Claude Code, it needs neither a `node@lts` dependency nor the npm-postinstall workaround. `misePackages` is the single entry `["pi"]`; `postInstallSteps` is empty; there is **no** `auth`. The descriptor declares a `provision` step whose new `kind` (`"pi-provider"`) selects the pi provisioner. `miseKey("pi")` returns `"pi"`, so the existing `installedAgents` derivation in `AgentManager.svelte` detects it from the workspace `mise.toml [tools]` with no change.

### Provider provisioning: `models.json` generated from the catalogue (`src/lib/agents/pi.ts` — new)

Mirrors `src/lib/agents/opencode.ts`. A `buildPiLocalLlmProvider()` helper generates pi's `models.json` provider block from the same `MODELS` catalogue, so pi's model map and the inference server's served catalogue cannot drift. Pi's provider schema differs from OpenCode's in field names and shape, so the generator is pi-specific rather than shared:

```jsonc
{
  "providers": {
    "local-llm": {
      "baseUrl": "http://host.containers.internal:51199/v1",
      "api": "openai-completions",
      "apiKey": "sk-local",
      "models": [
        { "id": "phi-4-mini-reasoning", "name": "Phi-4 Mini Reasoning" }
        // …one entry per catalogue model
      ]
    }
  }
}
```

Key shape differences from OpenCode, all confirmed by research (see Validation): the top-level key is `providers` (a map) not `provider`; the address field is `baseUrl` (camelCase) not `baseURL`; the provider declares `api: "openai-completions"` instead of an npm loader (`npm: "@ai-sdk/openai-compatible"`); and `models` is an **array** of `{ id, name }` objects, not a map keyed by id. The dummy `apiKey` (`sk-local`) is required for the same reason OpenCode needs one — an OpenAI-compatible client sends an `Authorization` header that llama-server ignores. The reused constant `INFERENCE_VM_PORT`/`LOCAL_LLM_BASE_URL`/`LOCAL_LLM_PROVIDER_ID` values are shared with OpenCode's; whether they are imported from `opencode.ts` or re-declared is an Open Question.

Pi selects its active model with the `--model provider/id` CLI flag rather than a config-file default (research; the docs show no default-model key in `models.json`). So, unlike OpenCode, provisioning writes **no** top-level default-model field; the recommended pick (`local-llm/phi-4-mini-reasoning`, matching `recommendModels`) is surfaced to the user as guidance, not written into the config. Whether to also inject a launch alias/default is an Open Question.

### Config directory redirect into the workspace (`PI_CODING_AGENT_DIR` via `mise.toml [env]`)

This is the feature's one genuinely new mechanism relative to OpenCode. Pi reads `models.json` from `$HOME/.pi/agent/` (i.e. `/root/.pi/agent/models.json` in the container) by default, which is **not** the bind-mounted workspace, so OpenCode's host-side "write into the project directory" approach cannot target it directly. Pi honours the `PI_CODING_AGENT_DIR` environment variable to override that directory (research; default `~/.pi/agent`). The proposal sets `PI_CODING_AGENT_DIR = "/root/work/.mio/pi-agent"` in the workspace `mise.toml [env]` table and writes `models.json` host-side into `<workspace>/.mio/pi-agent/models.json`. Because the base image activates mise in every interactive bash pane with `WORKDIR /root/work` (Containerfile lines 41, 45), the `[env]` entry is exported into the shell before `pi` runs, so pi resolves its config from the redirected, bind-mounted, user-visible location. This keeps the whole provisioning host-side and container-state-independent — the same properties that make OpenCode's provisioning robust — and puts pi's config alongside the existing `<workspace>/.mio/zellij.json`.

### Provisioning command (`provision_pi` or a generalized `provision_opencode`, `src-tauri/src/lib.rs`)

The provisioning does two host-side writes into the bind-mounted workspace: (1) the `models.json` JSON write (a scoped merge like `provision_opencode`: set only `providers[local-llm]`, preserve any user additions), and (2) an idempotent `mise.toml [env]` entry setting `PI_CODING_AGENT_DIR`, using the `toml` crate already imported for `parse_installed`. Both are plain `std::fs` operations against the host, needing no `podman exec` and working whether the container is running or stopped. Whether this is a new `provision_pi` command or a generalization of `provision_opencode` (parameterized by config filename, top-level key, and an optional `[env]` injection) is an Open Question; the `provision` descriptor's `kind` field already exists to route between provisioners.

### UI flow (`src/routes/projects/AgentManager.svelte`)

Pi appears in the existing "Add agent" picker. Installing it runs the binary install (progress log, as today) and then the provisioning step, after which the row shows the same "local-llm provider configured" ready state OpenCode shows — including the "Start a model to use it" hint linking to `/models` when `inference_status.running` is false. The existing config-only rendering branch (auth-absent agents) already covers pi; the only UI change is dispatching to the pi provider builder and provisioning command when `descriptor.provision.kind === "pi-provider"`.

## Technology Choices with Tradeoffs

### Install mechanism: mise `pi` registry tool (not `npm:@earendil-works/pi-coding-agent`, not the curl/PowerShell installer)

Chosen: install pi through mise's native `pi` tool, reusing `install_packages` with no npm-postinstall step, exactly as OpenCode does.

Alternatives evaluated:

- **`npm:@earendil-works/pi-coding-agent` (mirror the Claude Code descriptor)** — rejected as the primary path. The official npm command is `npm install -g --ignore-scripts @earendil-works/pi-coding-agent`; the explicit `--ignore-scripts` signals pi needs no postinstall, but the npm route still forces a `node@lts` dependency into the descriptor, which the aqua/github mise backend avoids by shipping a self-contained binary. Retained as a fallback if the mise `pi` registry entry proves unavailable on a target platform.
- **The `curl -fsSL https://pi.dev/install.sh | sh` / PowerShell installer** — rejected for the same reason the OpenCode curl script was: it installs outside mise's tool management, so `AgentManager.svelte`'s "installed?" derivation (which checks the workspace mise manifest) would not see it, and version pinning would fall outside the app's model.

### Config placement: redirect the config dir into the workspace (not write to `~/.pi/agent`, not project-cwd discovery)

Chosen: set `PI_CODING_AGENT_DIR` to a workspace subdirectory via `mise.toml [env]` and write `models.json` host-side there.

Alternatives evaluated:

- **Write `~/.pi/agent/models.json` inside the container via `podman exec`** — rejected as the primary mechanism. It reintroduces exactly what OpenCode's host-side design eliminated: a dependency on the container running at provision time, and JSON shell-escaping through `podman exec`. It also hides the config from the user's synced tree (the container home is not bind-mounted). Kept as the fallback if the `mise.toml [env]` redirect proves not to be honoured by pi in the terminal.
- **Rely on a project-level / current-working-directory `models.json` discovery** — rejected because research confirms only `~/.pi/agent/models.json` (overridable via `PI_CODING_AGENT_DIR`) for the models config; unlike `AGENTS.md`/`SYSTEM.md`, `models.json` is not documented as discovered from the cwd or parent directories, so cwd discovery is an unproven dependency.

### Provider wiring: explicit named `local-llm` provider in `models.json` (not the built-in `openai` provider via env vars)

Chosen: write an explicit custom provider (id `local-llm`, `api: "openai-completions"`) into `models.json`, mirroring OpenCode's explicit-provider decision. Rejected alternative — overloading a built-in `openai` provider via `OPENAI_BASE_URL` — for the same reasons OpenCode rejected it: it collides with a future real OpenAI setup and depends on env-var interpretation of the base URL. A named provider is self-describing in pi's UI and cannot collide.

### Endpoint addressing: reuse the app-wide `inference-reverse` forward and the fixed VM port (not a pi-specific forward)

Chosen: write the same stable literal `baseUrl` (`http://host.containers.internal:51199/v1`) OpenCode uses, served by the already-built app-wide reverse Mutagen forward bound to the inference lifecycle. No new forward, no new port. Rejected alternative — a pi-specific forward or port — rejected because the inference server is a single app-wide process already fronted by one fixed-port forward that every local container reaches; a second forward would duplicate the exact same bridge.

### Default model: surface guidance, do not write a config default (pi selects via `--model`)

Chosen: leave model selection to pi's `--model provider/id` flow and surface the recommended pick as UI guidance. Alternative — **write a top-level default-model field into `models.json`** — rejected/deferred because research shows no documented default-model key in `models.json` (unlike OpenCode's top-level `model`), so writing one is an unproven dependency; whether pi exposes another settings file for a launch default is left open.

## Validation

The design rests on (a) the OpenCode feature's already-cleared 2026-06-29 spike, which proved the reverse-Mutagen-forward topology, the mise-install path, and that an OpenAI-compatible agent drives the local llama engine with a dummy key and a config-alias model id — all of which pi reuses unchanged; (b) documentation research on pi conducted 2026-07-01; and (c) a pi-specific spike run 2026-07-01 that cleared the two research-only risks (recorded below).

### Spike: pi install + config-dir redirect (2026-07-01, cleared)

Run against a throwaway container from the baked base image (`sandbox-mio/base:2`) on `podman-machine-default`, with the workspace bind-mounted at `/root/work`. Each risk and the command that cleared it:

- **R1 — mise's `pi` backend ships a working self-contained binary (no node).** `mise use pi` (in a workspace whose `mise.toml` had only `[tools]`) downloaded and installed `pi@0.80.2` from `pi-linux-x64.tar.gz` (with attestation/SLSA verification), and `mise exec -- pi --version` printed `0.80.2`. No `node@lts` was present, confirming the mise-install choice over `npm:@earendil-works/pi-coding-agent`.
- **R2 — pi honours `PI_CODING_AGENT_DIR` exported from a workspace `mise.toml [env]`, and loads `models.json` from the redirected, bind-mounted location.** With `[env] PI_CODING_AGENT_DIR = "/root/work/.mio/pi-agent"` in the workspace `mise.toml` and a `models.json` written host-side into `<workspace>/.mio/pi-agent/`, both `mise exec -- sh -c 'echo $PI_CODING_AGENT_DIR'` and an interactive `bash -ic` pane (the `mise activate` path the terminal uses) printed `/root/work/.mio/pi-agent`. `pi --offline --list-models` then printed `local-llm  phi-4-mini-reasoning  128K  16.4K …`, and the negative control (`PI_CODING_AGENT_DIR` pointed at an empty dir) listed no `local-llm` model — proving pi loaded the provider from the redirected file, not a default location.

Facts established for the build (`02_dev.md`):

- Pi version at spike time is `0.80.2`; the mise tool id is `pi`.
- Minimal `models` entries (`{ "id", "name" }` only) are sufficient — pi supplied defaults for `context` (128K) and `max-out` (16.4K); the richer `reasoning`/`input`/`cost`/`contextWindow`/`maxTokens` fields are optional.
- Pi exposes `--offline` / `--list-models` for config introspection without a network call, and selects a model via `--model provider/id` with `-p`/`--print` for non-interactive runs. There is **no** `models` subcommand (`pi models` is treated as an agent prompt); pi's actual subcommands are `install`/`remove`/`update`/`list`/`config` (extension management).
- With no provider configured, pi errors `No API key found for the selected model` (useful signal for the UI "not configured" vs "configured" distinction); with the `local-llm` provider configured but the endpoint down, it errors `Connection error` instead — i.e. a configured-but-dead endpoint is distinguishable from an unconfigured one.

Spike artifacts (throwaway containers and bind-mount workspaces) were removed afterward. The LLM round-trip against the live `inference-reverse` forward was **not** re-run: it reuses the OpenCode spike's proven path (same forward, same fixed VM port, same llama engine, same OpenAI-compatible protocol); only the two pi-specific unknowns above were open, and both cleared.

### Cleared by research (2026-07-01)

- **Pi is installable via mise.** The mise registry lists `pi` with npm, aqua, and github backends for `earendil-works/pi` (`mise use pi`). Sources: <https://mise-tools.jdx.dev/tools/pi>, <https://www.npmjs.com/package/@earendil-works/pi-coding-agent>. This is the evidence for the mise-install choice over the npm/curl alternatives.
- **Pi configures custom OpenAI-compatible providers in `models.json`, with no login for a local provider.** The models config is a JSON file with a top-level `providers` map; each provider declares `baseUrl`, `api: "openai-completions"`, `apiKey`, and a `models` array of `{ id, name, … }`. A local provider needs no OAuth. Source: pi docs "Custom models" / `models.json` (`https://pi.dev/docs/latest`). This is the evidence behind the config-only (no-auth) treatment and the provider block shape.
- **Pi's config directory is `~/.pi/agent`, overridable via `PI_CODING_AGENT_DIR`.** Documented alongside `PI_CODING_AGENT_SESSION_DIR`, `PI_PACKAGE_DIR`, etc. (pi usage docs, `earendil-works/pi/packages/coding-agent/docs/usage.md`). This is the evidence for the config-dir redirect mechanism.
- **Pi selects the active model with `--model provider/id`.** The CLI flag accepts `provider/id` (and optional thinking levels). No default-model key is documented in `models.json`. Source: pi usage docs. This is the evidence for surfacing the model as guidance rather than writing a config default.
- **Base image activates mise in interactive panes at the workspace.** `/root/.bashrc` runs `eval "$(mise activate bash)"` and `WORKDIR` is `/root/work` (`src-tauri/resources/Containerfile` lines 41, 45), so a workspace `mise.toml [env]` entry is exported before `pi` runs. This is the in-repo evidence that the `PI_CODING_AGENT_DIR` redirect via `[env]` is honoured in the terminal.

## Open Questions

- **Confirming spike for pi in-container** *(resolved by the 2026-07-01 spike; recorded here as the decision's origin)*: the two research-only risks — mise `pi` ships a working self-contained binary, and pi honours `PI_CODING_AGENT_DIR` from a mise `[env]` export and loads the redirected `models.json` — both cleared (see Validation). The OpenCode spike cleared everything else. No further spike is a build prerequisite.
- **Provision command: new `provision_pi` vs. generalized `provision_opencode`**: pi needs a JSON write with a different top-level key (`providers` vs `provider`) plus a `mise.toml [env]` injection OpenCode does not. Resolve whether to add a separate `provision_pi` command or generalize the existing one (parameters: config filename, top-level provider key, optional `[env]` var+value). The `provision` descriptor already carries a `kind` discriminant to route either way.
- **Shared constants ownership**: `INFERENCE_VM_PORT`, `LOCAL_LLM_BASE_URL`, and `LOCAL_LLM_PROVIDER_ID` are identical for pi and OpenCode. Resolve whether `pi.ts` imports them from `opencode.ts`, or whether they move to a shared `src/lib/agents/local-llm.ts` module both import (avoiding a pi→opencode dependency direction that reads oddly).
- **`models.json` merge scope and the `[env]` idempotency**: `provision_opencode` merges by setting only `provider[local-llm]`. Confirm the pi write applies the same scoped merge (set only `providers[local-llm]`, preserve other providers) and that the `mise.toml [env]` write is idempotent and preserves any existing `[env]` entries and `[tools]` table when re-run.
- **Default/launch model injection**: if pi turns out to support a settings file (beyond `models.json`) for a default model, should provisioning also write `local-llm/phi-4-mini-reasoning` as the default so the user can run bare `pi`? Left open pending confirmation that such a key exists.
- **Model `name`/`id` fields per engine**: like OpenCode, the llama engine tolerates a config-alias `id` that differs from its reported model name (OpenCode spike). Confirm pi's `models[].id` alias is likewise accepted by the llama engine, and record the Foundry-engine resolved-id caveat as deferred (v1 wires pi to the llama engine only, matching OpenCode's v1 scope).

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Pi cannot find `models.json` despite provisioning | The file was written into the bind-mounted workspace but `PI_CODING_AGENT_DIR` was not exported, so pi looked in `~/.pi/agent` | Set `PI_CODING_AGENT_DIR = "/root/work/.mio/pi-agent"` in the workspace `mise.toml [env]`; the base image's `mise activate` in `.bashrc` exports it before `pi` runs |
| Provider block silently ignored by pi | Copied OpenCode's field names (`baseURL`, `provider`, npm loader, `models` map) into pi's schema | Use pi's schema: top-level `providers` map, `baseUrl` (camelCase), `api: "openai-completions"`, `models` as an **array** of `{ id, name }` |
| `provision_pi` clobbers a user's other pi providers | Overwriting `providers` instead of merging | Scoped merge: set only `providers[local-llm]`; preserve every other provider key, mirroring `provision_opencode` |
| `[env]` write wipes the `[tools]` table or existing env vars | Rewriting `mise.toml` from scratch instead of editing it | Parse with the `toml` crate (already used by `parse_installed`), set only `env.PI_CODING_AGENT_DIR`, and serialize back, preserving `[tools]` and any other `[env]` keys |
| Pi shows "not authenticated" forever | Treating pi like Claude Code and reading a credential file | Pi has no `auth`; the config-only rendering branch (auth-absent agents, added for OpenCode) already treats it as ready once provisioned |
| Pi provider points at a dead endpoint | The `inference-reverse` forward only exists while the inference server runs | Start a model at `/models` first; the app-wide forward is created by the inference lifecycle, not by pi install — provisioning is valid regardless |
| Pi install not detected as installed | Installed via the curl/PowerShell script or raw binary, bypassing mise | Install through the mise `pi` tool (`misePackages: ["pi"]`) so the existing `miseKey`-based derivation detects it |
| Pi rejects the model id | The served model id is not listed in the provider `models` array, or the Foundry resolved id differs from the catalogue alias | Generate the `models` array from `src/lib/models/catalogue.ts`; v1 wires pi to the llama engine only (alias-tolerant), deferring Foundry as OpenCode did |
