# Remote Sandbox Terminal — Build Plan

This document is the build plan for the remote-sandbox terminal feature whose design is in `docs/features/remote-terminal/01_ideas.md`. The feature extends the in-app Zellij web terminal to both local and deployed sandboxes via a **single code path**: a Mutagen forwarding session bridges a Windows-host loopback port to the Zellij web server inside the sandbox container, regardless of whether that container is local or remote. All five phases (A–E) are Remaining; nothing in this feature is implemented yet.

## The Concrete Model

The `open_terminal` Tauri command (`src-tauri/src/lib.rs:63`) is rebuilt around one path that works for every sandbox. The key insight is that the local Podman machine (`podman-machine-default`) is already an SSH-accessible endpoint — the same alias that Mutagen uses for file sync on local deployments — so `mutagen forward` can bridge to it just as it bridges to a remote host.

The command determines the Podman connection to use: if a Mutagen sync session named after the sandbox exists (meaning the sandbox is deployed), its `host` field is the connection alias; otherwise the local default connection (`backends::DEFAULT_CONNECTION = "podman-machine-default"`) is used. With the connection known, a single runtime method `RuntimeManager::prepare_terminal(name, connection, workspace_host_path)` starts the Zellij web server via `podman --connection <connection> exec`, mints a one-time token, and reads the port that the container published for `8082/tcp` via `podman --connection <connection> port`. That port is on the **target host's** loopback (the VM loopback for local, the remote-host loopback for deployed). A Mutagen forwarding session named `terminal-<name>` then bridges a Windows-host ephemeral port to that target-host port. The command returns `TerminalSession { url: "http://127.0.0.1:<windows_port>/", token }` — unchanged shape, unchanged frontend.

The existing `RuntimeManager::open_terminal` method, which was local-only and returned a ready `TerminalSession`, is **removed** and replaced by `prepare_terminal`. The assembly of `TerminalSession` (including the Mutagen forward step) moves to the command layer in `lib.rs`.

### Port reachability assumption for the local path

For local sandboxes the forwarding destination is `podman-machine-default:tcp:127.0.0.1:<port>` where `<port>` is what `podman port` reports. The web-terminal spike (`docs/features/web-terminal/01_ideas.md` Validation) confirmed that `Invoke-WebRequest http://127.0.0.1:<port>/` from Windows succeeds — meaning gvproxy exposes the same port number on Windows loopback as pasta assigns on the VM loopback (identity port mapping). Mutagen's SSH session into the VM then reaches the same port on the VM side. **This assumption must be confirmed in a spike before the build begins** (see Validation below).

## Resolved Decisions

- **Unified code path.** A single `prepare_terminal` method and a single Mutagen forward step handle both local and deployed sandboxes. There is no `if deployed` branch inside `open_terminal`.
- **`RuntimeManager::open_terminal` is removed.** The local-only implementation is replaced by `prepare_terminal`, which accepts an explicit `connection` parameter and works for any Podman connection. Any caller of the old method must be updated (only `src-tauri/src/lib.rs:68` calls it).
- **gvproxy bypassed for local terminal I/O.** Local terminal traffic now flows Windows → Mutagen SSH → VM → pasta → container instead of Windows → gvproxy → VM → pasta → container. This adds one SSH hop for local sandboxes but eliminates the local/remote branch. The tradeoff is acceptable.
- **`--publish` and pasta flags remain on both local and remote creates.** Zellij binds `127.0.0.1:8082` inside the container; pasta's `--host-lo-to-ns-lo` makes that port reachable on the target host's loopback. Without both flags, Mutagen's forwarding destination would have nothing to connect to.
- **Whether Mutagen source spec accepts port `0`.** Not confirmed; `TcpListener::bind("127.0.0.1:0")` is used to obtain the Windows-side port before calling Mutagen.
- **Remote port persistence across container restarts.** `prepare_terminal` re-queries `podman --connection <connection> port` on every call. If the port changed (container recreated), `forward_create` terminates the old session and creates a new one.
- **Forwarding session status in the UI.** Deferred; out of scope for this iteration.
- **`AllowTcpForwarding` on managed remote hosts.** If Mutagen sync works, the same SSH channel supports TCP forwarding. A `mutagen forward create` failure surfaces as `RuntimeError::TerminalUnavailable` with Mutagen's stderr.
- **HTTPS.** Plain HTTP, same as the local terminal. All traffic is encrypted by Mutagen's SSH transport.

