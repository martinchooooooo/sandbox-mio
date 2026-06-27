# Remote Sandbox Terminal (Zellij via Mutagen Forward)

This document is the ideation for extending the in-app Zellij web terminal — already working for local sandboxes — to sandboxes deployed on a remote host. The terminal is provided by `zellij web` running inside the remote container; a Mutagen forwarding session carries traffic from a Windows-host loopback port to the remote container's web port, so the same `WebviewWindow` mechanism used locally loads the terminal without any new authentication or process-management infrastructure. Nothing for this case exists today: the remote container `create` call deliberately omits the port-publish and pasta-network flags (`src-tauri/src/backends/mod.rs`, deploy-time `create` near line 398), and no forwarding session is created at deploy time. The feature builds on four existing seams: the Mutagen sidecar already bundled and running for file sync (`src-tauri/src/sync/mod.rs`), the SSH alias written at deploy time (`src-tauri/src/ssh/mod.rs`), the `open_terminal` runtime operation and Tauri command already working for local sandboxes (`src-tauri/src/runtime/mod.rs:206`, `src-tauri/src/lib.rs:63`), and the `WebviewWindow` open path in the frontend (`src/routes/projects/SandboxItem.svelte:172`).

## Current State

The local terminal feature (Phases A–D of `docs/features/web-terminal/02_dev.md`) is fully implemented. The Tauri command `open_terminal(name, workspace_path)` returns a `TerminalSession { url, token }`, which the frontend uses to open a `WebviewWindow`. The command lives in `src-tauri/src/runtime/mod.rs:206` and is gated to local containers: it reads the published host port with `podman port <name> 8082/tcp`, which resolves to `127.0.0.1:<ephemeral>` on the Windows host only for local containers.

For remote sandboxes, the deploy flow (`PodmanCliBackend::deploy()` in `src-tauri/src/backends/mod.rs:380`) creates a remote container via `podman --connection <target> create ...` — intentionally without `--publish` or the pasta network flag, since no forwarding mechanism existed to expose the port. The remote container has Zellij baked in (same base image) but its web port is unreachable from the Windows host.

The SSH alias for each deployment is written to `~/.ssh/sandbox-mio/config` by `write_alias()` in `src-tauri/src/ssh/mod.rs` at deploy time and remains for the deployment's lifetime. Mutagen uses this alias (as `<alias>:<remote_path>`) for its scp-style sync URL, proving the alias is valid and the SSH key authenticates for the full lifetime of the sync session.

The bundled Mutagen sidecar is version 0.18.1 (`src-tauri/binaries/mutagen-x86_64-pc-windows-msvc.exe`). Alongside `mutagen sync`, it ships a fully independent `mutagen forward` subsystem with `create`, `list`, `pause`, `resume`, and `terminate` subcommands. Forwarding sessions are managed by the same daemon process and respect the same `MUTAGEN_DATA_DIRECTORY` isolation as sync sessions, so the existing `command()` helper in `src-tauri/src/sync/mod.rs` can drive both.

A sandbox is identified as deployed (remote) by the presence of a Mutagen sync session sharing its name (`sandbox_deployments()` in `src-tauri/src/lib.rs`, querying `SyncEngine::sessions()`). There is no `is_remote` field; the session list is the ground truth. The `SessionInfo` struct (`src-tauri/src/sync/types.rs`) carries the remote host alias alongside the sync status, giving `open_terminal` everything it needs to open the forwarding session.

## Proposed Model

### Port Publishing on the Remote Container

The remote `create` call (near `src-tauri/src/backends/mod.rs:398`) must publish `127.0.0.1::8082` and set `--network pasta:--host-lo-to-ns-lo` on the remote container, for the same reasons as the local container: Zellij binds container-loopback `127.0.0.1:8082`, and pasta's default routes published ports to `eth0`, not loopback. This change mirrors Phase B of the local build plan exactly. The assigned ephemeral port on the **remote host's loopback** is read back with `podman --connection <target> port <name> 8082/tcp`.

