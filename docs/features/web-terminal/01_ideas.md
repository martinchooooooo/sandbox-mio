---
type: ideas
title: "In-App Web Terminal (Zellij) Plan"
feature: web-terminal
---
# In-App Web Terminal (Zellij) Plan

This document is the ideation for giving each sandbox an interactive terminal that opens inside the Tauri app. The terminal is provided by the Zellij web client (`zellij web`, see https://zellij.dev/tutorials/web-client/) running inside the sandbox container and surfaced to the user as a webview window in the desktop app. None of this exists yet: there is no terminal UI, the container publishes no ports, and Zellij is not part of the base image. The feature builds on three existing seams — the flox/Nix runtime substrate already baked into the base image (`src-tauri/resources/Containerfile`), the `podman create` container lifecycle (`create` in `src-tauri/src/backends/mod.rs` line 286), and the `podman exec` command path (`flox_exec` and `podman` in `src-tauri/src/runtime/mod.rs`). The per-sandbox action buttons that a "Terminal" button joins live in `src/routes/projects/+page.svelte` (lines 342–357).

## Current State

A sandbox is a rootless Podman container created from the flox-baked base image `sandbox-mio/base:flox`. `create` in `src-tauri/src/backends/mod.rs` (line 286) runs `podman create --pull=missing --tty --volume <workspace>:/root/work --workdir /root/work --name <name> <image> /bin/sh`. The command publishes no ports: there is no `--publish` flag, so nothing inside the container is reachable from the host today. `WORKSPACE_MOUNT` is `/root/work` (`src-tauri/src/backends/mod.rs` line 33) and is the container's working directory.

The base image already carries the Nix substrate and the flox CLI, baked by `src-tauri/resources/Containerfile` (Nix installed via `apk add nix git`, flox pinned to a commit via `nix profile add github:flox/flox/<rev>`). Per the runtime-management spike (`docs/features/sandbox-runtime-management/01_ideas.md`), Nix packages carry their own glibc closure and run on the Alpine/musl base, so any nixpkgs binary — including Zellij — can run in the sandbox without a system glibc. The image is built on demand per connection by `ensure_base_image` (`src-tauri/src/backends/mod.rs` line 258), never pulled from a registry.

Software runs inside a sandbox over `podman exec`. `RuntimeManager` (`src-tauri/src/runtime/types.rs` line 20) wraps this: `flox_exec` (`src-tauri/src/runtime/mod.rs` line 73) starts the container if stopped, then runs `podman exec --workdir /root/work <name> flox <args>`. The same `podman exec` shape is the mechanism a terminal-launch command would reuse to start `zellij web` inside the container. Package installs flow through `install_package` (`src-tauri/src/lib.rs` line 43) → `RuntimeManager::install` → `flox install`, which writes the per-sandbox `.flox/` manifest under the synced workspace.

The frontend renders one row per sandbox in `src/routes/projects/+page.svelte`. The action buttons (`folder`, `undeploy`, `start`/`stop`, `deploy`, `destroy`) are at lines 342–357; each calls an `async function` that wraps `invoke(...)` of a Tauri command. A "Terminal" button joins this row and calls a new command. The app opens external paths today via `tauri_plugin_opener` (`revealItemInDir`, line 7; the plugin is registered in `src-tauri/src/lib.rs` line 252). The Tauri window config is in `src-tauri/tauri.conf.json` (`windows` at line 13, `security.csp` is `null` at line 21), and capabilities are in `src-tauri/capabilities/default.json`.

## Zellij As The Terminal Server

The terminal server is Zellij's built-in web client, run as `zellij web` inside the sandbox container. By default it binds `http://127.0.0.1:8082` *inside the container's* network namespace and serves a browser-based terminal that attaches to Zellij sessions. The seam that launches it is a new `RuntimeManager` operation (or a sibling of it) that execs `zellij web` in the container over the existing `podman exec` path (`src-tauri/src/runtime/mod.rs`). Because the server binds container-local `127.0.0.1`, it is unreachable from the host until a port is published (see "Publishing The Web Client Port").

## Authentication With A One-Time Token

The Zellij web server refuses logins without a token. A token is minted with `zellij web --create-token` and is **displayed only once** — it can never be re-read. The launch command therefore execs `zellij web --create-token`, captures the token from stdout, and hands it to the frontend together with the URL so the webview can authenticate. The seam is the return type of the new launch command: it returns a `{ url, token }` pair rather than a bare URL. The 2026-06-27 spike established that `/auth` is a JavaScript login page and the token is **not** consumable via a URL query parameter (`GET /auth?token=…` does not authenticate; a form `POST /auth` returns `405`), so auto-login requires injecting the token into the page from the webview (e.g. a Tauri `initialization_script` that seeds `localStorage`/cookie) rather than appending it to the URL. The exact value the page reads is the remaining unknown (see Open Questions).

## Publishing The Web Client Port

`zellij web` binds container-local `127.0.0.1:8082`, so the container must publish that port to the host. Podman maps it with `--publish 127.0.0.1:<hostport>:8082`, and on Windows the Podman machine VM forwards the published port through to the Windows host's loopback, making `http://127.0.0.1:<hostport>` reachable from the Tauri app. A container's port mappings are fixed at `podman create` time and cannot be added to an existing container, so this changes the `create` call in `src-tauri/src/backends/mod.rs` (line 286): every sandbox must be created with the web-client port published. Binding the *host* side to `127.0.0.1` (not `0.0.0.0`) keeps the terminal off the local network, matching Zellij's own loopback default.

Multiple sandboxes cannot all publish host port 8082, so the host port is allocated per sandbox. Letting Podman assign an ephemeral host port (`--publish 127.0.0.1::8082`) and reading it back with `podman port <name> 8082` avoids collisions without a port-bookkeeping table; the read-back value becomes the `url` in the launch command's return.

The 2026-06-27 spike found that publishing alone is not enough: Podman's default rootless network backend is **pasta**, which forwards a published port to the container's `eth0`, not its loopback. Because Zellij binds the container's `127.0.0.1` (and binding any other IP forces SSL), the container must be created with `--network pasta:--host-lo-to-ns-lo` so pasta translates the host-loopback-published port to the container's loopback. `slirp4netns`, whose rootlessport forwards to container loopback by default, is not installed in the Podman machine, so the pasta flag is the available mechanism. This also changes the `create` call in `src-tauri/src/backends/mod.rs` (line 286).

## Surfacing The Terminal As An In-App Webview

The terminal opens inside the Tauri app rather than the system browser. The launch flow is: the "Terminal" button in `src/routes/projects/+page.svelte` (the action row at lines 342–357) calls a new command; the command starts `zellij web`, mints a token, reads the published host port, and returns `{ url, token }`; the frontend opens a Tauri `WebviewWindow` pointed at that URL. A dedicated `WebviewWindow` (its own top-level window) is required rather than an embedded iframe: the 2026-06-27 spike found every web-client response carries `X-Frame-Options: DENY`, which blocks iframing but does not apply to a top-level webview document. Opening a webview window at runtime requires the corresponding capability in `src-tauri/capabilities/default.json`.

## Technology Choices With Tradeoffs

### Terminal engine: Zellij web client

Zellij's `zellij web` is the chosen terminal engine. It ships a complete browser terminal (multiplexer, layouts, scrollback) with a built-in HTTP server and token auth, so the app provides a terminal by launching one process rather than building terminal I/O plumbing. Alternatives evaluated:

- **`ttyd` / `gotty` (a generic browser-TTY bridge wrapping `/bin/sh`)** — rejected as the primary engine because it provides only a raw shell with no multiplexing, session persistence, or layouts, and is a second tool to package and pin alongside the existing flox stack. Zellij gives session persistence (a dropped webview reattaches to the running session) for free.
- **`xterm.js` in the Svelte frontend driven over a Tauri-managed PTY** — rejected because it requires the app to own the PTY lifecycle, stdin/stdout streaming, and resize protocol across the `podman exec` boundary, and it does not survive the host process; it is substantially more code than execing a server that already does all of this. Reconsider only if the webview/token approach proves unworkable.
- **A system-browser tab via `tauri_plugin_opener` instead of an in-app webview** — rejected against the feature's explicit "inside our Tauri app" requirement, though it is the simplest path and the natural fallback if loading `http://127.0.0.1` in an in-app `WebviewWindow` hits a CSP or mixed-content wall.

### Installing Zellij: flox into the base image

Zellij is installed from nixpkgs via flox and baked into the base image (`src-tauri/resources/Containerfile`), the same mechanism that already bakes the flox CLI. nixpkgs ships a `zellij` package whose Nix closure carries its own glibc, so it runs on the Alpine/musl base (proven for nix packages generally by the runtime-management spike). Baking it into the base image means every sandbox has a terminal instantly and the binary is staged on every connection by `ensure_base_image`. Alternatives evaluated:

- **Per-sandbox flox install (not baked) via the existing `install_package` path** — rejected as the default because it makes the terminal opt-in, adds first-launch latency, and lists `zellij` among the user's chosen packages in the UI (`list_packages`), conflating an app-provided capability with user software. Baking keeps the terminal an always-present app feature. Revisit if base-image size becomes a concern.
- **`apk add zellij` from Alpine repositories** — rejected because it pins to Alpine mirror state rather than the flox lockfile, may lag the Zellij version that has the web client, and reintroduces the non-reproducible install path the runtime-management feature deliberately removed.
- **Downloading an upstream prebuilt Zellij binary** — rejected because Zellij's release page documents no Alpine/musl static variant, and a curled binary leaves no manifest record and is unmanaged across upgrades, unlike a pinned nixpkgs build.

### Port exposure: published ephemeral host port bound to loopback

Publishing `127.0.0.1::8082` at `podman create` and reading the assigned host port back is chosen over the alternatives:

- **A fixed host port (e.g. always 8082)** — rejected because two running sandboxes would collide on the host port; ephemeral assignment sidesteps a port-allocation table entirely.
- **An on-demand SSH/`socat` tunnel from host to container instead of a published port** — rejected for local sandboxes as unnecessary moving parts when Podman already forwards published ports through the machine VM. This path is, however, the likely answer for *remote-deployed* sandboxes, where the container's published port lives on the remote host (see Open Questions).
- **Podman host networking (`--network host`)** — rejected because rootless Podman on a Windows machine VM does not expose container host-networking to the Windows host, so it would not make the port reachable from the Tauri app.

## Validation

A feasibility spike was run by hand on 2026-06-27 against the real flox-baked base image (`sandbox-mio/base:flox`) on the local `podman-machine-default` connection (Podman 5.8.2, rootless, pasta networking). It drove a container over `podman exec` exactly as the app does. The reproduction script is at `docs/features/web-terminal/spike/reproduce.sh`. The spike proved the end-to-end path — flox-installed Zellij serving the web client to the Windows host — and isolated three setup requirements that the build must encode.

Cleared risks:

- **Zellij installs via flox on the musl base and runs.** `flox install zellij` succeeded (`✔ 'zellij' installed to environment 'work'`) and `zellij --version` reported `zellij 0.44.3` — recent enough for the web client (added in Zellij 0.43). The nixpkgs closure carries its own glibc, as the runtime-management spike found for nix packages generally. Critical detail: the install must exec **without a login shell**. `/etc/profile` in the base image sets `NIX_REMOTE=daemon`, and there is no nix daemon, so `podman exec ... sh -lc 'flox install ...'` fails with `error: cannot connect to socket at '/nix/var/nix/daemon-socket/socket'`. The app's `flox_exec` (`src-tauri/src/runtime/mod.rs` line 73) already execs `flox` directly without a login shell, so it is unaffected; a terminal-launch command must do the same.
- **The web server runs inside the container.** After creating a config file, `zellij web --daemonize` reported `Web server online with version: 0.44.3` and `netstat -ltn` showed `127.0.0.1:8082 LISTEN`; an in-container fetch returned the web client HTML.
- **The published port reaches the Windows host.** With the container run as `--network pasta:--host-lo-to-ns-lo --publish 127.0.0.1::8082`, `Invoke-WebRequest http://127.0.0.1:<hostport>/` from the Windows host returned `STATUS: 200`, `Content-Type: text/html`, serving the web client. The full chain works: Windows loopback → gvproxy → machine VM → pasta → container `127.0.0.1:8082`.
- **`zellij web` has `--ip` and `--port` CLI flags.** `zellij web --help` lists `--ip` (default `127.0.0.1`), `--port` (default `8082`), `--daemonize`, `--status`, `--stop`, and token management (`--create-token`, `--revoke-token`, `--list-tokens`). Binding any IP other than `127.0.0.1` requires `--cert` and `--key` (SSL), which is why the design keeps the server on container loopback and relies on Podman port publishing instead.
- **The terminal cannot be iframed; a top-level `WebviewWindow` is required.** Every response carried `X-Frame-Options: DENY` and `Content-Security-Policy: default-src 'self'; ... connect-src 'self' ws: wss:`. Embedding the terminal in an `<iframe>` inside the SvelteKit page would therefore be blocked. A dedicated Tauri `WebviewWindow` loads the URL as a top-level document, to which `X-Frame-Options` does not apply, confirming that design choice.

Blockers / requirements surfaced (each cleared in the spike but mandatory for the build):

- **The web server needs a config file or it never binds.** First launches reported `Web Server started` but `--status` showed offline and nothing listened; the daemon log (`/tmp/zellij-0/zellij-log/zellij.log`) showed `ERROR zellij_client::web_client ... Failed to find default config file path`. Creating one (`zellij setup --dump-config > /root/.config/zellij/config.kdl`) fixed it. The build must ensure a config exists before starting the server.
- **pasta's default forwarding does not reach a loopback-bound server.** Without `--host-lo-to-ns-lo`, the published port was unreachable from both the Windows host (`connection closed unexpectedly`) and the machine VM (no connection), because pasta forwards published ports to the container's `eth0` while Zellij binds `127.0.0.1`. `slirp4netns` (whose rootlessport forwards to container loopback) is not installed in the Podman machine, so `--host-lo-to-ns-lo` is the available fix and must be set on the container's network.

Not yet validated:

- **Non-interactive token login.** `/auth` is a JavaScript login page (HTTP 200; a form `POST /auth` returns `405 Method Not Allowed`, and `GET /auth?token=…` does not authenticate), so the one-time token is not consumable via a URL query parameter. Auto-login from the webview would require injecting the token into the page (e.g. a Tauri `initialization_script` seeding `localStorage`/cookie) rather than appending it to the URL; the exact mechanism the page expects is unconfirmed.
- **Loading the URL in an actual Tauri `WebviewWindow`.** The X-Frame-Options finding settles the iframe-vs-window choice, but launching a real `WebviewWindow` against the loopback URL was not exercised in the spike.

## Open Questions

- **Token delivery to the webview.** The 2026-06-27 spike ruled out the URL-query-parameter path (`/auth` is a JS page; `GET /auth?token=…` does not authenticate). What remains undecided is *which* injection mechanism logs the user in automatically — a Tauri `initialization_script` seeding `localStorage`/cookie with the exact key the page reads, versus presenting a paste field — which `02_dev.md` resolves after inspecting the web client's auth JavaScript.
- **Web-server lifecycle.** Whether `zellij web` is started on demand by the "Terminal" command and left running for the container's lifetime, started fresh each click, or enabled declaratively via Zellij config (`web_server true`) baked into the image, is undecided; on-demand avoids a server running when unused but adds first-click latency.
- **Baked versus per-sandbox install.** Whether Zellij is baked into `src-tauri/resources/Containerfile` (always present, larger base image, restaged on every connection) or installed per sandbox through the flox manifest is undecided; this ideas document leans baked, but the build plan confirms it against base-image size.
- **Remote-deployed sandboxes.** Whether the terminal works for a sandbox deployed to a remote host — where the published port is on the remote, not the Windows host — and if so whether it is reached over an SSH tunnel, is undecided and may be scoped out of the first iteration (local sandboxes only).
- **In-app webview versus external browser fallback.** The spike confirmed the terminal must open as a top-level `WebviewWindow` (iframing is blocked by `X-Frame-Options: DENY`). What is undecided is whether to keep a `tauri_plugin_opener` (system browser) fallback if a real `WebviewWindow` fails to load the loopback URL, since launching an actual window was not exercised in the spike.
- **HTTP versus HTTPS.** Zellij warns that HTTPS (`web_server_cert`/`web_server_key`) is a hard requirement for sharing beyond the local machine. Whether plain HTTP over loopback is acceptable for the local-only case (it is for a loopback-bound port) or whether a self-signed certificate is configured for parity with a future remote case is undecided.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Web terminal unreachable from the Tauri app | The container has no port mapping; `create` in `src-tauri/src/backends/mod.rs` (line 286) runs `podman create` with no `--publish`, and `zellij web` binds container-local `127.0.0.1:8082` | Create the container with `--publish 127.0.0.1::8082`; port mappings cannot be added after `podman create`, so existing sandboxes must be recreated to gain a terminal |
| Login token lost and cannot be recovered | `zellij web --create-token` displays the token only once and can never redisplay it | Capture the token from the launch command's stdout at mint time and return it to the frontend with the URL; never assume it can be re-read |
| Two sandboxes collide on the host port | Both published the same fixed host port (e.g. 8082) | Publish an ephemeral host port (`--publish 127.0.0.1::8082`) and read the assigned value back with `podman port <name> 8082` |
| Terminal exposed on the local network | Publishing with the host side bound to `0.0.0.0` | Bind the host side to `127.0.0.1` (`--publish 127.0.0.1::8082`) so the terminal stays on loopback, matching Zellij's own default |
| `zellij` assumed broken on the Alpine/musl base | The base image (`src-tauri/resources/Containerfile`) is Alpine with musl and no system glibc | No fix needed: the nixpkgs `zellij` closure carries its own glibc and runs on musl, as nix packages did in the runtime-management spike |
| Webview window fails to open at runtime | The webview-window creation capability is missing from `src-tauri/capabilities/default.json` | Add the capability that permits creating a `WebviewWindow` before invoking it from `src/routes/projects/+page.svelte` |
| Remote-deployed sandbox terminal does not connect | The published port is on the remote host's loopback, not the Windows host, so `http://127.0.0.1:<hostport>` hits nothing locally | Treat remote terminals as a separate case (SSH tunnel) or scope the first iteration to local sandboxes (see Open Questions) |
| Published port unreachable even though the server is listening | Podman's default pasta network forwards published ports to the container's `eth0`, but `zellij web` binds container `127.0.0.1` | Create the container with `--network pasta:--host-lo-to-ns-lo` so pasta forwards host-loopback ports to the container's loopback (`slirp4netns` is not installed in the Podman machine) |
| `zellij web` logs "Web Server started" but nothing listens and `--status` says offline | The web server aborts when it cannot resolve a config file (`Failed to find default config file path` in `/tmp/zellij-0/zellij-log/zellij.log`) | Create a config first: `zellij setup --dump-config > /root/.config/zellij/config.kdl` before starting the server |
| `flox install` fails with `cannot connect to socket at '/nix/var/nix/daemon-socket/socket'` | A login shell sources `/etc/profile`, which sets `NIX_REMOTE=daemon`, but the sandbox runs single-user nix with no daemon | Exec `flox` directly without a login shell (as `flox_exec` in `src-tauri/src/runtime/mod.rs` already does); never `sh -lc` around flox/nix |
| Terminal appears blank when embedded in the app page | The web client sends `X-Frame-Options: DENY`, blocking it inside an `<iframe>` | Open it as a top-level Tauri `WebviewWindow`, to which `X-Frame-Options` does not apply, not an embedded iframe |
| Git Bash mangles `/bin/sh` into a Windows path when invoking podman by hand | MSYS path conversion rewrites leading-slash arguments | Prefix one-off podman commands with `MSYS_NO_PATHCONV=1` (the running app uses the Rust `podman` helper and is unaffected) |