## Validation Required Before Build

The following spike item must be confirmed before Phase B is written into code. The local-path unification depends on it.

**Confirm gvproxy identity port mapping.** Create a local sandbox container with `--publish 127.0.0.1::8082 --network pasta:--host-lo-to-ns-lo`. Start Zellij inside it. Read the port with `podman --connection podman-machine-default port <name> 8082/tcp` (returns `127.0.0.1:<port>`). Then run:

```
mutagen forward create --name=spike-test tcp:127.0.0.1:19999 podman-machine-default:tcp:127.0.0.1:<port>
```

Fetch `http://127.0.0.1:19999/` from Windows. If it returns the Zellij web client HTML, the assumption holds and gvproxy uses identity mapping. Record the date and result in the `01_ideas.md` Validation section. Terminate the session afterward with `mutagen forward terminate spike-test`.

## Remaining: Phase A — Publish the Zellij Port on the Remote Container

Edit `deploy` in `src-tauri/src/backends/mod.rs:380`. The remote `podman create` call at line 426 currently passes:

```rust
&["create", "--tty", "--volume", &volume, "--workdir", WORKSPACE_MOUNT, "--name", name, &image, "/bin/sh"]
```

Add the same `--network` and `--publish` flags already present on the local `create` at line 322:

```rust
let publish = format!("127.0.0.1::{ZELLIJ_WEB_PORT}");
// insert before --volume:
"--network", "pasta:--host-lo-to-ns-lo", "--publish", &publish,
```

`ZELLIJ_WEB_PORT` (`8082`) is already defined at `src-tauri/src/backends/mod.rs:42`. Remove the comment on the local `create` call (line 317) that says "Only local sandboxes get this; the remote deploy `create` path is intentionally left without it" — the rationale is now reversed.

## Remaining: Phase B — Mutagen Forward Helpers

Add two free functions to `src-tauri/src/sync/mod.rs` alongside the existing `run` helper. Both use the existing `command()` builder and inherit `MUTAGEN_DATA_DIRECTORY` automatically.

```rust
/// Start a Mutagen forwarding session named `name`, listening on `local_port` on the Windows
/// host and forwarding to `remote_port` on `remote_alias`'s loopback. Terminates any prior
/// session of the same name first so the call is idempotent.
pub async fn forward_create<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    local_port: u16,
    remote_alias: &str,
    remote_port: u16,
) -> Result<(), SyncError> {
    let _ = run(app, &["forward", "terminate", name]).await; // best-effort; absent session is not an error
    let source = format!("tcp:127.0.0.1:{local_port}");
    let dest = format!("{remote_alias}:tcp:127.0.0.1:{remote_port}");
    run(app, &["forward", "create", &format!("--name={name}"), &source, &dest])
        .await
        .map(|_| ())
}

/// Terminate the Mutagen forwarding session named `name`. Best-effort: a missing session is not
/// an error.
pub async fn forward_terminate<R: Runtime>(app: &AppHandle<R>, name: &str) -> Result<(), SyncError> {
    let _ = run(app, &["forward", "terminate", name]).await;
    Ok(())
}
```

No trait is introduced. Forwarding is not polymorphic; `MutagenCli` is the only backend and calling these directly keeps the implementation flat.

## Remaining: Phase C — Replace `open_terminal` with `prepare_terminal` in the Runtime

### Trait change (`src-tauri/src/runtime/types.rs`)

**Remove** the `open_terminal` declaration (line 63). Add `prepare_terminal` in its place:

```rust
/// Start the zellij web server in sandbox `name` over `connection` (idempotently), mint a
/// one-time login token, and return `(token, container_port)` where `container_port` is the
/// ephemeral port on `connection`'s host loopback that the container published for the zellij
/// web server. The caller creates the Mutagen forwarding session that bridges a Windows-host
/// port to `container_port`. `workspace_host_path` is used only for the token-persistence
/// workaround (docs/techdebt.md). Works for both local (`podman-machine-default`) and remote
/// (any registered connection alias) containers.
async fn prepare_terminal(
    &self,
    name: &str,
    connection: &str,
    workspace_host_path: &str,
) -> Result<(String, u16), RuntimeError>;
```

