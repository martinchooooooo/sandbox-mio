---
type: spike
title: "Native Terminal Gating Spike"
feature: native-terminal
---
# Native Terminal Gating Spike

This spike times the current Terminal-tab chain and the proposed `russh`-over-`mio-sshd` chain described in `docs/features/native-terminal/01_ideas.md` (sections Current State and Validation), so `02_dev.md` can be planned on measured numbers. It ran on 2026-11-16 on the Windows 11 development host. All artefacts live in `docs/features/native-terminal/spike/`; no file under `src/`, `src-tauri/`, `e2e/` or `messages/` was changed.

## Headline

- On a warm Podman sandbox, the proposed path reaches the first complete zellij frame in a median 636 ms. The current chain reaches only the first HTTP 200 from the forwarded zellij web client in a median 1,767 ms, and needs a further median 705 ms of headless page and asset fetching before any webview work or websocket attach.
- On a freshly started Podman sandbox, `podman start` plus the proposed path takes a median 1,108 ms (402 ms start, 706 ms to the frame). The current chain takes a median 2,486 ms to the first HTTP 200 and 3,199 ms including the headless page-load estimate.
- The dominant cost in both paths is the engine `exec` round trip through the Windows Podman client into the WSL machine: a bare `podman exec <name> true` has a median 421 ms floor. The current chain pays that floor three to four times serially (start, status, optional daemonize, token) and then adds a Mutagen forward (median 585 to 647 ms). The proposed path pays it once.
- The hypothesis (many serial engine execs, the forward, the page load) is supported. Webview creation, the in-page JavaScript start-up and the zellij web websocket attach could not be measured without the app; the current-chain numbers are therefore lower bounds.
- Window-change works: a `russh` `window-change` request redraws zellij at the new size, and a plain shell reports the new size to `stty`.

## Environment