### Mutagen Forwarding Session

A Mutagen forwarding session bridges the remote host port to a Windows-host loopback port:

```
mutagen forward create \
  --name=terminal-<sandbox-name> \
  tcp:127.0.0.1:<windows_port> \
  <alias>:tcp:127.0.0.1:<remote_zellij_port>
```

`<alias>` is the SSH alias already written to `~/.ssh/sandbox-mio/config`. `<remote_zellij_port>` is the ephemeral port read from `podman --connection <target> port`. `<windows_port>` is an ephemeral port allocated by binding `TcpListener::bind("127.0.0.1:0")` in Rust, reading the OS-assigned port, and dropping the listener before Mutagen binds it. The `WebviewWindow` then opens `http://127.0.0.1:<windows_port>/` — the same shape as the local terminal URL.

The forwarding session is created idempotently: any stale session named `terminal-<name>` is terminated first, then recreated (mirroring how `SyncEngine::create` works in `src-tauri/src/sync/mod.rs:166`). On a second `open_terminal` click the Mutagen daemon is already forwarding, so `mutagen forward list terminal-<name>` can detect the active session and return the cached Windows port without recreating it.

### Forwarding Lifecycle Tied to Sync Lifecycle

The forwarding session is a peer to the sync session: it is created when the terminal is first opened for a deployed sandbox and terminated when the sandbox is undeployed or destroyed. `undeploy_sandbox` and `destroy_sandbox` in `src-tauri/src/lib.rs` already call `SyncEngine::terminate(name)` to end the sync session; they are extended to also call `mutagen forward terminate terminal-<name>` before tearing down the remote container. No new Tauri state is needed to track the tunnel: the Mutagen daemon owns the session's lifetime, and the session name encodes the sandbox name, so the cleanup call is stateless.

### Unified `open_terminal` Command

The existing `open_terminal` Tauri command (`src-tauri/src/lib.rs:63`) is extended rather than split. The runtime implementation detects whether the named sandbox is deployed by consulting `SyncEngine::sessions()`: if a session named `<name>` exists, the sandbox is remote; otherwise it is local. The two code paths share the Zellij start-server, mint-token, and build-URL steps; they differ only in how the Windows-side port is obtained (local: `podman port` on the default connection; remote: `podman --connection <target> port` then a Mutagen forwarding session).

### Frontend — Remote Terminal Button Visibility

The existing "Terminal" button in `src/routes/projects/SandboxItem.svelte` is gated to local (non-deployed) sandboxes. This gate is removed: the button appears for both local and deployed sandboxes. The command response shape (`TerminalSession { url, token }`) is unchanged, so the token-copy banner and `WebviewWindow` open code require no structural change.

## Technology Choices with Tradeoffs

### Port-Forward Transport: Mutagen `forward` Subsystem

Mutagen 0.18.1 ships `mutagen forward create <source> <destination>` which accepts TCP endpoints in the form `tcp:127.0.0.1:<port>` (local listener) and `<ssh-alias>:tcp:127.0.0.1:<port>` (remote connect). The bundled sidecar is confirmed to have this subcommand (`mutagen forward --help` lists `create`, `list`, `pause`, `resume`, `terminate`). The Mutagen daemon already runs for file sync; forwarding sessions share that daemon, the same `MUTAGEN_DATA_DIRECTORY` isolation, and the same SSH alias resolution. This means no new authentication code, no new binary dependency, no new process lifecycle in Tauri state, and automatic reconnection on network drop.

Alternatives evaluated:

- **System `ssh -N -L` subprocess** — rejected because it requires the Tauri app to manage a `tokio::process::Child` per sandbox in a `TunnelRegistry` Tauri state struct. On Windows, child processes without `kill_on_drop(true)` survive the parent process, leaving orphaned `ssh` processes on crash. Reconnection after a network drop requires detecting process death and respawning. Mutagen's daemon handles all of this transparently as a named, persistent session.

