# In-App Web Terminal (Zellij) — Build Plan

This document is the build plan for the in-app web terminal whose design is in `docs/features/web-terminal/01_ideas.md`. It continues that ideation and supersedes it where a decision is now settled. The terminal is the Zellij web client (`zellij web`) run inside a sandbox container, published to the host loopback through Podman, and opened in a dedicated Tauri `WebviewWindow`. The 2026-06-27 spike recorded in `01_ideas.md` proved the end-to-end path (flox-installed Zellij serving HTTP 200 to the Windows host) and is the basis for this plan. **Status: nothing in this feature is implemented yet — all four phases (A–D) below are Remaining.** The feature touches four files of substance: `src-tauri/resources/Containerfile`, `src-tauri/src/backends/mod.rs`, `src-tauri/src/runtime/{mod.rs,types.rs}`, `src-tauri/src/lib.rs`, plus the frontend `src/routes/projects/+page.svelte`, the locale files under `messages/`, and `src-tauri/capabilities/default.json`.

## The Concrete Model

A terminal is reached in four hops that mirror the spike. (1) The base image ships Zellij and a Zellij config so the binary is on the container `PATH` and the web server can start. (2) Every sandbox container is created with the web-client port published to the host loopback and pasta told to forward host-loopback ports to the container loopback. (3) A new `RuntimeManager::open_terminal` operation, invoked by a new `open_terminal` Tauri command, starts the Zellij web server on demand (idempotently), mints a one-time login token, reads back the ephemeral host port Podman assigned, and returns a `{ url, token }` pair. (4) The frontend "Terminal" button calls that command and opens a dedicated `WebviewWindow` at the returned URL; the token is surfaced to the user for the one-time login the web client requires.

Zellij is baked into the **image's root Nix profile** exactly as the flox CLI already is (`nix profile add nixpkgs#zellij` then a symlink into `/usr/local/bin`), not into a per-sandbox flox environment. This means `zellij` is on the global `PATH` and `open_terminal` runs plain `zellij web` over `podman exec` — it does **not** go through `flox activate`, and it does not depend on the workspace `.flox/` environment existing. This is the key refinement over the `01_ideas.md` phrase "flox into the base image", which conflated baking-into-the-image (the flox-CLI mechanism) with the per-sandbox flox env; the build uses the former.

## Resolved Decisions

Each Open Question from `01_ideas.md` is resolved here; one is deferred deliberately.

- **Token delivery to the webview** → The `WebviewWindow` is created with an `initializationScript` that calls `POST /command/login` with `{ auth_token, remember_me: true }` on every page load. Zellij's `/command/login` endpoint sets a 30-day session cookie; the script then redirects away from `/auth` if needed. This eliminates the manual paste UX. Mechanism sourced from: https://github.com/zellij-org/zellij/issues/5194#issuecomment-4516799423. **Implemented** in `src/routes/projects/SandboxItem.svelte:openTerminal`.
- **Where the auth code is stored** → the **plaintext token** is captured from the `zellij web --create-token` stdout and written to the file `/root/work/.mio/zellij.json` in the sandbox workspace; the app does **not** rely on Zellij's own store and does not read back the validator hash. (Background, from the 2026-06-27 spike: Zellij keeps only a *hash* in the SQLite DB `/root/.local/share/zellij/tokens.db` in the container and never re-emits the plaintext, which is why the app must capture it at creation.) Writing the plaintext into the bind-mounted, Mutagen-synced `/root/work` is a **known, deliberate workaround** — it places a live secret on every synced remote and risks being committed — accepted as a temporary step and recorded in `docs/techdebt.md`. The intended replacement (transient memory / OS keychain) is the deferred "auto-login / token storage" follow-up.
- **Web-server lifecycle** → started **on demand** by `open_terminal`, idempotently: the command checks `zellij web --status` and starts `zellij web --daemonize` only when offline, then leaves the server running for the container's lifetime. No always-on `web_server true` config, and no per-click restart.
- **Baked versus per-sandbox install** → **baked** into `src-tauri/resources/Containerfile` (image root Nix profile), making the terminal an always-present capability rather than user-listed software in `list_packages`.
- **Remote-deployed sandboxes** → **out of scope** for this iteration; the terminal targets local sandboxes only. The deploy-time remote `create` path (`src-tauri/src/backends/mod.rs` near line 398) is intentionally left without the port/network flags, and reaching a remote terminal over an SSH tunnel is left for a later feature.
- **In-app webview versus external browser fallback** → a top-level Tauri `WebviewWindow` with **no** system-browser fallback in the MVP, since the spike showed the loopback URL serves HTTP 200 (`X-Frame-Options: DENY` only rules out iframing, not a top-level window).
- **HTTP versus HTTPS** → **plain HTTP over loopback**. No certificate is configured; the server stays bound to container `127.0.0.1` (binding any other IP would force `--cert`/`--key`), and the published host port is bound to `127.0.0.1`.