| Item | Value |
|---|---|
| Date | 2026-11-16 |
| Host | Windows 11 Home 10.0.26200, Git Bash for scripting |
| Podman client | 5.6.2 (the app's bundled `src-tauri/binaries/podman/podman.exe`); the system client is 5.8.2 |
| Podman server | 5.8.6 in machine `podman-machine-default` (WSL, rootless, 4 CPUs, 2 GiB), connection `podman-machine-default` |
| Base image | `localhost/sandbox-mio/base:7` (declared by `BASE_IMAGE` and `BASE_IMAGE_VERSION` in `src-tauri/src/backends/mod.rs`) |
| zellij | 0.44.3 (pinned by `ZELLIJ_VERSION` in `src-tauri/resources/Containerfile`) |
| Mutagen | 0.18.1, the bundled `src-tauri/binaries/mutagen-x86_64-pc-windows-msvc.exe`, with a private data directory |
| OpenSSH client | OpenSSH_for_Windows_9.5p2 (`C:\Windows\System32\OpenSSH\ssh.exe`) |
| microsandbox | `msb 0.7.12` (the bundled `src-tauri/binaries/msb/msb.exe`), base image `sandbox-mio/base:7` already in the local `msb` store |
| `russh` | 0.65.1 with the `ring` crypto backend (see Blockers) |

## Method

Throwaway containers named `spike-native-terminal-warm` and `spike-native-terminal-cold` were created from the base image with the flags `create_instance` in `src-tauri/src/provider/mod.rs` uses (`--tty`, `--network pasta`, workspace volume at `/root/work`, workdir `/root/work`, command `/bin/sh`). A private ed25519 key was generated and its public half copied to `/etc/ssh/authorized_keys.container`, the path `install_sandbox_key` in `src-tauri/src/backends/mod.rs` uses. A private SSH config holds one alias per container whose `ProxyCommand` is the bundled Podman client running `exec -i <name> mio-sshd`, with the user, identity file and known-hosts file pinned as `ensure_sandbox_alias` does. No user sandbox, user SSH config or user Mutagen data was used.

Mutagen resolves `ssh` and `scp` through `MUTAGEN_SSH_PATH`; a one-file shim (`docs/features/native-terminal/spike/ssh-wrapper/`) prepends `-F <private config>` so the private daemon resolves the spike's aliases. The Mutagen daemon ran under a private `MUTAGEN_DATA_DIRECTORY` and was stopped at the end.

**Current chain** (`docs/features/native-terminal/spike/current_chain.mjs`). Each iteration runs, in order and timed individually: `podman start`, `zellij web --status`, `zellij web --daemonize` (only when the status is not online), `zellij web --create-token`, Mutagen forward terminate plus create to in-container port 8082, then polling `GET /<name>` on the forwarded port until HTTP 200. A headless estimate of the page load follows: fetch the HTML's 18 assets in parallel (478,354 bytes), POST the token to `/command/login`, then fetch the HTML again (assets cached). The warm run kept the container running with the web server online; the cold run stopped the container (outside the timing) before every iteration, so `podman start` and `zellij web --daemonize` are paid each time. Seven iterations each. The Mutagen agent install in the container was done once beforehand and is reported separately.

**Proposed path** (`docs/features/native-terminal/spike/ssh-probe/`, a standalone Rust crate outside any workspace). The probe spawns `podman --connection <c> exec -i <name> mio-sshd` (or `msb exec --stream <name> -- mio-sshd`) with piped stdio, wraps child stdout and stdin as one stream, runs `russh` over it, authenticates with the key, requests a 120x40 pty, and runs the mode's remote command. Time zero is just before the spawn. Modes: `true` (SSH cost only), `shell` (login shell; time to a prompt ending in `# `), `zellij` (`zellij attach --create <name>`, session already exists), `zellij-new` (every zellij session is killed and deleted outside the timing first, so attach creates the session), and `stty` (resize proof). Eight iterations each, driven by `docs/features/native-terminal/spike/run_proposed.sh`.

The first-frame heuristic: the first moment at which the received bytes contain both the zellij tab-bar label `Tab #1` and the status-bar text `Ctrl`. The heuristic fires when the bytes carrying those two labels arrive, not when the frame is fully painted; zellij sends the rest of the frame in the same burst, so the error is small but unquantified. The resize check waits 500 ms after the first frame, records the largest cursor-position coordinates in the first frame, sends a `window-change` of 160x50, and records the largest cursor-position coordinates in the redraw. The plain-shell resize check runs `stty size` before and after the same `window-change`.

The OpenSSH baseline is `ssh -F <private config> -tt <alias> true` with the Windows OpenSSH client over the same `ProxyCommand`, eight iterations. All raw output is in `docs/features/native-terminal/spike/results/`; `docs/features/native-terminal/spike/summarize.mjs` produces `docs/features/native-terminal/spike/results/summary.txt` (median, then minimum to maximum, in milliseconds).

## Task 1: Current Chain, Podman

Median (minimum to maximum) milliseconds, seven iterations each.

| Stage | Warm container, web server online | Freshly started container |
|---|---|---|
| `podman start` | 291 (238-315) | 391 (377-437) |
| `zellij web --status` | 426 (382-503) | 477 (387-581) |
| `zellij web --daemonize` | not run | 448 (417-608) |
| `zellij web --create-token` | 381 (321-522) | 416 (367-609) |
| Mutagen forward terminate and create | 585 (505-667) | 647 (560-751) |
| First HTTP 200 on the forwarded port, after the forward exists | 7 (4-33) | 8 (4-30) |
| Whole chain to first HTTP 200 | 1,767 (1,564-1,941) | 2,486 (2,181-2,817) |
| Headless load 1: 18 assets, 478,354 bytes | 593 (564-672) | 574 (531-976) |
| Token login POST | 66 (53-86) | 81 (62-92) |
| Headless load 2: HTML only | 46 (44-48) | 47 (43-52) |
| Whole chain plus headless page load | 2,428 (2,266-2,641) | 3,199 (2,890-3,586) |

Further numbers:

- The first-ever Mutagen forward to a container (which installs the Mutagen agent there over `scp`) took 4,210 ms on the fresh container and 8,707 ms for the first forward after starting the private daemon on the warm container. The cost recurs once per container creation, not per terminal open.
- A bare `podman exec <name> true` has a median 421 ms (387-576, eight iterations); this is the per-exec floor.
- Not measured: `SandboxRouter::resolve` and the alias rewrite under the cross-process lock (`src-tauri/src/services/routing.rs`), WebView2 child-webview creation, the browser's JavaScript start-up and rendering, the zellij web websocket attach and session start. Webview creation and page load cannot be measured without the app; the headless fetch is a floor for network transfer only, because it skips parsing and executing the 18 scripts and does not use a browser cache or HTTP connection reuse the way WebView2 would.

## Task 2: Proposed Path, Podman

Median (minimum to maximum) milliseconds from spawn, eight iterations each.

| Measurement | `true` | plain shell | `zellij attach` (session exists) | `zellij attach` (session created) | `zellij attach` on a freshly started container |
|---|---|---|---|---|---|
| `mio-sshd` banner and key exchange complete | 318 (280-343) | 349 (290-523) | 372 (300-413) | 344 (303-353) | 301 (276-395) |
| Authenticated | 379 (342-406) | 410 (351-731) | 433 (359-475) | 405 (362-415) | 366 (338-477) |
| Pty requested and command sent | 380 (343-407) | 411 (354-757) | 434 (360-476) | 406 (363-416) | 368 (339-500) |
| First output byte | not applicable | 413 (359-796) | 540 (465-591) | 521 (471-556) | 490 (449-653) |
| Shell prompt | not applicable | 440 (396-830) | not applicable | not applicable | not applicable |
| First zellij frame (heuristic) | not applicable | not applicable | 636 (540-722) | 712 (669-742) | 706 (624-854) |
| Command exit | 402 (365-440) | not applicable | not applicable | not applicable | not applicable |

Further numbers:

- Plain OpenSSH `ssh -tt <alias> true` over the same `ProxyCommand`: median 546 ms (502-624). The `russh` `true` session at 402 ms is faster because no `ssh.exe` process is spawned and `cmd.exe` is not involved in the `ProxyCommand`.
- The freshly started container column excludes `podman start`, which took a median 402 ms (369-527) separately; the sum to the first frame is therefore about 1,108 ms.
- Attach cost over SSH alone: a ready authenticated channel with a pty takes a median 380 ms from spawn, which is close to the 421 ms bare-exec floor, so the SSH handshake adds little beyond the engine exec itself. `zellij attach` adds roughly 250 ms (existing session) to 330 ms (new session) from the ready channel to the first frame.
- The plain shell reaches its prompt at a median 440 ms. The login shell sources `/root/.bashrc` (`mise activate bash`), which is included.

### Resize

| Check | Result |
|---|---|
| zellij first frame at pty 120x40 | largest cursor coordinates row 40, column 120 in all 24 zellij iterations |
| zellij redraw after `window-change` to 160x50 | largest cursor coordinates row 50, column 160 in all 24 iterations; first redraw byte after a median 22 ms; about 23.5 KB of redraw output |
| plain shell `stty size` before and after the same `window-change` | `40 120` then `50 160` |

`window-change` sent from the in-process `russh` client resizes the remote pty over plain pipes, and zellij redraws at the new size.

## Task 3: Proposed Path, microsandbox

A throwaway microsandbox `spike-native-terminal-msb` was created from the already-loaded `sandbox-mio/base:7` with `msb create` and a named disk volume (the shape `create_instance` in `src-tauri/src/provider/msb.rs` uses), the public key was written with `msb exec --stream` as `copy_in` does, and the probe ran over `msb exec --stream <name> -- mio-sshd`. Median (minimum to maximum) milliseconds, eight iterations each.

| Measurement | `true` | plain shell | `zellij attach` (warm) | `zellij attach` after `msb stop` and `msb start` |
|---|---|---|---|---|
| Banner and key exchange complete | 66 (57-102) | 68 (45-107) | 61 (53-71) | 114 (85-128) |
| Authenticated | 84 (75-121) | 86 (64-124) | 80 (71-88) | 132 (103-146) |
| First output byte | not applicable | 194 (154-215) | 306 (265-388) | 523 (446-690) |
| Shell prompt | not applicable | 228 (198-248) | not applicable | not applicable |
| First zellij frame (heuristic) | not applicable | not applicable | 403 (340-919) | 985 (910-1,195) |
| Command exit | 218 (167-248) | not applicable | not applicable | not applicable |

Further numbers: a bare `msb exec --stream <name> -- true` has a median 87 ms (73-98); `msb start` of a stopped sandbox has a median 656 ms (542-751). A `window-change` to 160x50 redrew zellij to row 50, column 160 in all 16 zellij iterations, and `stty size` reported `40 120` then `50 160`. The cold column starts a new zellij session because the VM stop ends the old one.

## Not Run

| Item | Status | Reason |
|---|---|---|
| Remote-deploy sandbox (`russh` over the remote machine's `exec`) | not run | Out of scope for the spike by instruction. |
| Renderer fitness in WebView2 (wterm with Ghostty core versus xterm.js: first frame, output flood, IME, paste, mouse mode) | not run | Out of scope for the spike by instruction. |
| WebView2 child-webview creation and page-load timing for the current chain | not run | Needs the running app; the headless fetch is the only estimate. |
| `tracing`-span timing of `prepare_terminal_webview` inside the app | not run | Needs source changes under `src-tauri/`, which the spike may not make. |

## Risks Cleared

- **SSH carried on `podman exec -i ... mio-sshd` stdio by an in-process client.** `russh` completes key exchange and public-key authentication over the child's piped stdout and stdin, with no `ssh.exe` and no `ProxyCommand`. Cleared on Podman and on microsandbox.
- **Pty sizing at attach.** A `pty-req` of 120x40 reaches zellij, which draws a 120x40 frame.
- **Resize.** `window-change` redraws zellij at 160x50 and updates a plain shell's pty size; the OpenSSH-on-pipes limitation recorded in `01_ideas.md` does not apply to the in-process client.
- **Attach without the web server.** `zellij attach --create <sandbox-name>` works with no `zellij web` server, no token and no forward, both when the session exists and when attach must create it.
- **The latency goal is reachable.** The proposed path's warm median to first frame (636 ms) is under the current chain's median to the first HTTP 200 alone (1,767 ms), before webview creation.
- **Engine parity.** The same probe runs unchanged over `msb exec --stream`; microsandbox is faster than Podman per exec (87 ms versus 421 ms).

## Findings For The Plan

- **The per-exec floor of about 420 ms on Podman dominates the proposed path.** Removing further serial execs is worth more than tuning the SSH layer. Resolving the container's running state without a separate `podman start` exec on the warm path would save up to about 290 ms (`podman start` on a running container costs a median 291 ms).
- **The zellij first frame arrives about 100 to 300 ms after the first output byte.** zellij paints a large initial burst (about 23 KB of terminal output per full-screen frame).
- **The Mutagen forward is cheaper than the hypothesis assumed** (a median 585 ms on a warm container, once the agent is installed), but it still accounts for about a third of the current chain before the first HTTP 200.
- **Pre-warming (Q5 in `01_ideas.md`) becomes the next lever.** Opening the SSH session when a sandbox is selected would hide the roughly 400 ms engine exec and handshake, leaving the zellij attach cost of about 250 ms.

## Blockers And Build Notes

| Item | Verbatim error or detail | Resolution |
|---|---|---|
| `russh` with default features does not build on this host | `NASM command not found! Build cannot continue.` (from `aws-lc-sys`, the default `aws-lc-rs` crypto backend, on Windows) | The probe uses `default-features = false` with the `ring`, `flate2` and `rsa` features, which builds with only the Rust toolchain. `02_dev.md` must pick the `ring` feature or require NASM on developer machines and CI. |
| `podman` default connection on this host is an e2e machine | `podman ps` with no `--connection` printed `Error: unable to connect to Podman socket: failed to connect: dial tcp 127.0.0.1:51105: connectex: No connection could be made because the target machine actively refused it.` (the default system connection is `mio-tests-remote`, whose machine is stopped) | Every spike command passes `--connection podman-machine-default`; the e2e machines were not touched. |
| Mutagen cannot use a `.bat` shim as `ssh` | `unable to identify 'ssh' command: unable to locate command` | A compiled `ssh.exe` shim in `docs/features/native-terminal/spike/ssh-wrapper/` replaced the batch file. |
| Mutagen daemon started with a stale environment | `unable to receive server magic number: EOF (error output: thread 'main' (872) panicked at src\main.rs:9:49: SPIKE_SSH_CONFIG: NotPresent` | The Mutagen daemon outlives the CLI call that started it and keeps that call's environment, so the private daemon was restarted with the full private environment exported (`docs/features/native-terminal/spike/env.sh`). |

## Cleanup

Both Podman throwaway containers, the microsandbox sandbox and its volume were removed, and no `forward` session remained in the private Mutagen daemon, which was stopped with `mutagen daemon stop` under the private data directory. Two stray items could not be removed by the spike: one idle Mutagen daemon that a mistaken call (an `env.sh` quoting error left the private variables unset for one `forward terminate`) auto-started in the default `~/.mutagen` data directory, which the sandbox policy refused to let the spike stop; and a stray `C:\c` directory holding build output from one `cargo` call given an MSYS-style target path. Neither contains user data.

## Artefacts

| Path | Purpose |
|---|---|
| `docs/features/native-terminal/spike/env.sh` | Private environment for the other scripts |
| `docs/features/native-terminal/spike/current_chain.mjs` | Current-chain timing harness |
| `docs/features/native-terminal/spike/run_proposed.sh` | Runs the probe modes, cold variants and the OpenSSH baseline |
| `docs/features/native-terminal/spike/summarize.mjs` | Median and spread over `results/` |
| `docs/features/native-terminal/spike/ssh-probe/` | The `russh` probe crate |
| `docs/features/native-terminal/spike/ssh-wrapper/` | The `ssh.exe` and `scp.exe` shim for the private Mutagen daemon |
| `docs/features/native-terminal/spike/results/` | Raw JSON and text output, and `summary.txt` |

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `cargo build` of `russh` fails with `NASM command not found! Build cannot continue.` on Windows | `russh` defaults to the `aws-lc-rs` backend, whose `aws-lc-sys` build needs NASM on Windows | Depend on `russh` with `default-features = false` and the `ring` feature, as `docs/features/native-terminal/spike/ssh-probe/Cargo.toml` does |
| A script's `podman create`, `cp` or `exec` mangles an absolute in-container path | Git Bash rewrites arguments starting with `/` into Windows paths | Export `MSYS_NO_PATHCONV=1` (done in `docs/features/native-terminal/spike/env.sh`) and pass host files to Windows tools as `C:/...` paths |
| A bare `podman ps` fails with connection refused on this host | The default system connection is the e2e machine `mio-tests-remote` | Pass `--connection podman-machine-default` on every call |
| A private Mutagen daemon ignores a changed `SPIKE_SSH_CONFIG` or `MUTAGEN_SSH_PATH` | The daemon keeps the environment of the CLI call that auto-started it | Stop the daemon with `mutagen daemon stop` under the private data directory, export the variables, then start it |
| The `stty` resize probe reports `?` instead of a size | Terminal output separates the size line from escape sequences with a bare carriage return, so splitting on newlines alone misses it | Split on every control character, as `grab_size` in `docs/features/native-terminal/spike/ssh-probe/src/main.rs` does |