- **`russh` or `ssh2` Rust crate (native SSH in-process)** — rejected because both require reimplementing host-alias lookup, IdentityFile loading, and StrictHostKeyChecking behaviour that the system SSH client handles via `sandbox-mio/config`. Mutagen already abstracts over all of this.

- **`socat` over `podman exec`** — rejected because `socat` is not in the base image and installing it at tunnel-open time is a per-request dependency fetch with no lockfile.

- **Windows-native TCP relay in the Tauri process (`tokio::net::TcpListener` proxy)** — rejected because it requires the app to stream all terminal I/O (WebSocket frames) through userland, adding CPU overhead and latency. Both Mutagen forwarding and SSH tunneling kernel-forward packets below the app layer; the Tauri process is not in the data path.

### Port Allocation: Ephemeral OS Port via `TcpListener::bind("127.0.0.1:0")`

An ephemeral Windows-side port is allocated by binding a `TcpListener` to port 0, reading the OS-assigned port, and immediately dropping the listener before Mutagen binds it. This sidesteps a port-bookkeeping table, mirroring how the local design uses `--publish 127.0.0.1::8082` (Podman assigns the host port ephemerally).

Alternatives evaluated:

- **Fixed per-sandbox port derived from a hash or sequential counter** — rejected because two sandboxes could collide if the same counter slot is reused after a destroy/recreate cycle, and a persistent port table must survive app restarts. OS ephemeral assignment is collision-free without any bookkeeping.

- **Mutagen source port 0 (let Mutagen assign)** — not confirmed; the Mutagen `forward create` source spec `tcp:127.0.0.1:<port>` takes a port number, and it is unknown whether port `0` is accepted as an OS-assign hint. Confirmed via the `TcpListener` approach instead, which is identical in cost and avoids the unknown.

### Forwarding Session Lifecycle: Stateless Cleanup via Mutagen Named Sessions

Forwarding sessions are named `terminal-<sandbox-name>` and terminated by name at undeploy/destroy time. No Tauri state struct is needed: the Mutagen daemon is the source of truth. This is the same pattern used for sync sessions: `SyncEngine::terminate(name)` is a stateless `mutagen sync terminate <name>` call. Adding `mutagen forward terminate terminal-<name>` alongside it requires no new state.

Alternative evaluated:

- **Tauri-state `TunnelRegistry` mapping sandbox name to a held forwarding session handle** — rejected as unnecessary. A registry is needed when the app process owns the object (a child process, a socket, a file handle); when the Mutagen daemon owns the session, the app only needs the session name to interact with it, and the name is derivable from the sandbox name at any call site.

## Validation

No spike has been run for this feature yet. The following risks should be confirmed before the build plan is written:

1. **`mutagen forward create tcp:127.0.0.1:<port> <alias>:tcp:127.0.0.1:<remote_port>` works with a sandbox-mio alias.** The alias is known to authenticate for Mutagen sync; forwarding has not been exercised. The spike should confirm the session reaches `Connected` status and that `curl http://127.0.0.1:<port>/` from Windows returns the Zellij web client HTML.

2. **`podman --connection <target> port <name> 8082/tcp` returns the ephemeral remote port.** The local equivalent is proven (`podman port <name> 8082/tcp` → `127.0.0.1:<port>`); the remote query uses the same syntax with `--connection`.

3. **`pasta:--host-lo-to-ns-lo` behaves identically on the remote host's Podman.** The local spike confirmed this flag is required; the remote host may run a different Podman or pasta version. The base image version is reconciled by `ensure_image_on` at deploy time; pasta version compatibility is unconfirmed.