## Remaining: Phase A — Bake Zellij into the base image

Edit `src-tauri/resources/Containerfile` (the recipe `include_str!`-baked into the binary via `CONTAINERFILE` at `src-tauri/src/backends/mod.rs` line 21). Add, after the existing flox `nix profile add` step:

- `nix profile add nixpkgs#zellij` and `ln -sf /root/.nix-profile/bin/zellij /usr/local/bin/zellij`, mirroring the flox-CLI baking at lines 20–21 of the current Containerfile. **Pin the nixpkgs input to a full commit rev** (e.g. `nix profile add github:NixOS/nixpkgs/<rev>#zellij`): the 2026-06-27 spike found that the unpinned `nixpkgs#zellij` flakeref resolves through the GitHub API and failed the build with `API rate limit exceeded for <ip>` (HTTP 403), the same 60-req/h unauthenticated limit the Containerfile's flox-rev comment already calls out. A full rev fetches `archive/<rev>.tar.gz` directly and skips the rate-limited API.
- `mkdir -p /root/.config/zellij && zellij setup --dump-config > /root/.config/zellij/config.kdl` so the web server can resolve a config file (without it the server logs `Failed to find default config file path` and never binds — see `01_ideas.md` Validation).

Bump `BASE_IMAGE_VERSION` in `src-tauri/src/backends/mod.rs` (line 17) from `"flox"` to a new tag (e.g. `"zellij"`). This is mandatory: `ensure_base_image` (`src-tauri/src/backends/mod.rs` line 258) short-circuits when `podman image exists <tag>` succeeds, so without a new tag existing machines keep the old image and never get Zellij. Bumping the tag forces a one-time rebuild on every connection.

## Remaining: Phase B — Publish the web port at container create

Edit `create` in `src-tauri/src/backends/mod.rs` (line 286). The current `podman create` argument vector (lines 299–304) is `["create", "--pull=missing", "--tty", "--volume", &volume, "--workdir", WORKSPACE_MOUNT, "--name", name, &image, "/bin/sh"]`. Add three things:

- `--network`, `pasta:--host-lo-to-ns-lo` — so pasta forwards the host-loopback-published port to the container's loopback where Zellij binds (the spike proved the default pasta forwarding to `eth0` leaves a loopback-bound server unreachable).
- `--publish`, `127.0.0.1::8082` — publish the container's web port to an ephemeral host port bound to host loopback only.
- Introduce a `const ZELLIJ_WEB_PORT: u16 = 8082;` (or `&str`) near `WORKSPACE_MOUNT` (line 33) and build the publish/port-read strings from it so the container port is named once.

The remote deploy `create` path (the second `podman create` near line 398) is **not** changed (remote terminals are out of scope per Resolved Decisions).

## Remaining: Phase C — `open_terminal` in the runtime, plus the Tauri command

### Runtime trait and implementation

Add to the `RuntimeManager` trait in `src-tauri/src/runtime/types.rs` (line 20):

```
async fn open_terminal(&self, name: &str, workspace_host_path: &str) -> Result<TerminalSession, RuntimeError>;
```

`workspace_host_path` is passed (as `list` and `reproduce_remote` already do) because the token is written host-side into that directory's `.mio/` subfolder.

Add a serializable result struct in the same file:

```
#[derive(serde::Serialize)]
pub struct TerminalSession { pub url: String, pub token: String }
```

Implement `open_terminal` in `src-tauri/src/runtime/mod.rs` alongside the existing methods, reusing the `podman`/`podman_on` helpers (lines 59–71). It must **not** use a login shell (the `NIX_REMOTE=daemon` gotcha) and does **not** use `flox activate` (Zellij is on the global `PATH` from Phase A). Steps:

1. `podman start <name>` (the container must be running), reusing the pattern from `flox_exec` (line 74).
2. Ensure the server: `podman exec <name> zellij web --status`; if it reports offline, `podman exec <name> zellij web --daemonize`. (Status parsing keys on the spike-observed strings "online"/"offline".)
3. Mint a token: `podman exec <name> zellij web --create-token`, parse stdout for the `token_<n>: <value>` line and capture `<value>`. Add a small parser; the token is a UUID on the line after the `:`.
4. Read the host port: `podman --connection <c> port <name> 8082/tcp`, parse the `127.0.0.1:<hostport>` output, and build `url = format!("http://127.0.0.1:{hostport}/")`.
5. Persist the token (the `docs/techdebt.md` workaround): `std::fs::create_dir_all(<workspace_host_path>/.mio)` then `std::fs::write(<workspace_host_path>/.mio/zellij.json, ...)` with the serialized `{ token, url }`. Writing host-side (the path is bind-mounted into the container at `/root/work/.mio/`) avoids shell-escaping JSON through `podman exec`.
6. Return `TerminalSession { url, token }`.

