---
type: spike
title: "Spike 01: Local-Model Tool-Call Stream"
feature: chat-agent-anatomy
---
# Spike 01: Local-Model Tool-Call Stream

Hands-on spike run 2026-11-22 on the Windows 11 development host. It records what pi 1.5.0 emits over RPC when the local model makes tool calls, which `docs/features/chat-agent-anatomy/01_ideas.md` (Validation) listed as unobserved: no fixture in `src-tauri/src/agent_host/fixtures/` held a tool call. The throwaway sandbox, the host model server and the SSH forward were removed at the end; the scripts and traces in this directory remain.

## Setup

- **Sandbox.** A throwaway Podman container `mio-spike-anatomy` on `podman-machine-default` from the current base image `sandbox-mio/base:10`, not created through the app. Its workspace, in the session scratchpad, held the `mise.toml` (`pi = "1.5.0"`, `PI_CODING_AGENT_DIR = /root/work/.mio/pi-agent`), `models.json` and `settings.json` copied from the user's onboarding `hello-world` sandbox. The user's own `hello-world` sandbox was not touched.
- **pi.** pi 1.5.0 (`PI_PINNED_VERSION` in `src/lib/agents/pi.ts`) installed by `mise install`, run as `pi --mode rpc --no-session` directly, without the `pi-host.sh` supervisor. The supervisor only relays stdout into a journal, so the record stream is the same.
- **Model.** The app's own `llama-server.exe` serving `qwen3.5-4b` (Q4_K_M, `--ctx-size 32768`), the model the onboarding gave `hello-world`, on a host loopback port. One run used `qwen3.5-2b` under the same model id. The server was reverse-forwarded to sandbox port 51199 with Windows OpenSSH `ssh -R` over the container's `mio-sshd`, using a spike-only key authorised in the throwaway container only, in place of the app's Mutagen forward.
- **Artefacts.** `record.mjs` (host recorder: spawns pi through `podman exec -i`, sends one `prompt`, writes each record as `{ms, record}` with `ms` since the prompt, closes stdin at `agent_settled`) and `summarize.mjs` (collapses a trace into one line per beat). Traces are in `traces/`. The host model path that llama-server echoes as `responseModel` was redacted to the model id.

## Runs

| Trace | Model | Prompt | Outcome |
|---|---|---|---|
| `traces/01-list-files.jsonl` | 4B | "What files are in the current folder?" | One `bash` call (`ls -la`), then a reply. 13.1 s to `agent_settled`. |
| `traces/02-write-then-read.jsonl` | 4B | "Create a file called notes.txt containing the word hello, then read it back to check it worked." | `write`, then `read`, then a reply, over three turns. Two `Connection error.` failures, each recovered by one auto-retry. 17.4 s. |
| `traces/03-parallel.jsonl` | 4B | "Run these two commands at the same time, as two separate tool calls in one go: uname -a and date. Then tell me both results." | Two `bash` calls in one assistant message, executed concurrently, then a reply. 14.5 s. |
| `traces/04-write-then-read-rerun.jsonl` | 4B | Same as trace 02 | `write`, `read`, reply, no errors. 13.4 s. |
| `traces/05-list-files-2b.jsonl` | 2B | Same as trace 01 | One `bash` call (`ls -la`), then a reply. 9.9 s. |

## Results