Remove `TerminalSession` from `types.rs` — it is no longer returned by the runtime. It moves to `lib.rs` as a command-layer type (or stays in `types.rs` as a public struct used only by the command layer; either is fine — keep it in `types.rs` to avoid moving imports).

### Implementation (`src-tauri/src/runtime/mod.rs`)

**Remove** `open_terminal` from `impl RuntimeManager for PodmanMiseRuntime` (lines 206–264). Add `prepare_terminal`:

```rust
async fn prepare_terminal(
    &self,
    name: &str,
    connection: &str,
    workspace_host_path: &str,
) -> Result<(String, u16), RuntimeError> {
    // Start container, ensure Zellij web server is up, mint token, read port.
    // All podman calls use podman_on(connection, ...) — identical to reproduce_remote's pattern.
```

Steps, using `Self::podman_on(connection, ...)` throughout (same pattern as `reproduce_remote` at line 168):

1. `Self::podman_on(connection, &["start", name])` — start the container.
2. Check `zellij web --status` via `Self::podman_on(connection, &["exec", name, "zellij", "web", "--status"])`; inspect combined stdout+stderr for `"online"` (same logic as the removed `open_terminal`). If offline, daemonize.
3. Mint token: `Self::podman_on(connection, &["exec", name, "zellij", "web", "--create-token"])`; parse with existing `parse_token()`.
4. Read port: `Self::podman_on(connection, &["port", name, &format!("{ZELLIJ_WEB_PORT}/tcp")])`; parse with existing `parse_host_port()` which extracts the number from `127.0.0.1:<port>`. Return `RuntimeError::TerminalUnavailable` if absent.
5. Write token host-side to `<workspace_host_path>/.mio/zellij.json` (same `docs/techdebt.md` workaround).
6. Return `(token, port_as_u16)`.

`parse_host_port` currently returns `Option<String>` (the port as a string). Parse that string to `u16` before returning it; add a `RuntimeError::TerminalUnavailable` for a parse failure.

The `container_exec` helper at line 118 uses `self.podman(...)` (local connection only) and is not reused here. Inline the exec calls as `Self::podman_on(connection, &["exec", name, ...])`.

## Remaining: Phase D — `open_terminal` Command Rebuilt

### `ForwardPortMap` Tauri state (`src-tauri/src/lib.rs`)

Add a type alias and register it alongside the other state singletons:

```rust
type ForwardPortMap = Arc<Mutex<std::collections::HashMap<String, u16>>>;
```

Register in the `setup` closure (line 266), after the sync engine:

```rust
app.manage(ForwardPortMap::default());
```

`std::sync::Mutex` must be imported. `Arc` is already in scope.

### Updated `open_terminal` command

Replace the current command body. The signature gains three parameters:

```rust
#[tauri::command]
async fn open_terminal(
    name: String,
    workspace_path: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    forward_ports: State<'_, ForwardPortMap>,
    app: tauri::AppHandle,
) -> Result<TerminalSession, AppError> {
    // Determine the Podman connection: deployed sandbox → its session's host alias;
    // local sandbox → the local machine's default connection.
    let connection = engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.host)
        .unwrap_or_else(|| crate::backends::DEFAULT_CONNECTION.to_string());

    let (token, container_port) = runtime
        .prepare_terminal(&name, &connection, &workspace_path)
        .await?;

    // Allocate an ephemeral Windows-side port by binding and immediately releasing it.
    let windows_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| RuntimeError::TerminalUnavailable(e.to_string()))?;
        listener.local_addr()
            .map_err(|e| RuntimeError::TerminalUnavailable(e.to_string()))?.port()
        // listener is dropped here, releasing the port before Mutagen binds it
    };

    let forward_name = format!("terminal-{name}");
    sync::forward_create(&app, &forward_name, windows_port, &connection, container_port)
        .await
        .map_err(|e| RuntimeError::TerminalUnavailable(e.to_string()))?;
    forward_ports.lock().unwrap().insert(name.clone(), windows_port);

    Ok(TerminalSession {
        url: format!("http://127.0.0.1:{windows_port}/"),
        token,
    })
}
```