`RuntimeError` (`src-tauri/src/runtime/types.rs` lines 4–14) is reused as-is: failures surface as `CommandFailed`. Add a new variant only if port/token parsing needs a distinct message (e.g. `TerminalUnavailable(String)`); otherwise reuse `CommandFailed`.

### Tauri command and registration

Add to `src-tauri/src/lib.rs` (mirroring `install_package` at line 43):

```
#[tauri::command]
async fn open_terminal(name: String, workspace_path: String, runtime: State<'_, Arc<dyn RuntimeManager>>)
    -> Result<TerminalSession, AppError> { Ok(runtime.open_terminal(&name, &workspace_path).await?) }
```

The frontend already has `project.workspace_path` and passes it (as `list_packages` does).

Register `open_terminal` in the `tauri::generate_handler!` list (`src-tauri/src/lib.rs` lines 262–284). `AppError` already converts from `RuntimeError` via `#[from]` (see `src-tauri/src/error.rs`), so no error-mapping change is needed. `TerminalSession` is returned by value and serializes to `{ url, token }` for the frontend.

## Remaining: Phase D — Frontend button and webview window

### Capability

Add the webview-window creation permission to `src-tauri/capabilities/default.json` (currently `core:default`, `opener:default`, and the Mutagen shell rule). Add `core:webview:allow-create-webview-window` (and, if the new window needs runtime control such as close/focus, the relevant `core:window:*` permissions). The new `WebviewWindow` is created at runtime, so it is covered by the capability on the main window that calls the API.

### UI and command call

In `src/routes/projects/+page.svelte`:

- Add a "Terminal" `<button>` to the per-sandbox action row (the `{:else}` non-deployed branch around lines 350–356, next to Start/Stop/Deploy). Gate or label it for local (non-deployed) sandboxes per the remote-out-of-scope decision.
- Add an `async function openTerminal(project)` that `invoke<TerminalSession>("open_terminal", { name: project.name, workspacePath: project.workspace_path })`, then creates a `WebviewWindow` (import from `@tauri-apps/api/webviewWindow`) with a per-sandbox label (e.g. `terminal-${project.name}`) pointed at `session.url`; if a window with that label already exists, focus it instead of creating a duplicate.
- Surface `session.token` for the one-time login: display it in a small inline banner with a copy action (`navigator.clipboard` inside the app window) so the user can paste it into the web client's `/auth` page. (Auto-login injection is the deferred follow-up.)
- Wrap the call in the existing `try/catch` + `parseError(e).message` pattern used by the other actions (e.g. `installPackage`, lines 71–85).

### i18n

Add a `projects_terminal` key (and any token-banner strings) to every locale file under `messages/` (`en.json` first, then mirror into `de.json`, `ru.json`, per `AGENTS.md`). Call it as `m.projects_terminal()`.

## Lifecycle and Cross-Cutting Concerns

- **Web-server lifecycle.** The server is started on demand and persists until the container stops; it is not explicitly torn down. Because `open_terminal` is idempotent (it checks `--status` before `--daemonize`), reopening a terminal — or opening it after a container restart — re-attaches or restarts cleanly without spawning duplicates.
- **Token accumulation.** Each `open_terminal` mints a fresh token (the previous token's plaintext cannot be re-read, so reuse is impossible). Token hashes accumulate in Zellij's store (`/root/.local/share/zellij/tokens.db`, a SQLite DB in the container layer); this is acceptable for the MVP. A later cleanup could call `zellij web --revoke-all-tokens` before minting, at the cost of invalidating other open sessions. The DB persists across container restart but is lost on recreate, so a recreated sandbox simply mints anew.
- **Existing sandboxes do not gain a terminal automatically.** Bumping `BASE_IMAGE_VERSION` (Phase A) only affects newly created containers, and port mappings cannot be added to an existing container (Phase B applies at `create` time). Sandboxes created before this feature must be recreated to get both Zellij and the published port. This limitation is expected and should be surfaced in release notes / the e2e plan.
- **Loopback-only exposure.** Both the container bind (`127.0.0.1:8082`) and the host publish (`127.0.0.1::8082`) stay on loopback; nothing is exposed on the local network, and no TLS is configured (HTTP is acceptable over loopback only).
- **Window identity.** Using a deterministic per-sandbox `WebviewWindow` label keeps one terminal window per sandbox and lets a second click focus the existing window rather than stacking duplicates.
