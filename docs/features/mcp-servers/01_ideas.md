---
type: ideas
title: "MCP Servers for Sandbox Agents"
feature: mcp-servers
---
# MCP Servers for Sandbox Agents

This document scopes an app-wide library of MCP servers that a non-technical user fills by pasting whatever they were told to use (a URL or a JSON snippet from a README), and then switches on per sandbox. The app writes each switched-on server into the native MCP configuration of every agent installed in that sandbox: Claude Code, OpenCode and pi. Because Paseo, the terminal and the native chat all launch those same agents, one switch reaches all three surfaces. Nothing MCP-related exists in the app today. The relevant code is the agent catalogue (`src/lib/agents/catalogue.ts`), agent provisioning (`src-tauri/src/agents/config.rs`, `src/lib/agents/provision.ts`), the credential store (`src-tauri/src/credentials/mod.rs`), the GitHub reconcile that this feature's apply step copies (`src-tauri/src/services/github_access.rs`), the egress policy (`src-tauri/src/net_policy.rs`) and the Manage dashboard (`src/lib/workspace/sections.ts`, `src/lib/workspace/DashboardOverview.svelte`). The follow-on feature `chat-mcp-learning` ([01_ideas.md](../chat-mcp-learning/01_ideas.md)) builds the native-chat learning pieces on top of this library.

## Current State

- **No MCP support exists.** The only mention of MCP in the code base is iron-proxy's MCP tool policy, deferred in `docs/features/podman-egress-boundary/04_improvements.md`.
- **Three agents, three config homes.** The catalogue (`AGENTS` in `src/lib/agents/catalogue.ts`) installs Claude Code, OpenCode and pi through mise.
  - Claude Code keeps its state in the container home: `/root/.claude/` and `/root/.claude.json`. `inject()` and `seed_onboarding()` in `src-tauri/src/credentials/mod.rs` already patch `/root/.claude.json`.
  - OpenCode is provisioned host-side into the workspace's `opencode.json` by `provision_opencode()` (`src-tauri/src/agents/config.rs`).
  - pi's whole agent directory is in the workspace. `PI_CODING_AGENT_DIR` points at `/root/work/.mio/pi-agent` (`PI_CONFIG_DIR_CONTAINER` in `src/lib/agents/pi.ts`, written into `mise.toml` by `provision_pi()` in `src-tauri/src/agents/config.rs`).
- **pi 1.5.0 has built-in MCP.** MCP has been a built-in pi extension since 0.99.0. pi reads `mcp.json` (the common `mcpServers` shape) from its agent directory and `.pi/mcp.json` from the project. It supports stdio and streamable HTTP, but not legacy SSE. It interpolates `${VAR}` in `env` and `headers` and accepts a whole-value `!command`. Tools are named `mcp__<server>__<tool>`, and the default exposure is `codemode`. The native chat's `pi --mode rpc` launch in `src-tauri/resources/agent-host/pi-host.sh` passes no `--no-mcp`, so MCP is already active in the chat.
- **Paseo has no user-facing MCP setting.** Paseo 0.16's `daemon.mcp` keys in `~/.paseo/config.json` control only Paseo's own tool server. Third-party servers reach agents only through a plugin's `agent.open.before` hook, and the hook's `mcpServers` field is launch-only. Paseo launches the installed agents in the workspace, so each agent reads its own native MCP config as usual.
- **The GitHub reconcile is the template.** `reconcile_sandbox()` in `src-tauri/src/services/github_access.rs` applies an app-wide credential to running local sandboxes on create, start, save, delete and a Manage switch. It is serialised by one lock, records a per-sandbox outcome in app data (`sandbox_github_state_path()` in `src-tauri/src/paths.rs`) and skips deployed sandboxes. The per-sandbox opt-out lives in app data rather than in `.mio/`, because code inside the sandbox can write `.mio/` (`docs/features/github-cli-auth/02_dev.md`, Resolved Decision 10).
- **Secrets have a store and a delivery path.** `save_secret()` and `load_secret()` (`src-tauri/src/credentials/mod.rs`) hold single-value credentials under the app-data root. `MachineProvider::exec_piped` (`src-tauri/src/provider/types.rs`) writes bytes into a sandbox over stdin without putting them in argv.
- **Egress policy can block a remote server.** A proxied sandbox in allowlist mode reaches only the hosts in its accepted policy (`net_policy::read_accepted()` in `src-tauri/src/net_policy.rs`); `.mio/net-policy.json` is only a proposal since [net-policy-review](../net-policy-review/02_dev.md). A Podman policy change applies live; a microsandbox policy change recreates the VM.
- **No language runtime is baked in.** The base image (`src-tauri/resources/Containerfile`) has neither Node nor Python. Claude Code already pulls `node@lts` through mise.

