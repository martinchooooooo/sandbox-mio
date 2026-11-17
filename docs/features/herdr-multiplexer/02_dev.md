---
type: dev-plan
title: "Herdr Multiplexer Dev Plan"
feature: herdr-multiplexer
---
# Herdr Multiplexer Dev Plan

This document is the build plan for replacing zellij with Herdr as the multiplexer inside every sandbox, continuing [01_ideas.md](01_ideas.md) and its [spike record](spike/README.md). The work touches the base image (`src-tauri/resources/Containerfile`, `BASE_IMAGE_VERSION` in `src-tauri/src/backends/mod.rs`), the terminal host (`src-tauri/src/terminal/`), the e2e specs that name zellij (`e2e/test/specs/standard/`) and the documents that describe the base image. Status: all phases are implemented. Phases A and B were hand-verified against Podman, the e2e specs of Phase C are updated and passing, and the user ran the manual renderer checks of Phase D. The native-terminal feature (`docs/features/native-terminal/02_dev.md`) is still `in-progress` (its Phase F is open), so this plan assumes its implemented phases A to E and does not touch its remaining one.

## The Concrete Model

The multiplexer stays two strings in `src-tauri/src/terminal/mod.rs`; the SSH lifecycle in `src-tauri/src/terminal/host.rs` is unchanged.

- **One Herdr session per sandbox.** The Herdr session name is derived from the sandbox name by `session_name()` (new, `src-tauri/src/terminal/mod.rs`): names of 61 characters or fewer pass through unchanged, longer names become the first 52 characters, a hyphen and the first 8 hex characters of the name's SHA-256 (61 characters in total). `sha2` is already a dependency in `src-tauri/Cargo.toml`. `AttachRequest.name` keeps meaning the sandbox; only the two command builders take the derived session name.
- **Server start is an engine exec, never an SSH command.** `create_argv()` becomes a `sh -c` argv with the session name as a positional argument. The script it runs, in order: exit 127 when `herdr` is not on `PATH`; exit 0 when `herdr --session <name> status server` reports `status: running`; otherwise start `herdr --session <name> server` under `setsid` with stdin from `/dev/null` and output discarded, in the background; then poll `status server` every 100 ms for up to 5 seconds and exit 0 on `running`, exit 1 on timeout. The exec keeps `workdir: WORKDIR`. The check-then-start order is foreground on purpose: the spike's `spike/start_server.sh` backgrounds the whole check-and-start list, so the exec returns before the server exists, and an SSH client arriving first would start its own server with the stripped SSH environment.
- **Client attach runs `herdr --session <name>`** over the existing SSH pty channel, built by `client_command()` with `shell_quote()` as today.
- **Missing binary is its own error.** Exit code 127 from the start exec becomes `TerminalError::MultiplexerMissing` (new, `src-tauri/src/terminal/types.rs`), whose message tells the user the sandbox predates the Herdr terminal and must be recreated from the project list. It converts into `AppError::Terminal` like every other variant, so the Terminal view shows the message with its existing Retry and no new `AppErrorKind` is needed.
- **Image.** The Containerfile replaces the zellij block with a pinned Herdr release asset (`herdr-linux-x86_64`, v0.9.5, sha256 `14d9e8bc8b4a9e0118d77c32266f76d461f09b40342eb23752960bd89f59929d`) downloaded with curl, checksum-verified, installed to `/usr/local/bin/herdr`, and a baked `/root/.config/herdr/config.toml` with `onboarding = false`, both update checks off, and `default_shell` and `new_cwd` under `[terminal]`. The recipe and config are the ones in `spike/Containerfile.spike` and `spike/config.toml`. `BASE_IMAGE_VERSION` goes from 7 to 8.
- **Renderer.** `src/lib/terminal/WtermRenderer.svelte` and `TerminalView.svelte` change only if the Phase D acceptance check fails.

## Resolved Decisions

