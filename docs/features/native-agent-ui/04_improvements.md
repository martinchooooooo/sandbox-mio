---
type: improvements
title: "Native Agent UI Improvements"
feature: native-agent-ui
---
# Native Agent UI Improvements

Work deliberately left out of the first native agent UI build (`docs/features/native-agent-ui/01_ideas.md`), with the reason each item was deferred.

## Other agents through the Agent Client Protocol

Drive OpenCode (`opencode acp`) and Claude Code (through its ACP adapter) from the same chat view by adding an ACP event adapter next to the pi adapter in `src/lib/agent-chat/`. Deferred because v1 targets local models, where pi is the primary agent, and ACP drops pi-specific features.

## Several concurrent chats per sandbox

Run one supervisor per conversation so two pi sessions in one sandbox can work at once. Deferred because v1 runs one pi process per sandbox and switches sessions with `switch_session`.

## Session tree view

Render pi's full session tree from `get_tree` with branch navigation. Deferred because v1 offers the session list and forking from a user message, which covers the common case.

## Tool approvals

Ship a pi extension, provisioned into `.mio/pi-agent/`, that asks `confirm` before risky bash commands, and render the request with the Confirmation pattern. Deferred because the sandbox is the safety boundary and pi has no built-in approval gate.

## File attachments

Attach PDFs, documents and images to a prompt, extracting text the way `pi-web-ui`'s `loadAttachment` does. Deferred because pi's RPC accepts only images, and document extraction needs its own dependency choice.

## Open in Zed

A Manage launcher that opens the sandbox in Zed with a custom ACP agent reaching `pi-acp` over the sandbox's SSH alias. Deferred because Zed's custom-agent spawn on Windows fails with "os error 193" when the command wraps `ssh` (zed-industries/zed issue 65450), and host and sandbox paths differ.

## Journal rotation

Rotate the supervisor's journal at `agent_settled` boundaries, starting a new generation per rotation. Deferred because v1 writes a new journal per supervisor start (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 4); measure journal size over a long real session before building rotation.

## Idle stop of pi

Stop the supervisor after a period with no attach and no run, to free sandbox memory. Deferred because v1 starts pi lazily and stops it only on request or with the sandbox (Resolved Decision 12).

## Deployed sandboxes

Attach to a sandbox deployed to a remote host. `MachineProvider::exec_command` would follow the route's machine, but remote latency, session files reaching the host through Mutagen, and the remote engine's exec behaviour on a dropped client are untested. Deferred by Resolved Decision 11.

## Frontend unit tests for the pi event adapter

Add a unit-test runner for `src/lib/agent-chat/pi-events.ts` that feeds recorded traces in and asserts message lists out. Deferred because the frontend has no unit-test runner and the `light` tier already replays traces through the adapter (Resolved Decision 13).

## Foundry context window

Probe the context size Foundry Local actually serves and provision it as pi's `contextWindow`. Deferred because v1 writes the catalogue value for the Foundry engine (Resolved Decision 7).

## Multi-writer relay

Replace the single-writer `flock` with a relay that lets several clients write safely. Deferred because one writer plus Take control covers the two-window case (Resolved Decision 5).

## Timed-out dialogs

A dialog request carrying `timeout` is resolved by pi itself when the time runs out, and pi emits no event for it, so the chat keeps showing the dialog until the next fresh attach; an answer sent after the timeout is ignored by pi. Expiring the dialog in the store from its `timeout` would close the gap. Deferred because no provisioned extension raises timed dialogs yet.

## Mock agent host knobs

Extend `MockAgentHost` (`src-tauri/src/agent_host/mock.rs`) with a mock-only control command, guarded like `mock_configure` in `src-tauri/src/mockctl/mod.rs`, that sets whether `attach` reports the writer until `takeover` runs and which pi version `ensure` reports, plus a trigger word replaying a run that ends in `stopReason: "error"` and a recorded trace with tool calls, queue updates, compaction and retries. The knobs would let the `light` tier reach the read-only, outdated-pi and inference-down states and the tool, queue and notice rendering (`docs/features/native-agent-ui/03_e2e.md`, Proposed Mock Seams). Deferred because the light spec as written needs none of them, the standard spec covers the read-only and inference-down states on real infrastructure, and the rich trace must first be recorded from real pi 1.5.0 with a tool-capable model.