## The MCP Library

- **Entry:** a name (letters, digits, `_` and `-`, the rule pi enforces), a label, and a transport. The transport is either remote (`url`) or local (`command`, `args`, `cwd`). The entry also holds non-secret `env` and `headers`, a list of secret fields (names only), the runtime the local command needs, and the egress hosts the server needs.
- **Storage:** non-secret entries in `<app_data_root>/mcp/servers.json`, behind a new path helper in `src-tauri/src/paths.rs`. Secret values go in the credential store through `save_secret()`, so they never cross IPC and never reach a log.
- **Page:** a new app page in the sidebar (working route `src/routes/mcp/`) lists library entries and adds, edits and removes them. Page placement is Open Question "Library page placement".
- **Rust module:** a new `src-tauri/src/mcp/` owns the library, the import parser and rendering, each behind pure functions that `cargo test` covers.

## Adding a Server: Paste What You Were Given

- The add dialog takes one text box. A URL becomes a remote entry. A JSON snippet in the `mcpServers` shape becomes one entry per server in the snippet. Claude Desktop, Cursor and pi document servers in this shape, so it is what most READMEs show.
- The import parser (`src-tauri/src/mcp/import.rs`) marks a field as secret when its value is a placeholder (`<…>`, `YOUR_…`, `…-here`) or its name matches key, token, secret, password or `Authorization`. The dialog then asks for those values one by one, with plain-language labels.
- The parser derives the runtime from the command: `npx` needs `node@lts`, and `uvx` or `uv` needs `uv`. It derives the egress hosts from the URL, and adds the package-registry hosts for `npx` and `uvx`.
- An SSE-only server (`type: "sse"`) is refused with an explanation, because pi rejects SSE.

## Per-Sandbox Switch and the Reconcile

- **Switch state:** each sandbox's set of switched-on library entries, plus the last apply outcome, lives in `<app_data_root>/mcp/<sandbox>.json`, following GitHub's Resolved Decision 10. A new sandbox starts with no servers switched on.
- **Reconcile:** a new `src-tauri/src/services/mcp_access.rs` mirrors `github_access.rs`. It uses one lock and a pure `plan()` function, applies only to running local sandboxes, skips deployed sandboxes, and emits a `mcp-access://changed` event. The triggers are `create()` (`src-tauri/src/services/sandbox.rs`), `start_sandbox` (`src-tauri/src/commands/sandbox.rs`), `install_agent` (`src-tauri/src/commands/agents.rs`), any library or secret change (apply-all), and the Manage switch.
- **Manage UI:** a new section in `DASHBOARD_SECTIONS` (`src/lib/workspace/sections.ts`) lists library entries with one switch each and shows each entry's outcome. The states are applied, applies when the sandbox runs, needs a runtime, blocked by network policy, and failed.

## Rendering Into Each Agent's Config