1. **Q1, replace or keep zellij: replace, no setting.** The Containerfile drops zellij and `src-tauri/src/terminal/mod.rs` has one multiplexer path, matching native-terminal Resolved Decision 3.
2. **Q2, install route and pin: the release asset at v0.9.5, with a build-argument version and sha256.** The Containerfile uses `ARG HERDR_VERSION` and `ARG HERDR_SHA256` and runs `sha256sum -c` before `chmod +x`; `base-image-recipe.spec.ts` asserts the version argument is an exact semver. Moving to mise stays deferred to the next mise bump and is recorded in [04_improvements.md](04_improvements.md), so this decision is closed for the build.
3. **Q3, detached server start: one engine exec with the foreground check, background start and bounded readiness poll described in the model.** `create_argv()` renders it; `create_session()` in `src-tauri/src/terminal/mod.rs` interprets exit codes 0, 127 and other.
4. **Q4, session naming: a named session per sandbox through `session_name()`.** The 61-character cap is the socket-path limit measured in the spike; the hash suffix keeps two long names that share a 52-character prefix apart. A unit test in `src-tauri/src/terminal/mod.rs` covers the pass-through, the boundary at 61 and 62, and determinism. `validate_name()` in `src-tauri/src/backends/mod.rs` is left unchanged so existing long-named sandboxes keep working.
5. **Q5, mouse and clipboard through wterm: an acceptance check in Phase D, fixed in `WtermRenderer.svelte` if it fails, with xterm.js behind the renderer interface as the fallback.** The check is manual in the running app, since the e2e light tier uses the mock transport and cannot show Herdr's mouse modes.
6. **Q6, sandboxes created before the bump: detect and prompt.** Exit 127 maps to `TerminalError::MultiplexerMissing`; the existing outdated-image prompt in `src/routes/projects/SandboxItem.svelte` (driven by `Sandbox.image_version` and `get_dismissed_image_version`) already offers recreation, so no zellij fallback is kept.
7. **Q7, prefix key and detach binding: Herdr defaults, nothing bound app-side.** The baked config sets no keybindings, per native-terminal Resolved Decision 4.
8. **Q8, agent state in the app: still open and deferred.** It stays in [04_improvements.md](04_improvements.md); the build only needs the TUI.
9. **Q9, first-frame cost: accepted, measured in Phase D.** The spike's 743 ms against zellij's 635 ms is within the native terminal's budget; Phase D records the in-app figure in the feature's e2e plan, and a figure far above the spike's reopens the decision with the user.

## Parallel-Worktree Constraints

- **New app path:** none. Herdr's sockets and state live under `/root/.config/herdr` inside each sandbox; the host app reads and writes nothing new.
- **New host port:** none. The Herdr API socket is a Unix socket inside the sandbox.
- **New host-global resource:** none. The specs stay in the `standard` tier with the existing terminal specs. The host's own `~/.herdr` install is never used; every `herdr` command runs through `podman exec` inside a sandbox.
- **New bundled binary under `src-tauri/binaries/`:** none. Herdr is downloaded during the base-image build, so `src-tauri/build.rs` gets no asset group. The pin lives in the Containerfile, as mise's does.
- **New teardown of host-global state:** none.
- **New environment variable:** none.
- **Schedule cost:** bumping `BASE_IMAGE_VERSION` to 8 makes every worktree's first `standard` run build the new base image (`e2e/lib/podman-machine.ts` parses the constant); the build now needs outbound access to github.com for the Herdr asset, as it already does for mise.

## Implemented: Phase A: Base image

