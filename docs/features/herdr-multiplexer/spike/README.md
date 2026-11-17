---
type: spike
title: "Herdr Spike"
feature: herdr-multiplexer
---
# Herdr Spike

This spike answers the gating questions in `docs/features/herdr-multiplexer/01_ideas.md` (sections Validation and Open Questions). It ran on 2026-11-18 on the Windows 11 development host, against Podman only. All artefacts live in `docs/features/herdr-multiplexer/spike/`; no file outside `docs/features/herdr-multiplexer/` changed, and `src-tauri/resources/Containerfile` was not edited. Herdr ran only inside throwaway containers named `herdrspike-*` (all removed, with the image `herdrspike-img`); the host's own Herdr was not used.

## Headline

- The feature is not blocked. A named session's server starts through one `podman exec` that returns in a median 467 ms and leaves the server running with the container's environment.
- Install route B (pinned GitHub release asset, v0.9.5) works. Route A (mise) resolves the tool without the releases-list API but fails on attestation verification with the image's mise.
- The Herdr client attaches over `mio-sshd` with the probe, draws the UI, resizes on `window-change`, reacts to raw SGR mouse reports (focus and split-border drag), and emits OSC 52 on drag-select and double-click.
- Time to first complete frame is a median 743 ms, against 635 ms for zellij measured in the same harness (about 108 ms slower). Idle memory is lower: Herdr server 22 MB against zellij server 79 MB.
- The wterm half of the mouse and clipboard questions was not run. Reading the installed `@wterm` packages shows documented and coded support for every mouse mode and encoding Herdr enables, and for OSC 52 with selector `c`.
- Two new findings need decisions: a session name is limited to 61 characters under `/root` (a socket path limit), and a Herdr client started without a running server starts one itself with a stripped environment.

## Environment

| Item | Value |
|---|---|
| Date | 2026-11-18 |
| Podman client for the probe | bundled `src-tauri/binaries/podman/podman.exe` 5.6.2, connection `podman-machine-default` |
| Podman machine | `podman-machine-default` was stopped; started with `podman machine start` |
| Base image | `localhost/sandbox-mio/base:7`, 345 MB |
| Spike image | `localhost/herdrspike-img:1`, 375 MB (delta about 30 MB, the 30,285,672-byte binary) |
| Herdr | 0.9.5 stable (tag v0.9.5, published 2026-10-14, not a pre-release), linux x86_64 |
| zellij (comparison) | 0.44.3 from the base image |
| Probe | `herdr-probe/`, russh 0.65.1 with default features off and `ring`, `flate2`, `rsa` on |
| Screen rendering of captured bytes | `render.py` using the Python package pyte, with terminal queries stripped |

## Step 1: Install

Verdict: cleared by route B; route A blocked.

- **Route A, mise.** `mise registry` maps `herdr` to `github:ogulcancelik/herdr`, and the install requests the exact URL `https://github.com/herdrdev/herdr/releases/download/v0.9.5/herdr-linux-x86_64` with a HEAD request. It does not list releases through the GitHub API; the only other request was a POST to `mise-versions.jdx.dev/api/tools/herdr`. The install then fails at the attestation step with mise 2026.6.14, the version baked in the image. The same failure occurs with `github:herdrdev/herdr@0.9.5`.
- **Verbatim error:** `Failed to install github:herdrdev/herdr@0.9.5: GitHub artifact attestations verification error for github:herdrdev/herdr@0.9.5: Verification failed: Sigstore error: Verification error: TSA timestamp verification failed: Failed to verify timestamp signature: no certificate matches issuer and serial number`.
- A mise setting that turns attestation verification off was not tried: switching off a supply-chain check is outside this spike's remit (the permission system also refused the attempt). A newer mise (2026.11.9 was offered) may pass; that was not tried either, because it changes the image's mise pin.
- **Route B, pinned release asset.** Asset `herdr-linux-x86_64`, 30,285,672 bytes. The release publishes no checksum file; the GitHub release API exposes a per-asset digest, `sha256:14d9e8bc8b4a9e0118d77c32266f76d461f09b40342eb23752960bd89f59929d`, which the build checked with `sha256sum -c` (`OK`). The recipe is in `Containerfile.spike`.
- **Recommended pin:** 0.9.5 stable by route B, with the asset name, version and sha256 as build arguments. Other assets in the release: `herdr-linux-aarch64`, `herdr-macos-aarch64`, `herdr-macos-x86_64`, `herdr-windows-x86_64.zip`.

## Step 2: Detached server start

