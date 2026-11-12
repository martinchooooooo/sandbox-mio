---
type: spike
title: "Spike 01: pi RPC Supervisor, Attach and Reattach"
feature: native-agent-ui
---
# Spike 01: pi RPC Supervisor, Attach and Reattach

Hands-on spike run 2026-11-11 on the Windows 11 development host. It clears all four Validation risks of `docs/features/native-agent-ui/01_ideas.md` on Podman, repeats the supervisor and dialog risks on microsandbox (`msb 0.7.12`), and gathers evidence on three Open Questions (journal growth, pending dialogs, concurrent writers). Both spike sandboxes, the spike volume and the host model server were removed at the end; the scripts and traces in this directory remain.

## Setup

- **Sandboxes.** A Podman container `mio-spike-nau` on `podman-machine-default` and a microsandbox `mio-spike-nau-msb` with its own 2 GB disk volume, both from the current base image `sandbox-mio/base:7`. Neither went through the app: the workspace held a hand-written `mise.toml` (`pi = "latest"`, `PI_CODING_AGENT_DIR = /root/work/.mio/pi-agent`), `models.json` and `settings.json` matching what `provision_pi` (`src-tauri/src/agents/config.rs`) writes.
- **pi.** `mise install` resolved **pi 1.5.0** (`github:earendil-works/pi`), a newer major than the host's npm pi 0.79.4. The protocol facts below are from the RPC, JSON event stream and RPC extension UI documents bundled in `$(mise where pi)/docs/` for 1.5.0, and from captured traces.
- **Transport.** A spike-only SSH config (not `~/.ssh`) with the same ProxyCommand forms the app writes: `podman --connection … exec -i <name> mio-sshd` and `msb exec --stream <name> -- mio-sshd` (`src-tauri/src/backends/mod.rs`). Windows OpenSSH (`C:\Windows\System32\OpenSSH\ssh.exe`) was the client.
- **Model.** The app's own `llama-server.exe` serving `qwen3.5-2b` (Q4_K_M, `--ctx-size 32768`) on a host loopback port, reverse-forwarded to sandbox port 51199 with `ssh -R` in place of the app's Mutagen forward. The msb runs needed no model: they used the extension command only.
- **Artefacts.** `pi-host.sh` (supervisor: `start`, `status`, `stop`), `pi-attach.sh` (one per SSH session), `spike-confirm.ts` (an extension whose `/spikeconfirm` command raises a blocking `confirm`), `driver.mjs` (host client: splits on LF only, tags each record with its journal byte offset, persists a `{generation, offset}` cursor, can hard-kill SSH), `pending.mjs` (derives unanswered dialogs). Traces are in `traces/`, one record per line as `{offset, bytes, ms, record}`.

## Supervisor Shape That Was Tested

- `pi-host.sh start` creates a named pipe and an empty journal under `/run/mio/pi-host/`, writes a generation id, starts a keeper (`sleep infinity` holding the pipe open for writing) under `setsid`, then starts `setsid mise exec -- pi --mode rpc` in `/root/work` with stdin from the pipe and stdout appended to the journal.
- `mise exec` replaces itself with pi: the recorded pid is pi's own, in its own session.
- `pi-attach.sh <generation> <offset|end>` prints one synthetic `mio_attach` header line, then runs `tail -c +<offset+1> -f --pid=$$` on the journal and copies stdin into the pipe through `tee -a input.jsonl`. A generation mismatch resets the offset to 0.
- `pi-host.sh stop` kills the keeper only; pi sees end-of-file on stdin, which 1.5.0 documents as an orderly shutdown request.

## Results

