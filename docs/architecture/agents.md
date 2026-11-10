---
type: architecture
title: "Coding Agents and Credentials"
description: "The agent catalogue, installing agents into sandboxes, logging in once and reusing the login everywhere, local-model wiring, and the Paseo agent web UI."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-10T18:00:00+11:00
stale_after: 2027-02-05T00:00:00+11:00
code:
  - src/lib/agents/
  - src-tauri/src/agents/
  - src-tauri/src/credentials/
  - src-tauri/src/commands/agents.rs
  - src-tauri/src/commands/credentials.rs
  - src/routes/credentials/
  - src/routes/projects/AgentManager.svelte
sources:
  - resource: docs/features/agent-setup/02_dev.md
  - resource: docs/features/shared-agent-auth/02_dev.md
  - resource: docs/features/credentials-inline-auth/02_dev.md
  - resource: docs/features/opencode-agent/02_dev.md
  - resource: docs/features/pi-agent/02_dev.md
  - resource: docs/features/agent-web-ui/02_dev.md
  - resource: docs/features/local-llm-reachability/02_dev.md
---
# Coding Agents and Credentials

The app installs coding agents (Claude Code, OpenCode, pi) into a sandbox, lets the user log an agent in once and reuse that login in every sandbox, points the local-model agents at the host inference server, and offers Paseo as a browser alternative to the terminal.

## The Agent Catalogue

- One frontend catalogue declares every installable agent: its mise packages, post-install steps, how it authenticates (login command, credential file, expiry, onboarding state), an optional provisioning step, and the API hosts it calls.
- Agent behaviour is declared in the catalogue; components never branch on agent names.
- The catalogue's API hosts seed the starter allowlist for network policy ([Network](network.md)).

## Install

- Installing runs the agent's packages through mise inside the sandbox, so the agent is recorded in `mise.toml` and reproduced on remote deploy, then runs the post-install steps. Progress shows in the activity center.
- If the host credential store holds a login for the agent, install injects it, so a fresh agent starts signed in.

## Login and the Credential Store

- Login runs in a short-lived auth container rather than a user sandbox, so logging in needs no sandbox. Auth containers are hidden from the sandbox list.
- The login command runs with piped I/O. Its output streams to an inline card on the Credentials page, and the user pastes the returned code into the same card.
- After login, the app captures the agent's whole config directory, which carries onboarding state as well as the token, into a credential store in the app-data root.
- Any sandbox can then receive that bundle; injection also marks the agent's first-run onboarding complete. Auth status is read from the agent's credential file and its expiry.

## The GitHub Token

- The credential store also holds one single-value credential: a GitHub token the user pastes on the Credentials page. It is write-only over IPC; the page only learns whether a token is saved and when.
- Every in-sandbox mise call that resolves or downloads tools carries the token as `GITHUB_TOKEN` and `MISE_GITHUB_TOKEN`, which lifts GitHub's unauthenticated rate limit. With no token saved, nothing changes. The base-image build does not receive it.
- The token travels as a secret exec environment ([Providers](providers.md)), never in a command line.

## Local-Model Agents: OpenCode and pi

- OpenCode and pi are provisioned to use the host inference server as a model provider ([Local Inference](inference.md)). Provisioning writes their configuration into the workspace from the host, so it works whether or not the sandbox is running. OpenCode's configuration is merged into any existing user settings; pi's lives in its own file under `.mio/`.
- Every provisioned agent in every sandbox is re-provisioned, best-effort, at app startup and after each server start. That keeps the model list and server address current and migrates older workspaces.
- An agent's default model follows the running server, unless the user picked a model from another provider in the agent itself.

## Agent Web UI (Paseo)

- Paseo runs inside the sandbox as a pinned mise tool and is reached through a forward, like the terminal ([Workspace UI](workspace-ui.md)).
- Before Paseo starts, the app registers the sandbox workspace as a Paseo project, so the UI opens on that project.
- Manage offers Paseo only once an agent is installed.

## Invariants

- Credential bundle contents and the GitHub token never cross IPC or reach logs; only metadata does.
- Provisioning merges into a user's agent configuration rather than replacing it.
- An agent's behaviour is declared in the catalogue.