| Agent | File the reconcile writes | Location | Secret reference |
|---|---|---|---|
| Claude Code | `/root/.claude.json`, top-level `mcpServers` (user scope) | Container home | `headersHelper` for headers, `${VAR}` for stdio `env` through a launcher (spike) |
| OpenCode | `/root/.config/opencode/opencode.json`, `mcp` key (`local`/`remote` shapes) | Container home | `{file:…}` substitution |
| pi | `.mio/pi-agent/mcp.json`, `mcpServers` | Workspace (pi's agent directory) | Whole-value `!command` that reads the secret file |

- **Secret files:** the reconcile writes each secret value over stdin (`exec_piped`) to `/root/.config/mio/mcp/<server>/<FIELD>` with mode 600, outside the workspace. Each agent config references the file. The config never contains the value.
- **Only installed agents get a config.** `install_agent` triggers a reconcile, so an agent installed later picks up the switched-on servers.
- **Ownership:** the reconcile merges into each config file and removes only the entries it wrote, so a user's own entries survive. How it recognises its own entries is Open Question "Managed-entry ownership".
- **Runtimes:** before writing a local server's entry, the reconcile ensures its runtime through `mise use` in the workspace, so the runtime lands in `mise.toml` and is reproduced on remote deploy.
- **Egress:** when a proxied sandbox's policy is in allowlist mode and lacks a server's hosts, the switch shows "blocked by network policy" and offers to add the hosts through an accepted-policy apply (`apply_sandbox_policy` with the current revision), the way the network editor applies a change; writing the workspace file would only create a proposal.

## Phases

1. **Library, import, switch and render:** no-auth and API-key servers, both remote and local. This covers the user's "keys first" scope.
2. **OAuth sign-in:** for remote servers that sign in through a browser (Notion, Linear, Sentry, Atlassian). Run a spike first; see Open Question "OAuth callback route".

## Technology Choices With Tradeoffs

### How servers reach the agents: each agent's native config

- **Chosen:** write each agent's own MCP config. Every launch path (Paseo, the terminal, the native chat) reads that config, so one write covers all three.
- **Rejected: a Paseo plugin injecting `mcpServers` on `agent.open.before`.** The plugin covers Paseo only, so the terminal and the native chat miss the servers. It adds a plugin to install and keep compatible with each Paseo upgrade. Paseo also fails the session open for a provider without MCP support.
- **Rejected: a single MCP gateway on the host, forwarded into the sandbox.** Local servers would run on the host, outside the sandbox boundary that is the product's point. Each agent would still need a config entry pointing at the gateway.
- **Rejected for now: an in-sandbox aggregator process (one entry per agent, fanning out to many servers).** It adds a long-running process and a third-party dependency to supervise, and buys only one config entry per agent. Recorded in [04_improvements.md](04_improvements.md).

### Where the library lives: app-wide, switched on per sandbox

- **Chosen (user decision, 2026-11-18):** an app-wide library in app data, switched on per sandbox. The user adds a server once, the way the GitHub login is added once.
- **Rejected: a per-sandbox list in `.mio/`.** The list would travel with the project, but the user would re-add the same server in every sandbox. Code inside the sandbox could also rewrite which servers are on.
- **Consequence:** a sandbox's switch state lives in app data, so switch state does not follow a workspace to another machine.

### Where each agent config lives: the container home, except pi

- **Chosen:** user-scope config in the container home for Claude Code and OpenCode. For pi, the agent directory the app already owns in the workspace.
- **Rejected: project-scope files in the workspace (`.mcp.json`, `opencode.json`, `.pi/mcp.json`).** These files would be written into the user's repository and could be committed by accident. Claude Code asks for approval of project servers. pi reads `.pi/mcp.json` only after project trust is granted, and RPC mode cannot show the trust prompt. Project-scope files would work while the sandbox is stopped, but the reconcile pattern already handles stopped sandboxes by applying on start.
- **Cost:** the container home is lost when a sandbox is recreated. Every start runs the reconcile, which restores the home configs.

### How secrets reach a server: secret files plus each agent's native reference

- **Chosen:** secret files in the container home, referenced from each config. No config file holds a value, and pi's workspace-resident config stays secret-free, as the no-secrets-in-the-workspace invariant in `docs/architecture/overview.md` requires.
- **Rejected: inline values in the home configs.** Inline values are simple for Claude Code and OpenCode, but impossible for pi, whose config is in the workspace. They would also put values into files that agents read into context.
- **Rejected for now: proxy placeholder swapping (`docs/architecture/network.md`, Secret swapping).** With swapping, the agent never sees the value. But swapping exists only for proxied sandboxes and only covers HTTP to bound hosts, and the proxy is optional. Recorded in [04_improvements.md](04_improvements.md) as a hardening step for proxied sandboxes.
- **Rejected: environment variables on each agent process.** Paseo, the terminal and the chat supervisor each start agents differently, so no single launch point exists to set them.
- **Known exposure:** a secret file in the container home is readable by any process in the sandbox, including the agent. The GitHub token in `/root/.config/gh/hosts.yml` has the same exposure.

### Import format: the `mcpServers` JSON snippet

- **Chosen:** a URL or an `mcpServers` JSON snippet, the de-facto shape shared by Claude Desktop, Cursor and pi.
- **Rejected for v1: other client formats** (`claude mcp add …` command lines, VS Code's `servers` key, Codex TOML). Each needs its own parser, and the users this feature targets are mostly handed the JSON shape. Recorded in [04_improvements.md](04_improvements.md).
- **Rejected for v1: a curated catalogue only.** A catalogue does not serve "someone told me to use this MCP" for a server outside the catalogue. Recorded in [04_improvements.md](04_improvements.md).

### Runtimes for local servers: mise in the workspace

- **Chosen:** `mise use node@lts` or `mise use uv` on demand, the path Claude Code and Paseo already take. The runtime is recorded in `mise.toml` and reproduced on remote deploy.
- **Rejected: baking Node and uv into the base image.** Baking grows every sandbox, needs a `BASE_IMAGE_VERSION` bump, and splits runtime management between the image and mise.

### Apply model: the GitHub reconcile pattern

- **Chosen:** copy `github_access.rs`, which already handles stopped sandboxes, recreated homes, deployed sandboxes and racing triggers.
- **Rejected: writing configs only at toggle time.** Configs written only when the switch flips would be lost on recreate, would miss stopped sandboxes, and would miss agents installed later.
- **Per-worktree isolation:** all state is under the app-data root, which the e2e harness relocates per worktree, so the feature adds no host-global state.

## Validation

The gating spike ran only partially on 2026-11-19: Podman was unavailable, so only Claude Code's config location, `headersHelper`, `${VAR}` and a launcher script were confirmed, on the host (see [spike/mcp-config-locations.md](spike/mcp-config-locations.md)). Claude Code expands `${VAR}` from its own process environment, so the stdio secret reference must be the launcher form. OpenCode, pi and the Paseo launch path are untested. On 2026-11-18, research read the public documentation:

- Paseo: `https://paseo.sh/docs/mcp.md`, `https://paseo.sh/docs/plugins/hooks`, `https://paseo.sh/docs/configuration.md` and `https://paseo.sh/docs/custom-providers.md`.
- pi: the MCP, security and changelog pages under `https://github.com/earendil-works/pi/tree/main/packages/coding-agent`, `main` branch. The pinned 1.5.0 install also ships an MCP documentation page, listed in the system prompt recorded in `src-tauri/src/agent_host/fixtures/01-roundtrip.jsonl`.

The remaining gating checks should run before `02_dev.md`. In one Podman sandbox with all three agents installed, it should confirm that:

- Each agent lists a no-auth remote server and a stdio server (`npx -y @modelcontextprotocol/server-everything`) when configured at the locations in the rendering table.
- Each secret reference form resolves: Claude Code `headersHelper`, OpenCode `{file:}` and pi `!command`.
- pi reads `mcp.json` from `PI_CODING_AGENT_DIR` and connects in `--mode rpc`.
- An agent started from Paseo sees the servers.

## Open Questions

- **Library page placement:** a new sidebar page named "MCP servers", or a section of the existing "Accounts and logins" page (`src/routes/credentials/+page.svelte`). Users are told to "use this MCP", which argues for putting "MCP" in the name either way.
- **Managed-entry ownership:** how the reconcile recognises its own entries in a shared config file. Options: a reserved name prefix such as `mio-`, which is visible to the user and in tool names; or a ledger in `<app_data_root>/mcp/<sandbox>.json` of the names it wrote, which survives renames but drifts if the user edits the file.
- **Connection health:** whether the Manage section shows "connected" as well as "applied". Options: exec `pi mcp list --json`, available only when pi is installed; a host-side probe of remote URLs, which does not see the sandbox's egress policy; or no health signal in v1.
- **pi exposure for library servers:** which `exposure` the reconcile writes into pi's `mcp.json`. `codemode` is pi's default, `deferred` loads tools through tool search, and `direct` declares every tool. Small local models handle `direct` best, but `direct` costs context. `chat-mcp-learning` needs a decision for its demo server.
- **Credential shape for multi-field secrets:** `save_secret()` stores one value per service id. Options: one credential per server holding a map, or one credential per field (`mcp-<server>-<FIELD>`).
- **Running sessions after a change:** agents read MCP config when a session starts. The options are: leave running Paseo agent sessions alone and tell the user; restart the chat's pi through `pi-host.sh`; or send pi `/reload` over RPC. Which of these the reconcile does is undecided.
- **OAuth callback route (Phase 2):** pi, Claude Code and OpenCode each run the OAuth callback on a loopback port inside the sandbox. Options:
  - A Mutagen forward from the same host port into the sandbox for the length of the sign-in.
  - The paste-the-redirected-URL fallback that pi documents for SSH sessions.
  - A host-side sign-in whose tokens the app injects.

  pi stores tokens in `mcp-auth.json` in its agent directory, which is in the workspace. Any option that lets pi store tokens therefore conflicts with the no-secrets-in-the-workspace invariant unless pi's token file moves out of the workspace.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A server switched on for a sandbox does nothing in Claude Code, OpenCode or Paseo after the sandbox was recreated | The rendered configs live in the container home (`/root/.claude.json`, `/root/.config/opencode/opencode.json`), which a recreate discards | Run the MCP reconcile from `start_sandbox` (`src-tauri/src/commands/sandbox.rs`) so every start restores the home configs |
| An MCP OAuth token or a pasted key appears under the workspace's `.mio/pi-agent/` | pi's agent directory (`PI_CODING_AGENT_DIR`, `src/lib/agents/pi.ts`) is inside the workspace, so pi's own `mcp.json` values and `mcp-auth.json` land in the synced folder | Keep pi's `mcp.json` free of values (reference `/root/.config/mio/mcp/` with `!command`), and resolve Open Question "OAuth callback route" before allowing pi to store tokens |
| pi ignores servers in a project-level `.pi/mcp.json` in the native chat | pi reads project MCP config only after project trust, and `--mode rpc` cannot show the trust prompt | Write pi's servers to `.mio/pi-agent/mcp.json` (the agent directory), never to the project's `.pi/mcp.json` |
| A remote server added from a README fails in pi with an unsupported transport | pi rejects the legacy SSE transport (`type: "sse"`); only stdio and streamable HTTP work | Refuse SSE in the import parser (`src-tauri/src/mcp/import.rs`) and ask for the server's streamable HTTP (`/mcp`) endpoint |
| A switched-on server times out in a proxied sandbox | The sandbox's allowlist in `.mio/net-policy.json` lacks the server's host, or the package-registry hosts `npx` or `uvx` download from | Add the library entry's egress hosts through an accepted-policy apply (`apply_sandbox_policy`, `src-tauri/src/commands/net_policy.rs`) from the Manage section's "blocked by network policy" offer |
| A local server fails with `npx: not found` | The base image has no Node, and the reconcile wrote the entry before the runtime existed | Ensure the entry's runtime with `mise use` in the workspace before rendering the server into any agent config |
| The local model never calls an MCP tool that pi lists | pi's default `exposure` is `codemode`, which hides MCP tools from the model unless it writes codemode scripts | Set the server's `exposure` in `.mio/pi-agent/mcp.json`; see Open Question "pi exposure for library servers" |
| Editing `/root/.claude.json` loses the Claude login or onboarding state | `inject()` and `seed_onboarding()` (`src-tauri/src/credentials/mod.rs`) also write `/root/.claude.json` | Merge only the top-level `mcpServers` key, under the same reconcile lock, and never rewrite the whole file |
