---
type: architecture
title: "Coding Agents and Credentials"
description: "The agent catalogue, installing agents into sandboxes, logging in once and reusing the login everywhere, local-model wiring, and the Paseo agent web UI."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-19T12:00:00+11:00
stale_after: 2027-02-17T00:00:00+11:00
code:
  - src/lib/agents/
  - src-tauri/src/agents/
  - src-tauri/src/agent_host/
  - paseo-plugins/
  - src-tauri/resources/agent-host/
  - src-tauri/resources/agent-web/
  - src/lib/agent-chat/
  - src-tauri/src/commands/agent_chat.rs
  - src-tauri/src/credentials/
  - src-tauri/src/commands/agents.rs
  - src-tauri/src/commands/credentials.rs
  - src/routes/credentials/
  - src/routes/projects/AgentManager.svelte
  - src-tauri/src/services/github_access.rs
  - src/lib/workspace/GithubAccess.svelte
sources:
  - resource: docs/features/agent-setup/02_dev.md
  - resource: docs/features/shared-agent-auth/02_dev.md
  - resource: docs/features/credentials-inline-auth/02_dev.md
  - resource: docs/features/opencode-agent/02_dev.md
  - resource: docs/features/pi-agent/02_dev.md
  - resource: docs/features/agent-web-ui/02_dev.md
  - resource: docs/features/native-agent-ui/02_dev.md
  - resource: docs/features/local-llm-reachability/02_dev.md
  - resource: docs/features/techdebt-sweep/02_dev.md
  - resource: docs/features/github-cli-auth/02_dev.md
  - resource: docs/features/proxy-credential-secrets/02_dev.md
  - resource: docs/features/paseo-plugins/02_dev.md
---
# Coding Agents and Credentials

The app installs coding agents (Claude Code, OpenCode, pi) into a sandbox, lets the user log in once and reuse the login everywhere, points local-model agents at the host inference server, and offers Paseo and a built-in pi chat.

## The Agent Catalogue

- One frontend catalogue declares every installable agent: mise packages, post-install steps, authentication (login command, credential file, expiry, onboarding state), an optional provisioning step, and API hosts.
- The catalogue's API hosts seed the starter allowlist for network policy ([Network](network.md)).

## Install

- Installing runs the agent's packages through mise inside the sandbox, so the agent is recorded in `mise.toml` and reproduced on remote deploy, then runs the post-install steps.
- Install injects the stored login for the agent, if any, so a fresh agent starts signed in.

## Login and the Credential Store

- Login runs in a short-lived, hidden auth container, so it needs no sandbox; output streams to an inline card on the Credentials page, where the user pastes the returned code.
- After login, the app captures the agent's whole config directory (token and onboarding state) into a credential store in the app-data root.
- Any sandbox can receive that bundle; injection also marks first-run onboarding complete.

## The GitHub Token

- One single-value credential, saved by pasting a token or by a device-flow login in the auth container, and verified by the host with GitHub.
- In-sandbox mise calls carry it as a secret exec environment ([Providers](providers.md)), never a command line.
- The base image carries the GitHub CLI. A serialised reconcile logs it into each running local sandbox over stdin on create, start, save, delete and the Manage switch, honouring a per-sandbox opt-out.
- Every sandbox sits behind a proxy sidecar, so gh and mise hold a fixed placeholder and the sidecar swaps in the real token on requests to GitHub's hosts; the token stays on the host ([Network](network.md)). An opted-out sandbox holds nothing, and a deployed sandbox is skipped.

## Local-Model Agents: OpenCode and pi

- OpenCode and pi are provisioned to use the host inference server as a model provider ([Local Inference](inference.md)). Provisioning writes their configuration into the workspace from the host, so it works while the sandbox is stopped. OpenCode's is merged into existing user settings; pi's lives under `.mio/`.
- Every provisioned agent is re-provisioned, best-effort, at app startup and after each server start, keeping model list and address current. Its default model follows the running server, unless the user picked another provider's model in the agent.
- pi gets one local model, "Local LLM", keyed by the served id.

## Agent Web UI (Paseo)

- Paseo runs in-sandbox as a pinned mise tool behind a forward ([Workspace UI](workspace-ui.md)); the workspace is its project, named after the sandbox.
- The daemon restarts only when `mise.toml`, the password or the set of app plugins changes; starting a Paseo-using sandbox pre-starts it.
- App plugins ship in the binary, are streamed outside the workspace when stale, and reload in place.

## Native Chat (pi)

- A chat pane in the main webview drives pi's RPC mode inside the sandbox over the engine's streamed exec ([Workspace UI](workspace-ui.md)).
- A small in-sandbox supervisor keeps one pi alive across disconnects and journals its output; the host reattaches from a cursor and installs the supervisor scripts on demand.
- One attach is the writer; any other attach is read-only until the user takes control.
- Chat sessions live in the workspace under their own directory, so chat and Paseo never write one session file. The chat asks for an update when the sandbox runs an older pi than the catalogue pins.
- Deployed sandboxes are unsupported. Rationale: [native-agent-ui plan](../features/native-agent-ui/02_dev.md).

## Invariants

- Credential bundle contents and the GitHub token never cross IPC or reach logs; only metadata does.
- The real GitHub token never enters a sandbox behind a sidecar; the GitHub binding is app-owned, never sandbox-writable.
- Provisioning merges into a user's agent configuration rather than replacing it.
- An agent's behaviour is declared in the catalogue.
- At most one attach per sandbox writes to pi, and no session file has two writers.