Verdict: cleared (gates the feature).

- **Server start that works:** `herdr --session <name> server` run from a shell with `setsid`, output to `/dev/null`, stdin from `/dev/null`, and `&`, inside one `podman exec -w /root/work`. The exec returns at once (median 467 ms, range 418 to 585, six runs; the idempotent form is `start_server.sh`), and the server (a child of PID 1) keeps running after the exec ends.
- `podman exec -d ... herdr --session <name> server` also works, but leaves no way to read an error.
- A plain background `&` in `sh -c` without `setsid` also survived the exec ending, so `setsid` is belt and braces.
- **Environment:** a variable and a proxy variable set on the container showed up inside a pane of the exec-started server (`SPIKE_MARK=from-container-env`, `HTTPS_PROXY=http://proxy.invalid:1`).
- **Already running:** run in the foreground, a second `herdr --session <name> server` prints `error: herdr server is already running` then `api socket: /root/.config/herdr/sessions/s-one/herdr.sock` and exits 1. The backgrounded form always exits 0 and writes that error to its discarded output, so an idempotent single exec checks first: `herdr --session <name> status server` prints `status: running` (exit 0) or `status: not running` (exit 0). `start_server.sh` combines both; run twice per session over six sessions, it left exactly one server each (second-call median 502 ms).
- **`herdr session list --json`** shows the session: an object with a `sessions` array whose entries have `default`, `name`, `running`, `session_dir` and `socket_path`. A `default` session always appears, with `running` false. Without `--json` the output is a table with name, status, directory and socket.
- **Headless start creates no workspace.** `workspace list` and `pane list` return empty arrays until a client attaches; the first attach creates one workspace and one pane whose directory is the exec's working directory (`/root/work`) and whose environment is the server's.
- **Session names (Q4):** the legal set is ASCII letters, digits, `.`, `_` and `-`, which equals the character set `validate_name` in `src-tauri/src/backends/mod.rs` allows (the sandbox side additionally forbids a leading `.`, `_` or `-`). `My-box_1.x`, `a.b-c_9` and `9lives` all started. Verbatim rejections: `error: session name may only contain ASCII letters, numbers, '.', '_' and '-'` (for `a/b` and `a b`) and `error: session name cannot be . or ..`.
- **New limit.** `error: session name cannot be longer than 64 bytes` is the stated cap, but with `HOME=/root` the real cap is 61: names of 62 to 64 characters fail at start with `Error: Custom { kind: InvalidInput, error: "local socket name length exceeds capacity of sun_path of sockaddr_un" }`. Sandbox names have no length validation in `validate_name`.

## Step 3: Attach

Verdict: cleared.

- **Client command:** `herdr --session <name>` over the pty channel (`herdr session attach <name>` was not needed). The probe requests a 120x40 pty with `xterm-256color`.
- **First frame capture:** `results/first_frame.bin` (15,469 bytes in 3 s). The UI is a 26-column sidebar (workspaces, agents) on the left, a tab bar on row 1, and panes with box borders. The stream starts by resetting mouse modes, enters the alternate screen (`ESC [ ? 1049 h`), enables the modes listed under Step 4, and then queries the terminal: OSC 10 and OSC 11 colours, `ESC [ ? 996 n`, `ESC [ 16 t`, kitty keyboard negotiation, `ESC [ c` and DECRQM 1016. The probe answers none of these queries; whether Herdr stalls waiting for answers was not observed (the frame drew anyway).
- **Time to first complete frame**, from spawn, eight warm runs each (marker: pane text `root@` for Herdr, which arrives after the sidebar; `Tab #1` for zellij):

| Measurement | Herdr median (min-max) ms | zellij median (min-max) ms |
|---|---|---|
| Command sent after key exchange and auth | 460 (383-609) | not recorded |
| First output byte | 485 (414-635) | 610 (500-664) |
| First complete frame | 743 (669-903) | 635 (524-714) |
| Bare `podman exec <name> true` | 485 (418-549) | same run |

  The zellij figure matches the 636 ms in `docs/features/native-terminal/spike/README.md`. Herdr's first byte arrives sooner, but its frame is built over several repaints; the first synchronized-output block closes before the sidebar is painted.
