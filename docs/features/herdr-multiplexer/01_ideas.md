---
type: ideas
title: "Herdr Multiplexer Ideas"
feature: herdr-multiplexer
---
# Herdr Multiplexer Ideas

This document is the ideation for replacing zellij with [Herdr](https://herdr.dev) as the multiplexer that runs inside every sandbox and that the native terminal attaches to. The user wants two things Herdr offers: a mouse-first interface (click panes, tabs and workspaces, drag split borders) and awareness of coding agents (each agent pane is classified as working, blocked, done or idle). Nothing in this document is built. A gating spike ran on 2026-11-18 ([spike record](spike/README.md)) and cleared the feature's blocking risks; the wterm half of the mouse and clipboard path is the one thing it could not run. The native terminal (`docs/features/native-terminal/`) is implemented in code and already isolates the multiplexer behind two small seams, so the feature is mostly a base-image change plus two command strings, and the real risk is whether Herdr's mouse interface survives the terminal renderer. The relevant code lives in `src-tauri/src/terminal/` (session host), `src-tauri/resources/Containerfile` (base image), `src/lib/terminal/` (renderer) and `e2e/test/specs/standard/embedded-workspace.spec.ts` (the specs that name zellij).

## Current State

- **The multiplexer is two strings.** `create_argv` and `client_command` in `src-tauri/src/terminal/mod.rs` hold the whole zellij contract: an engine `exec` runs `zellij attach --create-background <sandbox-name>` so the session inherits the container's environment, then the SSH channel runs `zellij attach <sandbox-name>`. `session_already_running` in the same file maps zellij's "Session already exists" failure to success. The session is named after the sandbox, and `WORKDIR` pins the panes' directory to the workspace mount.
- **The session host is multiplexer-neutral otherwise.** `LifecycleHost` (`src-tauri/src/terminal/host.rs`) carries raw bytes over an in-process `russh` session on the engine's streamed exec of `mio-sshd`, forwards resizes as `window-change`, and detaches without ending the multiplexer's session.
- **The renderer is wterm with its Ghostty core**, in `src/lib/terminal/WtermRenderer.svelte`, the only file importing `@wterm/*`. Its mouse-mode forwarding was an unrun Phase D acceptance check of native-terminal (`docs/features/native-terminal/02_dev.md`).
- **zellij is baked into the base image** by `src-tauri/resources/Containerfile`: pinned `ZELLIJ_VERSION`, installed through mise, symlinked onto the global `PATH`, with its default config dumped to a known file. The image is `linux-x64` only. `e2e/test/specs/standard/base-image-recipe.spec.ts` asserts the zellij pin exists.
- **e2e specs read zellij directly.** `liveZellijSessions` in `embedded-workspace.spec.ts` runs `zellij list-sessions` through `podman exec` and asserts exactly one session named after the sandbox, that the session exists only after the Terminal tab opens, and that it survives closing the tab.
- **A host Herdr exists on the development machine** (preview build `0.9.4-preview.2026-10-14`, from `~/.herdr`). It is unrelated to the sandbox and must never be used by the app or its tests.

## What Herdr Is

Read from the Herdr documentation (version 0.9.5) on 2026-11-17 and from `herdr --help` on the host build; none of it has been run inside a sandbox.

- **Persistence model.** Panes run in a background server and a client draws the UI. Detaching the client leaves panes and agents running; running `herdr` again reattaches. Named sessions (`herdr --session <name>`, `herdr session list|attach|stop|delete`) each have their own socket and state. A headless `herdr server` command exists. A reboot or a server stop ends the panes. This matches the zellij properties the native terminal relies on.
- **Mouse.** Click focuses panes, tabs, workspaces and agents; dragging a split border resizes; right-click opens menus; drag-select copies on release; double-click selects a word; Ctrl-click opens links. `ui.mouse_capture` defaults to true.
- **Agent awareness.** Agent state comes from screen-matching rules in downloadable detection manifests, optionally overridden by lifecycle hooks that `herdr integration install <agent>` writes into an agent's own config. A socket API (`herdr agent list|get|read|prompt|wait`) exposes the same state to scripts. The `herdr agent` kind list names pi, claude, codex, opencode and others.
- **Maturity.** The documentation calls the plugin API early and Windows beta. Linux x86_64 and arm64 are listed platforms. The licence is Apache-2.0.
- **Network behaviour.** The default config has `update.version_check` and `update.manifest_check`, both background checks against herdr.dev, and `onboarding`, a first-run notification setup screen.

## Swapping the Multiplexer Behind the Terminal Host

The two command strings in `src-tauri/src/terminal/mod.rs` become Herdr's equivalents; the lifecycle in `host.rs` is unchanged.

- **Server start** stays an engine `exec` rather than an SSH command, for the reason native-terminal Resolved Decision 7 records: `sshd` drops the container's environment, which carries the egress proxy and CA variables, so panes started under an SSH-owned server would lack them. Herdr's server is a long-lived foreground process, so the exec must start it detached and return. The spike confirmed one exec that runs `herdr --session <name> server` under `setsid` with output discarded, in the workspace directory, returns in a median 467 ms and leaves the server running with the container's environment (Q3). A client that attaches to a session with no running server starts one itself with the SSH environment and `/root` as its directory, so the exec must always run first.
- **Client attach** runs the Herdr client for the sandbox's named session over the existing SSH channel with a pty, as zellij's client does today.
- **Session identity** is the sandbox name, as it is for zellij, so a sandbox maps to exactly one Herdr session and e2e specs can count sessions.
- **Detach on pane close** ends the SSH session and exec child only; the Herdr server keeps running.
- **Already-running** handling replaces `session_already_running`. A backgrounded second start exits 0 and discards Herdr's "server is already running" error, so the start exec checks first: `herdr --session <name> status server` prints `status: running` or `status: not running`, both exit 0, and `herdr session list --json` lists each session's `running` flag.

## Base Image

The Containerfile swaps the zellij block for a Herdr block: a pinned version, a binary on the global `PATH`, and a baked `config.toml` in place of the dumped zellij config.

- The baked config sets `default_shell` to bash and `new_cwd` to the workspace so panes read `.bashrc` (which activates mise) and start beside `mise.toml`, mirroring the `SHELL` and `WORKDIR` settings the zellij block depends on.
- It sets `onboarding` to false and disables `version_check` and `manifest_check`. A sandbox's traffic goes through the egress proxy, so unconfigured background checks would appear in the traffic viewer as unexplained herdr.dev requests, or be blocked and retried when the policy denies them.
- Shipping a new Containerfile recipe is a base-image version bump (`docs/architecture/overview.md`). Sandboxes created before the bump keep the old image: they have zellij and no Herdr, which the terminal host must handle (Q6).
- `base-image-recipe.spec.ts` changes its pin assertion from zellij to Herdr, and the "mise + zellij" size comments in the base-image build specs go stale.

## Mouse and Rendering Through wterm

Herdr's reason for being chosen only pays off if mouse reports reach it. Herdr enables xterm mouse reporting on the pty; wterm (Ghostty core) must encode clicks, drags, releases and wheel events in the reporting format Herdr requests and write them as input bytes.

- Today wterm's mouse-mode path is an unrun acceptance check, so the feature's first spike is the mouse path (see Validation).
- Drag-select copy in Herdr only helps if the copied text reaches the host clipboard. Whether Herdr uses an OSC 52 clipboard escape and whether wterm honours it is unknown (Q5). Without it, Herdr selection copies nowhere visible, and the browser's native selection no longer applies while the TUI captures the mouse.
- The renderer interface from native-terminal keeps xterm.js as the fallback; xterm.js's mouse and OSC 52 behaviour with TUIs is known from the VS Code terminal.

## Technology Choices With Tradeoffs

### Multiplexer: Herdr replaces zellij (chosen)

- **Chosen: Herdr, with zellij removed from the image.** It supplies the two requested capabilities in one tool, and the native terminal's seams make the swap small.
- **Rejected: keep zellij and add Herdr behind a setting.** It doubles the terminal's e2e surface and the image's size, and native-terminal Resolved Decision 3 already rejected a fallback setting for the same reason.
- **Rejected: keep zellij and add a custom plugin for agent state.** zellij has mouse support, but the user asked for Herdr's agent state detection, and zellij has no equivalent without writing and maintaining a plugin.
- **Rejected: tmux.** Same objection native-terminal raised: it abandons existing sessions and has neither a mouse-first interface nor agent awareness.

### Which Herdr front end: the TUI in the existing pane (chosen)

- **Chosen: the Herdr TUI, run by the existing `russh` session and rendered by wterm.** Everything native-terminal built is reused.
- **Rejected: the separate native Herdr app that herdr.dev also describes.** Its own documentation says it keeps a config and sessions apart from the TUI, and it is a native window: embedding it hits the same two-event-loop and WebdriverIO-visibility costs native-terminal rejected for GPUI.
- **Rejected: `herdr --remote <ssh-target>` from the host.** It needs a host Herdr binary (the Windows build is beta), an SSH alias in the host-global `~/.ssh` config and a TTY, and it bypasses the engine-exec transport that keeps the terminal per-worktree and mockable.
- **Rejected: `herdr terminal attach` or `herdr agent attach`.** They attach a single terminal or agent, not the workspace, tab and pane layout the user wants, and the documentation marks them as not available on native Windows.

### Where Herdr comes from in the image: the pinned release asset (chosen by spike)

- **Chosen: the pinned GitHub release asset**, `herdr-linux-x86_64` at v0.9.5, downloaded with curl and verified against its sha256, as mise itself is installed. The spike built it into a throwaway image: about 30 MB added. The cost is owning the asset name, version and checksum as build arguments; the release publishes no checksum file, so the digest comes from the GitHub release API.
- **Rejected: mise with an exact version**, as zellij is installed today. The registry maps `herdr` to `github:ogulcancelik/herdr` and requests the exact release URL without the rate-limited releases-list API, but the install fails with the image's mise: "GitHub artifact attestations verification error ... TSA timestamp verification failed". Turning attestation checks off drops a supply-chain check, and a newer mise changes an unrelated pin; revisit when mise is next bumped.
- **Rejected: the `install.sh` script.** It floats to the latest release at build time, which breaks the exact-pin rule `base-image-recipe.spec.ts` enforces.

### Session creation: server through the engine exec (chosen)

- **Chosen: start the named session's server with an engine exec, attach with SSH.** Keeps the container environment in every pane.
- **Rejected: start it as the SSH client command (attach creates the server).** The server inherits `sshd`'s stripped environment, the exact failure Resolved Decision 7 of native-terminal fixed for zellij.
- **Rejected: start the server at container boot from the Containerfile entrypoint.** Podman only: microsandbox boots its own guest init and ignores the entrypoint (comment in `src-tauri/resources/Containerfile`).

### Host-global state

None. Herdr runs inside each sandbox, so its sockets and sessions are per sandbox and per worktree, and the feature's specs stay in the `standard` tier with the existing terminal specs. The host's own `~/.herdr` install and its API socket are out of the picture; no test may touch them.

## Validation

A gating spike ran on 2026-11-18 on the Windows 11 development host against throwaway Podman containers built from the base image (`localhost/sandbox-mio/base:7`) plus Herdr 0.9.5. The record, probe, scripts and raw captures are in `docs/features/herdr-multiplexer/spike/` ([spike record](spike/README.md)). The probe is the native-terminal `ssh-probe` adapted to run the Herdr client. It ran against Podman only; microsandbox, a deployed sandbox and the Tauri webview were not run.

Cleared risks:

- **Detached server start gates the feature, and it works.** One `podman exec` starts a named session's server under `setsid`, returns in a median 467 ms (418 to 585, six runs), and leaves the server running. Panes inherit the container's environment (a proxy variable set on the container appeared in a pane) and the exec's working directory.
- **Attach, resize and persistence work.** `herdr --session <name>` over a pty on `podman exec -i <name> mio-sshd` draws the UI, a `window-change` to 160x50 redraws to row 50 and reflows, and after killing the exec child the server, layout, focus and a running `sleep` all survived and reattached.
- **Mouse works with raw bytes.** Herdr enables mouse modes 1000, 1002, 1003, 1015 and 1006 (SGR), plus bracketed paste (2004), focus reports (1004) and synchronized output (2026). SGR clicks moved focus between panes both ways, and a drag from a split border moved the split ratio from 0.5 to 0.628.
- **Clipboard works on Herdr's side.** Drag-select and double-click each emit an OSC 52 sequence with selector `c`, BEL-terminated, carrying the selected text.
- **The wterm packages support what Herdr uses, by reading them.** `@wterm/ghostty` and `@wterm/dom` document and code every mouse mode and encoding Herdr enables, and an OSC 52 write with selector `c` reaches `onClipboardWrite`, which `src/lib/terminal/TerminalView.svelte` already passes to `navigator.clipboard.writeText`.
- **The baked config suppresses first-run behaviour.** With defaults, Herdr checks for updates at server start, fetches its agent-detection manifest from herdr.dev over TLS and shows the onboarding box. With `onboarding = false` and both checks off there were no requests and no first-run screen. `default_shell` and `new_cwd` belong in the `[terminal]` table; panes were bash, with mise activated, in `/root/work`, even when the exec's directory was `/tmp`.
- **Install route B works** (see Technology Choices); route A fails on attestation.
- **Cost against zellij, same harness:** first complete frame is a median 743 ms (669 to 903, eight runs) against 635 ms for zellij, about 108 ms slower, though Herdr's first byte arrives sooner (485 against 610 ms) because it repaints several times. Idle memory is lower: Herdr server 22 MB and client 14 MB against a 79 MB zellij server (different pane counts, so indicative). Idle CPU about 0.3 to 0.4 percent.

Blockers surfaced:

- **mise attestation failure**, verbatim: "GitHub artifact attestations verification error for github:herdrdev/herdr@0.9.5: Verification failed: Sigstore error: Verification error: TSA timestamp verification failed: Failed to verify timestamp signature: no certificate matches issuer and serial number". Sidestepped by route B.
- **Session names over 61 characters fail** under `/root`, verbatim: "local socket name length exceeds capacity of sun_path of sockaddr_un". Herdr states a 64-byte cap (`session name cannot be longer than 64 bytes`) but the socket path under the sessions directory leaves 61. Sandbox names have no length check in `validate_name` (`src-tauri/src/backends/mod.rs`).

Not validated:

- **wterm in WebView2.** Real mouse input, drag-select, the clipboard write and Herdr's frame under wterm's renderer were not run; only the package code was read. First frame in the app is also unmeasured.
- **microsandbox** and a **deployed sandbox** on the remote test machine.
- **Herdr's agent detection** against the agents the app installs; the spike did not launch an agent.
- **A multi-pane workflow over a long session**, including Herdr's scrollback and memory growth.

## Open Questions

- **Q1: Replace or keep zellij.** Recommendation: replace, no setting, matching native-terminal Resolved Decision 3. The spike found nothing that argues for keeping zellij.
- **Q2: Install route and version pin.** Route A (mise) or B (release asset); which exact version, given the documentation is at 0.9.5, the host runs a preview, and Herdr is young. Spike finding: route B at v0.9.5 stable (asset `herdr-linux-x86_64`, sha256 `14d9e8bc8b4a9e0118d77c32266f76d461f09b40342eb23752960bd89f59929d`); route A fails on attestation with the image's mise. Open for the dev plan: whether a later mise bump should move the install to route A.
- **Q3: Detached server start.** Whether `herdr server` can be started under a session name through a single exec that returns, and how an already-running session is reported. Spike finding: yes. One exec under `setsid` returns in a median 467 ms, and the idempotent form checks `status server` first. Open for the dev plan: the exact argv.
- **Q4: Session naming.** Whether every sandbox name is a legal Herdr session name; the legal character set is undocumented in what was read. Also whether the default session or a named one is used. Recommendation: a named session per sandbox. Spike finding: the character sets match (letters, digits, `.`, `_`, `-`), but the real length cap is 61, not the stated 64, because of the socket path; the dev plan needs a shortening rule (for example a truncated name plus a hash suffix), and the e2e specs must find the session by that rule.
- **Q5: Mouse and clipboard through wterm.** Whether wterm forwards mouse reports and honours OSC 52. If either fails: fix in `WtermRenderer.svelte`, or move to xterm.js behind the same interface. Spike finding: the wterm packages code both and Herdr emits OSC 52 on selection, so the expected outcome is working, but WebView2 was not run; the check stays an acceptance check in the dev plan's terminal phase.
- **Q6: Sandboxes created before the image bump.** Whether the terminal host detects a missing Herdr binary and shows a recreate prompt, falls back to zellij for old sandboxes, or the bump recreates them. Recommendation: detect and prompt, since a fallback keeps zellij's code path alive.
- **Q7: Prefix key and detach binding.** Herdr's default prefix is Ctrl+B and `prefix+q` detaches, which ends the SSH session and puts the pane in its "ended" state with Reconnect. Recommendation: keep the defaults, and bind nothing app-side, as native-terminal Resolved Decision 4 says the focused terminal receives every key.
- **Q9: First-frame cost.** Herdr's first complete frame is about 108 ms slower than zellij's in the spike. Whether that is acceptable is a user call; the dev plan can measure it in the app first. Recommendation: accept, since the mouse and agent features are the reason for the swap and the native terminal stays well ahead of the web terminal it replaced.
- **Q8: Agent state in the app.** Herdr already shows agent state inside its own sidebar. Whether the app reads it too is deferred in [04_improvements.md](04_improvements.md); the build only needs the TUI.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Clicks and drags in the pane do nothing | The renderer does not encode mouse reports, or Herdr's `ui.mouse_capture` is off | Verify wterm's mouse-mode path in the spike; keep `ui.mouse_capture` at its default in the baked config |
| Herdr shows a first-run notification setup screen on first attach | `onboarding` defaults to true when the key is missing | Set `onboarding = false` in the baked `config.toml` |
| Unexplained herdr.dev requests appear in the traffic viewer, or fail under a strict egress policy | `update.version_check` and `update.manifest_check` default to true | Disable both in the baked config |
| Panes lack the proxy and CA environment variables | The server was started over SSH, whose `sshd` drops the container environment | Start the server through the engine exec, as native-terminal Resolved Decision 7 requires |
| Panes open `sh` or in the wrong directory, so mise tools are missing | Herdr's `default_shell` falls back to `$SHELL` then `/bin/sh`, and `new_cwd` defaults to following the source pane | Set `default_shell` and `new_cwd` in the baked config |
| The host's own Herdr session answers a `herdr` command a spec runs | A `herdr` run on the host talks to the API socket under `%APPDATA%\herdr` | Run Herdr commands only through `podman exec` or `msb exec` inside the sandbox |
| An old sandbox's Terminal tab fails to attach | The sandbox predates the base-image bump and has no Herdr binary | Detect a missing binary and prompt to recreate (Q6) |
| `base-image-recipe.spec.ts` fails after the swap | It asserts a pinned zellij version in every Containerfile | Change the assertion to the Herdr pin in the same change |
| The spec that counts sessions finds none | `liveZellijSessions` calls `zellij list-sessions` | Read `herdr session list --json` inside the sandbox instead |
| Every pane's process dies | `herdr server stop` or a container stop ends the panes; only detach is non-destructive | Never call the stop commands on pane close; close only the SSH session |
| A sandbox with a long name will not start its terminal | Herdr's session socket path under `/root` leaves 61 characters for the name (`local socket name length exceeds capacity of sun_path of sockaddr_un`), and `validate_name` sets no length limit | Derive the session name through a shortening rule with a hash suffix for names over 61 characters |
| Panes show `/root` and no proxy variables | The client attached first and started a server itself with the SSH environment | Always run the start exec before attaching; never let the client be the first thing to touch a session |
| A second start exec looks successful but reports nothing | A backgrounded `herdr server` that finds a running server exits 0 and writes its error to discarded output | Check `herdr --session <name> status server` before starting; both of its answers exit 0 |
| The base-image build fails installing Herdr through mise | mise's artifact attestation check fails on this release (`TSA timestamp verification failed`) | Download the pinned release asset and verify its sha256; do not disable attestation checks |
| Herdr's `default_shell` and `new_cwd` settings have no effect | They belong in the `[terminal]` table, not the top level of the config | Put them under `[terminal]` in the baked `config.toml` |