`AppError` does not need a new variant: `RuntimeError::TerminalUnavailable` already covers `TcpListener` and `forward_create` failures, and `AppError` converts from `RuntimeError` via `#[from]` (see `src-tauri/src/error.rs`). Wrap the `SyncError` from `forward_create` into `RuntimeError::TerminalUnavailable` as shown above.

### Cleanup in `undeploy_sandbox` and `destroy_sandbox`

Both commands gain `forward_ports: State<'_, ForwardPortMap>` and `app: tauri::AppHandle` parameters. Add `forward_terminate` + map removal immediately before each `engine.terminate` call.

In `undeploy_sandbox` (line 133, before `engine.terminate`):

```rust
sync::forward_terminate(&app, &format!("terminal-{name}")).await;
forward_ports.lock().unwrap().remove(&name);
engine.terminate(&name).await?;
```

In `destroy_sandbox` (line 169, before `engine.terminate`):

```rust
sync::forward_terminate(&app, &format!("terminal-{name}")).await;
forward_ports.lock().unwrap().remove(&name);
engine.terminate(&name).await?;
```

Both `forward_terminate` calls are best-effort (their `Result` is already discarded inside the function). The map removal is infallible.

## Remaining: Phase E — Frontend Terminal Button for Deployed Sandboxes

The Terminal button in `src/routes/projects/SandboxItem.svelte:238` is currently inside the `{:else}` branch (non-deployed only). Move it to appear in both branches by adding it to the `{#if deployment}` block (line 226):

```svelte
{#if deployment}
    <label class="flex items-center gap-1 text-xs">
        <Switch checked={syncOn} onCheckedChange={toggleSync} />
        {m.projects_sync()}
    </label>
    <button onclick={openTerminal}>{m.projects_terminal()}</button>   <!-- add here -->
    <button onclick={undeploy}>{m.projects_undeploy()}</button>
```

The `openTerminal` function (`src/routes/projects/SandboxItem.svelte:172`) is unchanged — it invokes `open_terminal` with `name` and `workspacePath` and opens a `WebviewWindow`. The command now handles both paths identically from the frontend's perspective.

No i18n changes needed: `projects_terminal`, `projects_terminal_token`, and `projects_terminal_copy` keys exist in all locale files.

## Lifecycle and Cross-Cutting Concerns

- **`open_terminal` idempotency.** `sync::forward_create` terminates any prior forwarding session of the same name before creating a new one. On a second "Terminal" click the old session is replaced, the `ForwardPortMap` entry is updated to the new Windows port, and the `WebviewWindow` open logic in the frontend focuses the existing window (same label). The existing window loses its connection and must be reopened — acceptable for the MVP; improving this would require detecting a live session via `mutagen forward list` and reusing the stored port.

- **App exit.** `sync::shutdown` (`src-tauri/src/lib.rs:303`) calls `mutagen daemon stop` on `tauri::RunEvent::Exit`. Stopping the Mutagen daemon terminates all its sessions — sync and forward — so forwarding sessions are not orphaned on a clean exit.

- **App crash.** The Mutagen daemon survives a crash. Its forwarding sessions persist and are cleaned up on the next `open_terminal` call (which calls `forward_create`, which terminates the old session by name) or on the next `undeploy_sandbox`/`destroy_sandbox`.

- **`ForwardPortMap` on restart.** The map starts empty each launch. Forwarding sessions left by a prior run are cleaned up lazily: the next `forward_create` call terminates them by name before recreating. The map only needs to hold live ports for the current run's `undeploy`/`destroy` cleanup path.

- **Zellij token persistence (existing tech debt).** Unchanged: plaintext token written to `<workspace>/.mio/zellij.json` for both local and remote terminals, as documented in `docs/techdebt.md`.

- **Remote container start.** `prepare_terminal` calls `podman --connection <connection> start <name>` as its first step. For the remote container this is correct and expected — the remote container must be running for `exec` to work. The local container is always running (it was stopped at deploy time and kept stopped; the terminal call starts it).

Wait — for a deployed sandbox, the local container is kept **stopped** after deploy (the deploy flow stops it). The remote container is running. `prepare_terminal` is called with the **remote** connection, so it starts and execs the **remote** container. The stopped local container is untouched. This is the correct behavior.