| # | Risk | Result | Evidence |
|---|---|---|---|
| 1 | Prompt round trip over `ssh <alias>` against the local model | **Cleared.** 6.5 s from attach to `agent_settled`; 149 records, 56,840 journal bytes for a two-sentence answer with thinking. | `traces/01-roundtrip.jsonl` (first replay fixture) |
| 2 | pi survives a dropped connection; reattach from a stored offset is gap-free | **Cleared.** SSH hard-killed after 15 `text_delta`s; pi kept generating (journal 71,763 → 114,166 bytes in 8 s); reattach caught up in 481 ms. Offsets across both traces are contiguous and the joined `text_delta`s equal the `message_end` text (593 chars). | `traces/02a-drop.jsonl`, `traces/02b-reattach.jsonl` |
| 3 | Session files land in the workspace, readable host-side while stopped | **Cleared (Podman).** After `podman stop` the host read `.mio/pi-agent/sessions/--root-work--/<timestamp>_<uuid>.jsonl`: a `session` header (version 3, `cwd` `/root/work`), `model_change`, `thinking_level_change`, one system, two user and two assistant `message` entries, each with a `parentId`. Not tested on msb, where the workspace reaches the host through Mutagen. | Session header quoted in this row |
| 4 | An `extension_ui_request` raised while detached is recoverable | **Cleared, with a caveat.** pi blocks the command handler but keeps answering other commands (`get_state` replied while the dialog was open). Answering by id after reattach resumed the handler (`notify`, then the prompt's `disposition: "handled"` response). `get_state` has no pending-dialog field, so a client without its cursor cannot find the request from pi alone. | `traces/04b-dialog.jsonl` … `traces/04d-answer.jsonl` |
| — | Supervisor and dialog flow on microsandbox | **Cleared.** The supervisor started by one `msb exec` survived it (pi and keeper reparented to PID 1, `init.krun`). Attach through the msb alias, hard drop, reattach and answer all behaved as on Podman. Attach latency was 150–250 ms versus about 1.4 s on Podman. | `traces/05a-msb-dialog.jsonl` … `traces/05c-msb-result.jsonl` |
| — | No orphans after a hard drop | **Cleared on both engines.** No `tail`, `tee` or `cat` remained after the SSH client was killed: the proxy's end-of-file reaches `sshd -i`, which closes the session's stdin. | `ps` after each drop |
| — | Orderly stop | **Cleared.** Killing the keeper ended pi in 0.57 s with no signal sent to pi. | `pi-host.sh stop` printed `stopped (orderly)` |
| — | Restart resets the cursor | **Cleared.** A new `start` wrote a new generation; an attach carrying the old generation was served from offset 0. | `mio_attach` header `offset: 0` |

## Findings That Change the Ideas Document

- **`/run` is not a tmpfs** in either engine's sandbox (`/proc/mounts` lists no `/run` mount; PID 1 is `tini` under Podman, `init.krun` under msb). Supervisor state, including `pi.pid`, survives a sandbox restart. Post-restart PIDs are low (pi got pid 13), so a bare `kill -0` on a stale pid can match an unrelated process: with `pi.pid` set to a live `sleep`, the original check reported running. `running()` now also requires `--mode rpc` in `/proc/<pid>/cmdline`; after a real `podman restart` it reported `stopped` and a fresh start succeeded.
- **Journal growth is linear in pi 1.5.0.** `message_update` carries only `usage` and the delta (the cumulative `partial` is removed on the wire): 136 updates, 34,984 bytes, largest 635 bytes. About 150 bytes of each update is the repeated `usage` object. Each run also journals the roughly 5 KB system message twice (`message_start`, `message_end`) and repeats the run's messages in `agent_end`.
- **`agent_settled` exists in 1.5.0** with an `aborted` field; the 0.79.4 docs on the host do not mention it. `prompt` responses carry `data.disposition` (`started`, `handled`, …); an extension command answers `handled` only after its handler returns.
- **Pending dialogs are derivable from an input log.** With `tee -a input.jsonl` in the attach script, `pending.mjs` lists dialog requests (`select`, `confirm`, `input`, `editor`) in the journal whose ids never appear in an `extension_ui_response` in the input log. A client attached at `end` with no cursor found, answered and cleared a pending `confirm`. Caveat seen in the run: a dialog answered before the input log existed shows as pending forever, so the log must start with the generation. An answer to an already-resolved id is silently ignored.
- **Provisioned model metadata is incomplete.** `get_state` reported `contextWindow: 128000` and `reasoning: false` for `qwen3.5-2b`, because `buildPiLocalLlmProvider` (`src/lib/agents/pi.ts`) writes neither field. The server ran with a 32,768-token context, and the model emitted thinking blocks although `thinkingLevel` was `off`.
- **Extensions under `PI_CODING_AGENT_DIR/extensions/` load without a trust prompt** (`get_commands` lists `spikeconfirm` with `scope: "user"`), so a provisioned extension is a viable path for app-side hooks.

## Concurrent Writers: Inconclusive

Two attaches each sent one 200,000-character `set_session_name` line at the same moment, three times. All six commands succeeded and every `session_info_changed` name was pure `A` or pure `B`; pi's stderr stayed empty. The writes probably never overlapped in time, given roughly 1.4 s of jitter in Podman SSH setup, and POSIX guarantees atomic pipe writes only up to `PIPE_BUF` (4096 bytes here). The result does not show that concurrent writers are safe.

## Not Covered

- Session files on msb reaching the host through Mutagen while the sandbox is stopped.
- The real `ensure_sandbox_alias` flow and the app's Mutagen inference forward; the spike used hand-written equivalents.
- Compaction, auto-retry, steering, follow-up and tool-execution events: the 2B model answered without tool calls, so `traces/01-roundtrip.jsonl` has none of them.

## Blockers Hit

- `podman run -v …:/root/work` from Git Bash failed with `crun: chdir to 'C:/Program Files/Git/root/work': No such file or directory` until `MSYS_NO_PATHCONV=1` was set: MSYS rewrites POSIX-looking arguments.
- Passing a 200 KB JSON line as a command-line argument failed with `Argument list too long`; `driver.mjs` gained `--send-file`.
- The first reattach check failed (`contiguous offsets: false`) because the driver kept recording records after saving its cursor. The driver now stops consuming once it finishes, and the persisted cursor is the end of the last consumed record.
