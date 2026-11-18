---
type: ideas
title: "MCP in the Native Chat, for Learning"
feature: chat-mcp-learning
---
# MCP in the Native Chat, for Learning

This document scopes a small MCP learning surface in the native chat, which drives pi in RPC mode inside a sandbox. It has two parts. First, a built-in demo MCP server ships in the app binary and works offline with the local model and no account. Second, the chat makes MCP visible: it shows which MCP servers and tools pi has, and marks each tool call that went to an MCP server. The demo server becomes a built-in entry of the MCP library from `mcp-servers` ([01_ideas.md](../mcp-servers/01_ideas.md)), so it is switched on per sandbox like any other server. The point is that switching it on teaches the real flow. The chat code lives in `src/lib/agent-chat/` (`pi-events.ts`, `ToolPart.svelte`, `ChatHeader.svelte`, `chat.svelte.ts`), the in-sandbox supervisor in `src-tauri/resources/agent-host/` and its installer in `src-tauri/src/agent_host/mod.rs`. Later learning material, such as an onboarding chapter, is out of scope ([04_improvements.md](04_improvements.md)).

## Current State

- **MCP is already live in the chat's pi.** `pi-host.sh` starts `pi --mode rpc --session-dir …` without `--no-mcp`. The pinned pi 1.5.0 (`PI_PINNED_VERSION` in `src/lib/agents/pi.ts`) loads `mcp.json` from its agent directory, `.mio/pi-agent/` in the workspace. Nothing writes that file today.
- **Tool calls already stream as events.** `pi-events.ts` (`src/lib/agent-chat/pi-events.ts`) maps pi's `tool_execution_start`, `tool_execution_update` and `tool_execution_end` records, which carry `toolName`, `args` and `result`, into tool parts. pi names an MCP tool `mcp__<server>__<tool>`, so an MCP call already arrives as an ordinary tool part with that name.
- **Tool rendering is keyed by name.** `ToolPart.svelte` (`src/lib/agent-chat/ToolPart.svelte`) switches on `part.toolName` for `bash`, `edit`, `write` and `read`, and falls back to a generic trace for any other name. An MCP call renders today as a generic trace titled `mcp__…`.
- **The session's tool list is in the stream but unused.** pi's system `message_start` record carries `toolsAdded` with every tool's name, description and parameters (recorded in `src-tauri/src/agent_host/fixtures/01-roundtrip.jsonl`). `pi-events.ts` does not read `toolsAdded`.
- **Supervisor scripts already ship in the binary.** `pi-host.sh` and `pi-attach.sh` are `include_str!`-ed into `src-tauri/src/agent_host/mod.rs` and installed on demand into `/usr/local/lib/mio` (`SCRIPT_DIR`), with a script version check.
- **The local model is small.** The fixture shows a 2B-parameter model. pi's default MCP `exposure`, `codemode`, makes the model write scripts to reach MCP tools, which a model of that size handles poorly.
- **No Node in the base image.** `node@lts` arrives only through mise (`src-tauri/resources/Containerfile`, `src/lib/agents/catalogue.ts`).

## The Demo MCP Server

- **What ships:** one dependency-free JavaScript file in a new `src-tauri/resources/mcp-demo/`, `include_str!`-ed and installed into `/usr/local/lib/mio` the same way the supervisor scripts are. The file speaks MCP's stdio transport (newline-delimited JSON-RPC: `initialize`, `tools/list` and `tools/call`) and nothing else.
- **Tools:** a few harmless, offline, deterministic tools, each showing a different MCP idea. A tool with no arguments (current time) shows the simplest call. A tool with typed arguments (roll dice: count and sides) shows the input schema. A tool with a visible side effect (add a note, list notes) writes a file in the workspace that the user can open from the sandbox's Folder button. The exact set is Open Question "Demo tool set".
- **Library entry:** `mcp-servers` gains a built-in, non-removable library entry, working name `demo`. A short name keeps the tool names readable (`mcp__demo__roll_dice`). pi replaces `-` with `_` in tool names, which also argues against a hyphenated name. The entry's runtime is `node@lts`, it needs no egress hosts, and pi gets it with `exposure: "direct"` so the small model sees the tools as ordinary tool definitions.
- **Switching it on** goes through the `mcp-servers` reconcile like any other server, so the learner sees the real steps: the library, the per-sandbox switch, and the agent picking the server up.

## Making MCP Visible in the Chat

- **Tool part:** `ToolPart.svelte` recognises the `mcp__<server>__<tool>` prefix. It shows the tool name with an "MCP · `<server>`" badge, the arguments sent and the content returned. A learner can then tell a built-in tool (`bash`) from one an MCP server provided.
- **Tools popover:** `pi-events.ts` reads `toolsAdded` from the system message. `ChatHeader.svelte` gains a "Tools" popover (`popover` from `src/lib/components/ui/`) that lists built-in tools and, grouped per server, the MCP tools. Each tool shows its description.
- **First-use prompt:** when the demo server is on and the session is empty, the chat's empty state suggests one prompt that triggers a demo tool ("Roll two dice for me"). A one-line "What is MCP?" explainer links to the tools popover.
- **Pickup after a change:** pi reads `mcp.json` at session start, so a server switched on while the chat runs is absent until pi restarts. The chat shows a "Tools changed — restart the chat to load them" notice when the `mcp-access://changed` event names its sandbox. How the restart happens is Open Question "Restart versus reload".

## Technology Choices With Tradeoffs

### Demo server runtime: one dependency-free Node script