4. **The Zellij web client served over the Mutagen forwarding session renders and authenticates correctly.** The token mint and paste flow is already proven locally; via forwarding the URL shape is `http://127.0.0.1:<windows_port>/`, identical. The spike should confirm a browser (or WebviewWindow) can complete the Zellij login through the forwarded port.

## Open Questions

- **Whether Mutagen forward accepts `tcp:127.0.0.1:0` as a source.** If the Mutagen source spec accepts port 0 as an OS-assign hint (like `TcpListener::bind("0.0.0.0:0")`), the `TcpListener` pre-allocation step is unnecessary. If not, pre-allocation with the tiny race window is the path. This is resolvable in the spike.

- **Remote port persistence across container restarts.** The ephemeral port published by the remote container is stable for the container's lifetime but changes if the container is recreated. If the user stops and restarts the remote container (not recreates it), the port mapping is preserved and the existing Mutagen forwarding session continues to work. Whether `open_terminal` should always re-query `podman port` and recreate the forwarding session defensively, or trust the cached session, is undecided.

- **Forwarding session status in the UI.** Mutagen sync status is already surfaced in the frontend (`sandbox_deployments()` returns `Vec<SessionInfo>` with `SyncStatus`). Whether the forwarding session status (connected/disconnected/error) should also be surfaced — and if so, as part of the existing `SessionInfo` or a separate event — is undecided.

- **`AllowTcpForwarding` on managed remote hosts.** Mutagen forwarding uses the same SSH transport as sync, so if Mutagen sync works, forwarding is likely to work too — but `AllowTcpForwarding no` in `sshd_config` could block only the port-forward step while leaving file sync unaffected. Whether the app should detect this and surface a useful error is undecided.

- **HTTPS for the remote terminal.** Over the Mutagen forwarding session all traffic is encrypted at the SSH layer, so plain HTTP inside the tunnel is confidential. Whether to configure Zellij with TLS (for parity with a hypothetical future direct-access path) is undecided. The local terminal uses plain HTTP over loopback and the same reasoning applies here.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `podman port` returns nothing for the remote container | The remote `create` call omits `--publish 8082` (intentionally left out in the local-only MVP, `src-tauri/src/backends/mod.rs` near line 398) | Add `--publish 127.0.0.1::8082` and `--network pasta:--host-lo-to-ns-lo` to the remote `create` call, mirroring Phase B of the local build plan |
| Mutagen forwarding session stays `Connecting` indefinitely | The forwarding destination port does not exist yet (Zellij not started), or `AllowTcpForwarding` is disabled on the remote `sshd` | Ensure `zellij web --daemonize` runs before the forwarding session is created; check `sshd_config` on the remote host |
| `open_terminal` creates a duplicate forwarding session | The existing session was not detected before recreating | Call `mutagen forward list terminal-<name>` before `forward create`; terminate the old session only if it exists, then create fresh |
| Forwarding session orphaned after undeploy/destroy | `mutagen forward terminate terminal-<name>` not called before removing the remote container | Extend both `undeploy_sandbox` and `destroy_sandbox` in `src-tauri/src/lib.rs` to terminate the forwarding session by name before tearing down the sync session |
| Windows-side port collides with another service | OS ephemeral port was reassigned between `TcpListener` drop and Mutagen bind | Retry port pre-allocation; the race window is small on loopback but non-zero |
| `pasta:--host-lo-to-ns-lo` unrecognised on remote Podman | Remote host runs an older Podman or pasta version | Confirm pasta version at deploy time (`ensure_image_on` is the natural hook) |
| Remote terminal button visible but fails for local sandboxes | Gate logic incorrectly branches to the remote path | Gate the Mutagen-forward path strictly on `SyncEngine::sessions()` containing the sandbox name, not on container status |
| Zellij token written to workspace and synced to remote | `docs/techdebt.md` workaround writes plaintext token to `<workspace>/.mio/zellij.json`, which Mutagen syncs bidirectionally | Same known tech debt as the local terminal; the remote case does not worsen it |