- `src-tauri/resources/Containerfile`: remove the `ZELLIJ_VERSION` block and the zellij config dump; add the Herdr block and baked config per the model. Reword the `SHELL=bash` comment (Herdr's `default_shell` is the setting that matters now; keep `SHELL` because other tools read it) and the `WORKDIR` comment that names the zellij server.
- The baked config is written with `printf` in the Containerfile, as the sshd config is, not copied from a file: `ensure_image` in `src-tauri/src/provider/mod.rs` writes the embedded Containerfile alone into a temporary build context, so no `herdr-config.toml` exists.
- `src-tauri/src/backends/mod.rs`: `BASE_IMAGE_VERSION` 7 to 8.
- `e2e/test/specs/standard/base-image-recipe.spec.ts`: replace `PINNED_ZELLIJ` with a pattern for the `HERDR_VERSION` argument and add an assertion that a `sha256sum -c` check follows the download; update the floating-latest comment.
- `e2e/test/specs/standard/base-image-build.spec.ts`, `base-image-remote.spec.ts`: the "mise + zellij" size comments now say "mise + Herdr" without a size, because the new image size was not re-measured. The recipe spec keeps the zellij pin check for the two spike Containerfiles in `docs/features/sandbox-runtime-management/spike/mise/`, which still install zellij.
- Hand-verified: `podman build` of the Containerfile succeeds on the Podman machine, `sha256sum -c` passes and `herdr --version` prints 0.9.5.

## Implemented: Phase B: Terminal host

- `src-tauri/src/terminal/mod.rs`: add `session_name()`; rewrite `create_argv()` and `client_command()` to take the session name; delete `session_already_running()`; change `create_session()` to map exit 127 to `TerminalError::MultiplexerMissing` and any other non-zero exit to `TerminalError::SessionCreate` carrying stderr; update the module and `WORKDIR` doc comments. Replace the three unit tests: argv shape, `shell_quote` cases kept, `session_name` cases.
- `src-tauri/src/terminal/types.rs`: add `MultiplexerMissing`; reword the doc comments that say zellij (`AttachRequest.name`, `TerminalEvent::Exited`, `SessionCreate`, the `TerminalHost` trait and its `attach` and `detach` methods).
- `src-tauri/src/commands/terminal.rs`, `src-tauri/src/workspace/mod.rs` (the comment near line 415), `src-tauri/AGENTS.md` (the `TerminalHost` row and the `terminal/` bullet), root `AGENTS.md` (the repo-map line): replace zellij with Herdr.
- `src-tauri/src/terminal/mock.rs` and `host.rs`: no change. Verify with `cargo check` and `cargo test` in `src-tauri/`.
- Hand-verified against a container on the new image, running the start script through `podman exec -w /root/work`: the first call returns 0 only after `status server` reports running (about 790 ms including roughly 430 ms of exec overhead), a second call returns 0 at once (about 450 ms), `herdr session list --json` shows the named session running, the server process holds the container's proxy variable and `/root/work` as its working directory, and with `herdr` removed from `PATH` the script exits 127. The full SSH attach through the app was not run by hand; Phase C's spec covers it.

## Implemented: Phase C: e2e specs

- `e2e/test/specs/standard/embedded-workspace.spec.ts`: replace `liveZellijSessions()` with a helper that runs `herdr session list --json` through `podman exec` and returns the names of sessions with `running` true, excluding the always-present `default` session. Keep the three assertions (none before the Terminal tab opens, exactly one named after the sandbox once attached, still one after closing and reopening the tab). Spec sandbox names are short, so the helper compares against the plain name; a long-name rule is covered by the Rust unit test only.
- Rename the zellij wording in that spec's titles, comments and `timeoutMsg` strings.
- Run: `embedded-workspace.spec.ts` passes in full (13 tests) against the Podman test machine in the real webview, which proves the SSH attach, the single named session, and the close and reopen reattach on the new image. `base-image-recipe.spec.ts` was checked only by evaluating its patterns against the Containerfile, not by a wdio run.
- `docs/features/herdr-multiplexer/03_e2e.md` is the next step after this document and covers any new spec coverage (an old-image sandbox's error, the long name).

## Implemented: Phase D: Renderer acceptance

- The pointer, selection and clipboard checks need a person operating the window, and no automated driver covers Herdr's mouse modes. The user ran every check by hand, including microsandbox and a deployed sandbox, and reported all of them checked with no issues; no measured first-frame time was reported, and `WtermRenderer.svelte` needed no change. The checks: run the app against a Podman sandbox and check in the real webview: a click on a pane and a tab moves focus; dragging a split border resizes; drag-select and double-click write the selection to the host clipboard (OSC 52 through `onClipboardWrite` in `src/lib/terminal/TerminalView.svelte`); Shift-drag falls back to native selection; the wheel scrolls. Record the time to first complete frame.
- If a check fails, fix it in `src/lib/terminal/WtermRenderer.svelte` (the only file importing `@wterm/*`) or, if wterm cannot be made to work, swap the renderer behind the existing interface and record the swap in this document.
- Not covered: microsandbox and a deployed sandbox were never run in the spike. Run each once by hand in this phase and record the result.

## Implemented: Phase E: Documents

- `docs/architecture/overview.md`: the Base image row names zellij; change it to Herdr.
- `docs/architecture/workspace-ui.md` still describes the terminal as `zellij web`, which native-terminal's Phase F owns. Only the multiplexer name changed there; the rest of the paragraph still describes the web terminal until that phase runs.
- `docs/features/herdr-multiplexer/feature.md` is `implemented` and `docs/features/log.md` carries the `**Implement**` entry.
- Run `./build docs --fix`, then `./build docs`.

## Lifecycle and Cross-Cutting Concerns

- **Detach is non-destructive.** Closing the pane ends the SSH session and the exec child only; no code path calls `herdr server stop` or `herdr session stop`, so panes and agents survive. A container stop ends them, as a zellij session also ended.
- **Environment inheritance.** Every attach runs the start exec first, so the server is always created with the container's environment. A server that already runs keeps the environment it was created with; a sandbox whose proxy settings change needs its Herdr server restarted (a container restart does this).
- **Old sandboxes keep zellij and no Herdr.** They show the recreate message until recreated; their running zellij sessions are not migrated.
- **Superseded approach.** The previous `zellij attach --create-background` argv and `session_already_running()` are removed with zellij; do not reintroduce a stderr match for "already running", because the backgrounded Herdr server exits 0 and writes its error to discarded output.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Panes show `/root` and no proxy variables | The SSH client attached before the server was running and started one with the stripped `sshd` environment | Keep the readiness poll in the start exec of `create_argv()` (`src-tauri/src/terminal/mod.rs`); never background the status check |
| A second attach starts a duplicate server or reports a false success | A backgrounded `herdr server` against a running session exits 0 with its error discarded | Check `herdr --session <name> status server` in the foreground first; both of its answers exit 0 |
| `could not create the terminal session` with an empty message on an old sandbox | The sandbox predates base image 8 and has no `herdr` binary, so the start exec exits 127 | `create_session()` maps 127 to `TerminalError::MultiplexerMissing`; recreate the sandbox |
| `local socket name length exceeds capacity of sun_path of sockaddr_un` | A session name longer than 61 characters under `/root` | Derive the session name with `session_name()`; never pass the raw sandbox name |
| The base-image build fails installing Herdr through mise with `TSA timestamp verification failed` | mise 2026.6.14's attestation check fails on the Herdr release | Download the pinned asset with curl and verify the sha256; do not disable attestation |
| Herdr shows the welcome box or contacts herdr.dev | `onboarding`, `update.version_check` and `update.manifest_check` default to true | Keep all three off in the baked config |
| Herdr's `default_shell` and `new_cwd` have no effect | The keys sit at the top level instead of the `[terminal]` table | Put them under `[terminal]` in the baked config |
| `base-image-recipe.spec.ts` fails after the swap | It still asserts a pinned zellij version | Assert the `HERDR_VERSION` argument and the checksum step instead |
| The session-count spec finds none | `liveZellijSessions()` calls `zellij list-sessions`, which no longer exists in the image | Read `herdr session list --json` through `podman exec` and drop the `default` entry |
| A spec or script reaches the host's own Herdr | A `herdr` command run on the host talks to the API socket under `%APPDATA%\herdr` | Run Herdr only through `podman exec` or `msb exec` inside a sandbox |