- **Chosen:** a single JavaScript file run with `node`, which mise provides as `node@lts` and Claude Code already uses. The file ships in the app binary, so the server works offline and needs no registry hosts in the egress policy.
- **Rejected: the official TypeScript MCP SDK, fetched with `npx`.** The SDK needs registry egress at first run, drifts with upstream versions, and hides the protocol the demo exists to show.
- **Rejected: Python.** Python is not in the base image, and the server would add `uv` as a second runtime next to the `node@lts` that Claude Code already brings.
- **Rejected: a compiled Rust binary.** A binary needs a cross-compile for x86_64 and aarch64 Linux plus a build pipeline, for a server of under two hundred lines.
- **Rejected: POSIX `sh`.** The base image has no `jq`, and hand-parsing JSON-RPC in `sh` is fragile.
- **Rejected: a hosted remote demo server.** A hosted server needs the network, hosting and an egress allowlist entry, while the local-model chat otherwise works offline.

### Demo delivery: a library entry, not a chat-only feature

- **Chosen:** the demo server is a library entry switched on per sandbox, so the learning path is the real path.
- **Rejected: a pi extension that registers tools directly (`pi.registerTool`).** Extension tools are not MCP, so a learner would see no MCP server, and nothing learned would transfer to Paseo or Claude Code.
- **Rejected: always on in every chat.** Tools appearing without a visible switch teach nothing about how a server is added.

### pi exposure for the demo: `direct`

- **Chosen:** `direct`, which declares the demo's few tools to the model like built-ins. The demo has three or four tools, so the context cost is small.
- **Rejected: `codemode` (pi's default).** The model must write a script to reach a tool, which small local models do poorly and which obscures the one-call-one-tool picture.
- **Rejected: `deferred`.** The model must call `tool_search` first, which adds a step a learner has to understand before seeing an MCP call.

### Source of the tool list: `toolsAdded` in the stream

- **Chosen:** read `toolsAdded` from the system message the chat already receives. No extra exec, and the list is exactly what the model sees.
- **Rejected: exec `pi mcp list --json` in the sandbox.** It costs an extra round trip per check, and it reports configured servers rather than the tools the session holds. It could still serve failed-connection detail; see Open Question "Showing a server that failed to connect".
- **Unverified:** whether `toolsAdded` includes MCP tools under `direct` exposure. The gating spike checks this.

## Validation

No spike has run. The design rests on pi's MCP, RPC and JSON-event documentation pages under `https://github.com/earendil-works/pi/tree/main/packages/coding-agent` (`main` branch, read 2026-11-18) and on the recorded RPC stream in `src-tauri/src/agent_host/fixtures/01-roundtrip.jsonl`. A gating spike should use pi 1.5.0 in `--mode rpc` with the local model, and a hand-written `mcp.json` in `PI_CODING_AGENT_DIR` that points at a draft demo script with `exposure: "direct"`. It should confirm that:

- pi connects to the demo script in RPC mode.
- The system message's `toolsAdded` lists the `mcp__demo__…` tools.
- The local model calls a demo tool when asked.
- The `tool_execution_*` records carry the `mcp__demo__…` name.

## Open Questions

- **Demo tool set:** which three or four tools ship. A notes tool writes into the workspace, which shows a real side effect but leaves a file in the user's project. A location under `.mio/` keeps the file out of the way but less visible.
- **Restart versus reload:** how the chat loads a changed server set. Options: restart pi through `pi-host.sh`, which drops nothing because pi rebuilds history from its session file; or send pi's `/reload` over RPC, which is undocumented for RPC mode.
- **Showing a server that failed to connect:** pi reports failed connections "once after startup", and the docs do not say how the report reaches an RPC client. Options: parse `stderr.log` in the supervisor state directory, read `~/.pi/agent/mcp.log` (or its `PI_CODING_AGENT_DIR` equivalent), exec `pi mcp list --json`, or show nothing in v1.
- **Demo server for other agents:** whether the demo entry also renders into Claude Code and OpenCode when they are installed. It costs nothing extra through the `mcp-servers` reconcile, but the learning copy is written for the chat.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The demo server is switched on but the chat shows no MCP tools | pi reads `mcp.json` only at session start, and the chat's pi was already running | Restart the chat's pi after an `mcp-access://changed` event for the sandbox; see Open Question "Restart versus reload" |
| The local model answers in prose and never calls a demo tool | The demo entry was rendered with pi's default `codemode` exposure, which hides MCP tools from the model | Render the demo entry into `.mio/pi-agent/mcp.json` with `exposure: "direct"` |
| A demo tool appears as `mcp__mio_demo__…` instead of the expected name | pi replaces every character other than letters, digits and `_` with `_` in MCP tool names | Name the library entry with letters, digits and `_` only, e.g. `demo` |
| The demo server fails to start with `node: not found` | The base image has no Node, and `node@lts` arrives only through mise | Ensure `node@lts` with `mise use` in the workspace before rendering the demo entry, as for any local library entry |
| An MCP tool call renders as a generic trace with no server badge | `ToolPart.svelte` (`src/lib/agent-chat/ToolPart.svelte`) switches on exact tool names and has no prefix match for `mcp__` | Match the `mcp__<server>__<tool>` prefix in `ToolPart.svelte` before the exact-name cases |
| The demo script runs an outdated version after an app update | The script is installed into `/usr/local/lib/mio` in the sandbox and kept until replaced | Version-check the script on install, the way `src-tauri/src/agent_host/mod.rs` checks `SCRIPT_VERSION` for the supervisor scripts |