| # | Question from `01_ideas.md` | Result | Evidence |
|---|---|---|---|
| 1 | Record order for one tool call | **Answered.** Within the assistant message: `thinking_start`, `thinking_delta`s, `toolcall_start` (carries `id` and `toolName`), `toolcall_delta`s, then `thinking_end` and `toolcall_end` together at the end. Then `message_end` (`stopReason: "toolUse"`, content `[thinking, toolCall]`), `tool_execution_start`, zero or more `tool_execution_update`s, `tool_execution_end` (with `durationMs`), a `toolResult` `message_start`/`message_end` pair, `turn_end`, and a new `turn_start` for the reply. | `traces/01-list-files.jsonl` |
| 2 | Typical tool duration | **Answered.** `durationMs` 7 to 90 ms across every call (`write` 7 and 9, `read` 16 and 26, `bash` 34 to 90). `tool_execution_start` follows `toolcall_end` within 1 ms. A tool part therefore spends about 1 ms in state `input` and under 100 ms in `running`. | All traces |
| 3 | Thinking before a tool call | **Yes.** Every assistant message in every trace opened with thinking, before tool calls and before replies, on both models, although no thinking level was set. | All traces |
| 4 | Concurrent tool calls | **Concurrent.** The two calls of trace 03 started 2 ms apart and ended in the same millisecond; their `toolcall_start`s streamed one after the other, but both `toolcall_end`s arrived together at message end. The extensions guide bundled with pi 1.5.0 (under `$(mise where pi)`) states that tool calls from one assistant message run in parallel by default and that `tool_execution_end` may arrive in completion order. | `traces/03-parallel.jsonl` |
| — | Does the 2B model call tools | **Yes, when the prompt needs one.** The native-agent-ui spike's 2B answered a question that needed no tool; here it ran `ls -la` like the 4B. | `traces/05-list-files-2b.jsonl` |

## Findings That Change the Plan

- **`thinking_end` is held until the message ends.** The thinking block's deltas stop before the next block starts, but its end event arrives only with the message's final `toolcall_end` or `text_end`. A reasoning part therefore stays `streaming` while the tool request or the reply streams after it, and today's chat shows "Thinking…" over a reply that is already arriving. The current beat cannot be read from the `streaming` flags; it must come from the newest opened block.
- **The Brain's tool request is visible only through `toolcall_start`.** For a single call, `toolcall_start` arrives 0.5 to 1.4 s before `toolcall_end`, while the model writes the arguments. `applyPiRecord()` in `src/lib/agent-chat/pi-events.ts` ignores `toolcall_start`, so the tool part appears at `toolcall_end`, 1 ms before execution starts, and the "Brain asks" moment has no on-screen time of its own.
- **The model reads silently first.** Between the assistant `message_start` and the first `thinking_delta` the stream is silent for 0.4 to 3.7 s (prompt processing; longest on the first turn). The Brain should look busy from `message_start`.
- **A run can fail and retry mid-run.** In trace 02 the assistant message after each tool result ended at once with `stopReason: "error"`, empty content and `errorMessage: "Connection error."`, followed by `agent_end`, `auto_retry_start` (`attempt` 1 of 3, `delayMs` 2000), a fresh `agent_start`, the successful message, and `auto_retry_end` (`success: true`). `agent_settled` came only once, at the very end. The rerun (trace 04) had no errors, and llama-server logged no failure, so the spike's SSH forward is the likely cause; whether the app's Mutagen forward shows the same is untested. Either way the chat must cope with an empty errored assistant message and a retry inside one run.
- **Records the adapter ignores:** `turn_start`, `turn_end` and `entry_appended` arrive in every run; `turn_start` marks each pass of the Brain-Arms loop.

## Not Covered

- The app's real transport: the `pi-host.sh` supervisor, the attach stream and the Mutagen inference forward.
- Tool errors (`isError: true`), aborts during tool execution, and long-running tools with many `tool_execution_update`s.
- Models other than the 2B and 4B Qwen 3.5 builds.

## Observation Outside This Feature

llama-server reports the model's full host file path, including the Windows user folder, as `responseModel` in every streamed chunk unless it is started with `--alias`. The app's launch in `src-tauri/src/inference/llama.rs` passes no `--alias`, so the real chat journal and pi's session files in the workspace likely carry the host path as well.

## Blockers Hit

- The container could not reach the host model server: `curl http://host.containers.internal:51299/health` failed, because llama-server binds to host loopback only. Fixed with an `ssh -R` reverse forward over `mio-sshd`.
- The first redaction attempt left the traces unchanged: Git Bash collapsed the doubled backslashes in an inline regular expression. Writing the redaction script to a file fixed it.