- **Resize:** after a `window-change` to 160x50 the redraw addresses row 50 (the 120x40 frame's largest cursor row is 40), and the pane text reflows to the wider pane (`results/resize.bin`, offsets in `results/resize.json`). Cleared. Herdr's diff repaint did not address column 160, so the column proof is the reflow.
- **Client-started server (important):** attaching to a session whose server is not running starts one (first frame in 1,014 ms) but with the SSH environment: a pane in it showed `MARK=[]`, `PX=[]` and directory `/root`, where the exec-started server showed the container's variables and `/root/work`. This is the failure native-terminal Resolved Decision 7 records for zellij, so the engine-exec start stays required.

## Step 4: Mouse

Verdict: raw-byte half cleared; wterm-in-webview half not run.

- **Modes Herdr enables** (after resetting them all first): DECSET 1000, 1002, 1003 (any-motion), 1015 (urxvt) and 1006 (SGR), plus 2004 (bracketed paste), 1004 (focus reports), 2031 and 2026 (synchronized output). It leaves 1005 and 1016 reset.
- **Click focus.** With two panes split right, `pane list` before: `w1:p1` focused. An SGR press and release at column 95, row 10 (inside pane 2) gave `w1:p2` focused; the same at column 40 returned focus to `w1:p1`.
- **Border drag.** `pane layout` split ratio before: 0.5. An SGR press at column 73 (the border), five button-held motion reports (code 32) to column 85, and a release gave ratio 0.62765956 (widths 75 and 45 of 120 in the headless layout). A second drag from column 74 changed nothing because the border had already moved; this is expected, not a failure. Raw bytes: `results/click.bin`, `results/click2.bin`, `results/drag_73.bin`, `results/drag_74.bin`; helper `sgr.sh`.
- **wterm in the Tauri webview: not run.** The app was not started. Code and docs evidence only: the `@wterm/ghostty` README states that the core exposes mouse tracking modes 1000, 1002 and 1003, and that the DOM layer supports X10, UTF-8 (1005), SGR (1006), urxvt (1015) and SGR-pixel (1016) reports. `node_modules/@wterm/dom/dist/input.js` (`handleMouse`) encodes press, move, release and wheel from mouse events when tracking is on, reports motion for 1003 and for 1002 with a button held, and returns without reporting when Shift is held (so Shift-drag falls back to native selection) or when the pointer is outside the cell grid. `src/lib/terminal/WtermRenderer.svelte` does not touch mouse handling; it only forwards `onClipboardWrite`. Whether the DOM mouse listeners behave correctly inside WebView2 over a running Herdr is what remains unrun.

## Step 5: Clipboard

Verdict: Herdr side cleared; wterm side by code reading only.

- A drag-select with SGR bytes in pane 1 (press, eight held-motion reports, release) made Herdr emit an OSC 52 sequence with selector `c`, terminated by BEL, whose decoded payload was the selected text. A double-click on a word emitted a second one. `osc52.py` lists them from `results/select.bin`.
- `ui.copy_on_select` defaults to true, which is why release copies; no clipboard tool (`xclip`, `xsel`, `wl-copy`) exists in the image, so OSC 52 is the only route.
- `@wterm/ghostty` README: OSC 52 writes reach `getClipboardWrite()` and `onClipboardWrite(text)`; the default selector and `c` are supported, base64 UTF-8 up to 65,536 decoded bytes; queries and other selectors are ignored; the host decides whether to accept. The bundled wasm exports `get_clipboard_write_len` and `get_clipboard_write_ptr`. `src/lib/terminal/TerminalView.svelte` already passes `onClipboardWrite` to `navigator.clipboard.writeText`. Whether WebView2 permits that write without a user gesture was not run.

## Step 6: Persistence

Verdict: cleared.

- With two panes, a counter loop in pane 1 and `sleep 600` in pane 2, the probe attached and then killed the exec child with no SSH disconnect. Three seconds later the server was still running, the split ratio and focus were unchanged, and the `sleep 600` process kept its PID (1243). No `herdr` client process lingered in the container.
- A reattach (`results/persist2.bin`) redrew both panes with the same content, including the counter's earlier output, and the counter continued (`tick-234` read later).

## Step 7: Resources

Verdict: cleared.

| Measure | Value |
|---|---|
| Herdr server RSS, idle, 2 panes plus a one-line-per-second loop | 22.3 MB without a client, 22.5 MB with one |
| Herdr client RSS, attached | 14.1 MB |
| Herdr server CPU over a 10 s window | 3 clock ticks without a client, 4 with one (about 0.3 to 0.4 percent) |
| Herdr client CPU over 10 s | 4 ticks |
| zellij server RSS, one session, one pane | 79.5 MB, 1 tick in 10 s |

Herdr had two panes with a running loop and zellij one idle pane, so the memory comparison is not like for like; it still shows Herdr's server far below zellij's.

## Baked config and network checks

Verdict: cleared.

- **Defaults fire.** In a container with no `config.toml` and `herdr.dev` mapped to a local listener, the server logged `checking for updates` (`update.check.start`) at start, then `agent detection manifest update failed: failed to fetch https://herdr.dev/agent-detection/index.toml`, then `update check failed` (`failed to fetch update manifest`). The listener recorded TLS connection attempts naming `herdr.dev` (7 occurrences in 3,619 bytes).
- **Baked config suppresses them.** With `config.toml` setting `onboarding = false`, `update.version_check = false` and `update.manifest_check = false`, a container watched for over a minute logged no update or manifest line and the listener received no connection.
- **Onboarding.** Without the config the first attach shows the welcome box (`this is a mouse-first terminal.`, `ctrl+b enters prefix mode`); with it the first attach shows no such box (`results/onboard_dflt.bin`, `results/onboard_baked.bin`).
- **Shell and directory.** The keys sit in the `[terminal]` table: `default_shell = "/bin/bash"` and `new_cwd = "/root/work"`. Panes ran `/bin/bash`, mise showed as a shell function (activated through `.bashrc`), and a workspace created with the exec working directory `/tmp` still opened in `/root/work`. Without the config the same workspace opened in `/root`.

## Risks cleared

- Detached named-session start through one engine exec, with the container environment and idempotence.
- Session names: the sandbox name charset equals Herdr's.
- Client attach, resize, mouse focus, border drag, OSC 52, and persistence over the in-process russh path.
- Network checks suppressed; first-run screen suppressed.

## Blockers and new risks

| Item | Verbatim text or detail | Resolution |
|---|---|---|
| Route A install | `Failed to install github:herdrdev/herdr@0.9.5: GitHub artifact attestations verification error for github:herdrdev/herdr@0.9.5: Verification failed: Sigstore error: Verification error: TSA timestamp verification failed: Failed to verify timestamp signature: no certificate matches issuer and serial number` | Use route B. Retest route A only with a newer mise, in a separate base-image change. |
| Session name length | `Error: Custom { kind: InvalidInput, error: "local socket name length exceeds capacity of sun_path of sockaddr_un" }` for names of 62 to 64 characters; `error: session name cannot be longer than 64 bytes` above 64 | Q4 needs a rule: map a sandbox name longer than 61 characters to a shortened, hashed session name. |
| Client without server | A client started over SSH with no server starts one with `MARK=[]` and `PWD=/root` | Always start the server through the engine exec first. |
| First frame slower than zellij | 743 ms against 635 ms median | Within budget; the pre-warm lever in the native-terminal spike applies equally. |

## Not run

| Item | Reason |
|---|---|
| wterm in the Tauri webview (mouse reports, OSC 52 to the clipboard) | The app cannot be started headlessly; code reading only |
| microsandbox engine | Out of scope for this spike |
| Route A with a newer mise | Would change the image's mise pin |

## Artefacts

| Path | Purpose |
|---|---|
| `docs/features/herdr-multiplexer/spike/Containerfile.spike`, `config.toml` | Throwaway image recipe (route B) and the baked config |
| `docs/features/herdr-multiplexer/spike/herdr-probe/` | russh probe adapted from the native-terminal `ssh-probe`; runs one command over a pty and plays a byte script |
| `docs/features/herdr-multiplexer/spike/start_server.sh` | Idempotent single-exec server start |
| `docs/features/herdr-multiplexer/spike/sgr.sh`, `render.py`, `cupmax.py`, `osc52.py`, `summarize.py`, `idle.sh` | Helpers for mouse bytes, screen rendering, cursor extent, OSC 52 listing, medians and idle sampling |
| `docs/features/herdr-multiplexer/spike/results/` | Raw byte dumps, timing files and idle samples |

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `herdr server` blocks the exec | It runs in the foreground | Start it with `setsid`, redirected output and `&`, in one shell |
| A second start looks successful | The backgrounded form exits 0 and discards the error | Check `herdr --session <name> status server` first |
| A pane lacks the proxy variables or the workspace directory | The client over SSH started the server | Start the server through the engine exec |
| A long sandbox name fails to start a session | The socket path under `/root/.config/herdr/sessions/<name>/` exceeds `sun_path` above 61 characters | Shorten the session name |
| `pyte` raises on a captured stream | It rejects terminal queries such as the private DSR | `render.py` strips them first |
| mise cannot install Herdr | Attestation verification fails in mise 2026.6.14 | Use the pinned release asset |
