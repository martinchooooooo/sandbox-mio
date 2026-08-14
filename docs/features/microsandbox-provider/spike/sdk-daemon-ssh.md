# Spike: Rust SDK, the daemon question, SSH serve, and snapshots

Desk spike run 2026-08-09 against microsandbox `0.6.8` (published to crates.io 2026-07-29) and `superradcompany/microsandbox` at `main` (last push 2026-08-09). No `msb` binary was installed and nothing was compiled; every finding below is either a documentation quote or a read of upstream source fetched through the GitHub raw/contents API. The `gh` CLI is not installed on this host, so repository reads used `https://api.github.com/repos/superradcompany/microsandbox/...` and `https://raw.githubusercontent.com/superradcompany/microsandbox/main/...` directly.

This document answers only the eight questions assigned to it. It does not modify `01_ideas.md`, and where a finding contradicts `01_ideas.md` the contradiction is called out explicitly so the ideation document can be corrected separately.

## Summary

**Recommendation: keep the `msb` CLI as the v1 driver, and drop the daemon concern entirely — there is no daemon to supervise.**

The daemon contradiction recorded in `01_ideas.md` is resolved in favour of the README. Microsandbox has no host daemon today. The `microsandbox-server` crate that the SDK-overview quote described was last published as `0.2.6` on 2025-06-05 and no longer exists in the repository; the current architecture (`0.6.x`) spawns one microVM child process per sandbox from whichever process calls the API. Nothing in the app needs to start, supervise, health-check, or port-allocate a background service.

The CLI-versus-SDK decision is no longer a bet on an ambiguous process model, because the two are the same code. `crates/cli/Cargo.toml` declares `microsandbox.workspace = true`: the `msb` binary is a thin `clap` front end over the exact SDK crate, and every CLI command routes through SDK entry points such as `Sandbox::get(name)`. Interop is therefore total and structural rather than merely documented, and the argument in `01_ideas.md` for keeping the `ProcessRunner` seam and the `light` e2e tier stands on its own merits with no correctness risk attached.

Three findings argue against adopting the SDK for v1 even though its capability surface is strictly larger:

- **Windows is the wrong platform for the SDK today.** `sdk/rust/build.rs` refuses to install the prebuilt runtime on Windows and emits `cargo:warning=skipping microsandbox prebuilt runtime install on Windows; build msb/libkrunfw locally`. Linking the SDK into `src-tauri` on Windows therefore leaves the developer responsible for building `msb.exe` and `libkrunfw.dll` by hand. The `msb` installer script (`irm https://install.microsandbox.dev/windows | iex`) has no such gap.
- **The dependency footprint is larger than the entire current app.** The `microsandbox` crate's transitive closure in the upstream `Cargo.lock` is 687 packages, including `sea-orm` 2.0 plus `sqlx` with the SQLite driver (an embedded database), `rustls`, `russh`, and a second major version of `reqwest` (0.13 against our 0.12). It is edition 2024, so the toolchain floor rises to Rust 1.85 for a crate whose own `src-tauri/Cargo.toml` is still edition 2021.
- **Adopting the SDK deletes the `light` e2e tier for the microsandbox provider,** exactly as `01_ideas.md` argues, and there is now no compensating correctness benefit to weigh against it.

The SDK's genuine advantages should be recorded as v2 levers rather than v1 requirements: a real guest filesystem API (`sb.fs()`), incremental piped stdin without shelling out, per-sandbox SSH authorized-key scoping, and an in-process SSH server that removes the supervised-child-process problem from the Mutagen path altogether.

For SSH specifically, prefer `msb ssh serve --stdio` as a `ProxyCommand` over the TCP listener. It is a per-connection transport bridge that OpenSSH spawns and reaps itself, so the app supervises nothing, allocates no port, and cannot leak a listener. That directly retires the `01_ideas.md` open question about "a long-lived `msb ssh serve` process per sandbox that the app must supervise". Mutagen drives an ordinary `ssh` binary, so a `~/.ssh/config` host alias is all it needs.

One incidental confirmation outside the assigned scope, because it closes a load-bearing `01_ideas.md` unknown: `resolve_home()` in `crates/utils/lib/lib.rs` reads `MSB_HOME` as the root of *all* microsandbox state, with the doc comment "`MSB_HOME` lets CI and integration tests isolate microsandbox state (db, sandboxes, cache, logs) per process". Per-worktree e2e isolation is viable.

## 1. The daemon contradiction

**CONTRADICTED — the SDK-overview quote in `01_ideas.md` no longer reflects upstream. There is no daemon. The README is correct.**

The current SDK overview states the opposite of what `01_ideas.md` records: "The SDK embeds the microsandbox runtime directly into your application. Running locally, creating a sandbox spawns a VM as a child process; there is no daemon to install and no server to connect to." (https://docs.microsandbox.dev/sdk/overview.md). The introduction page agrees: "**Local runtime.** The SDK starts the sandbox process directly. No daemon, remote service, or infrastructure setup." (https://docs.microsandbox.dev/getting-started/introduction.md). The README claim quoted in `01_ideas.md` — "No setup server. No long-running daemon." — is still present at line 37 of `README.md` and is the accurate one.

Source confirms the docs rather than merely repeating them. `sdk/rust/lib/backend/mod.rs` opens with "Backend abstraction: routes SDK calls to either a local libkrun runtime or a remote msb-cloud control plane", and its `BackendKind` enum documents the local variant as "Local libkrun + agentd backend. Spawns microVMs on the calling host." The only two backends are `LocalBackend` (in-process libkrun) and `CloudBackend` (HTTP to microsandbox cloud, gated on `MSB_API_KEY`). `sdk/rust/lib/runtime/mod.rs` is described as "Sandbox process management" and exports `spawn_sandbox`; the sibling `spawn.rs` is 155 KB of child-process launch logic and `reap.rs` handles orphan cleanup. A daemon-client SDK would need neither.

The archaeology explains the stale quote. A `microsandbox-server` crate does exist on crates.io, but its newest version is `0.2.6`, published 2025-06-05, and the crates.io siblings `microsandbox-core` and `microsandbox-portal` are frozen at the same `0.2.6`. The repository's `crates/` directory today contains `agentd cli db filesystem image metrics metrics-collector migration network protocol runtime test-init test-macros test-utils utils` — no `server`. The daemon belonged to the abandoned `0.2.x` architecture, and documentation describing it predates the `0.6.x` rewrite.

The one socket in the picture is not a daemon. `sdk/rust/Cargo.toml` depends on `microsandbox-agent-client` with features `named-pipe` and `uds`, and `sdk/rust/lib/runtime/mod.rs` exposes `agent_socket_path(name)` returning "the host-side path of a sandbox's agentd relay socket". That endpoint is per-sandbox and is created by the sandbox's own child process; `agentd` is the guest-side PID 1, not a host service. No fixed port, no shared socket, no auto-start, nothing to health-check.

**Consequence for the app: `MsbProvider` supervises no background process.** The `ProviderCapabilities` struct needs no daemon-readiness flag, and `MachineManager`'s microsandbox readiness steps stay as `01_ideas.md` proposes them (binary present, `HypervisorPlatform` enabled, `msb doctor` clean) with nothing added.

## 2. Rust crate details

**CONFIRMED — crate `microsandbox` 0.6.8; no declared MSRV but edition 2024 forces Rust 1.85+; no tonic/prost; hyper/tower do arrive via reqwest; Windows compiles but the prebuilt runtime install is skipped.**

Crate name is `microsandbox`, installed with `cargo add microsandbox`. crates.io reports `max_version` `0.6.8`, published 2026-07-29, 21,208 downloads, edition 2024. Version `0.6.5` is yanked. The workspace pins all sibling crates with exact `=0.6.8` requirements and the manifest comment explains why: "these crates share private, unstable APIs across patch releases, so every published version must resolve its siblings to the same version. Caret would let an older release pull newer siblings and break." A version bump is therefore all-or-nothing across roughly a dozen crates.

**MSRV: undeclared.** Neither `sdk/rust/Cargo.toml` nor `[workspace.package]` in the root `Cargo.toml` carries a `rust-version` key, and the crates.io API returns `rust_version: None` for every published version. The effective floor is set by `edition = "2024"` and `resolver = "3"`, which require Rust 1.85 or newer. The SDK README states the requirement only as "Rust toolchain with Rust 2024 edition support". Our `src-tauri/Cargo.toml` is `edition = "2021"`; the two can coexist in one build, but the toolchain floor rises and is enforced by a crate that does not promise to hold it steady.

**Dependency footprint.** Resolving the `microsandbox` node's transitive closure in the upstream `Cargo.lock` (848 packages total in the workspace) yields **687 packages**. Against the specific probes asked for:

| Dependency | In the SDK's closure | Note |
|---|---|---|
| `tonic`, `tonic-prost`, `prost` | **No** | Present in the workspace only via `microsandbox-metrics-collector` and `opentelemetry-otlp`, neither of which the SDK depends on |
| `axum` | **No** | Only the `examples/rust/net-secrets-body` example pulls it |
| `hyper`, `hyper-util`, `h2`, `tower`, `tower-http` | **Yes** | Transitively via `reqwest` |
| `reqwest` | **Yes, 0.13.4** | Our app pins `reqwest = "0.12"` with `native-tls`; the two majors resolve side by side rather than unifying |
| `rustls` 0.23 + `rustls-platform-verifier` | **Yes** | A second TLS stack alongside our `native-tls`/schannel |
| `sea-orm` 2.0.1 + `sqlx-core` 0.9 (SQLite) | **Yes** | An embedded SQL database and its migration crate link into the binary |
| `russh` 0.62 + `russh-sftp` | **Yes** | Optional, behind the non-default `ssh` feature |
| `openssl-sys`, `native-tls`, `opentelemetry` | **No** | |

Feature defaults are `["keyring", "net", "prebuilt"]`. Note that `ssh` is **not** a default feature of the SDK, though it *is* a default of `microsandbox-cli`; any SDK use of `sb.ssh()` requires opting in.

**Windows support: the crate builds, the runtime does not install.** Windows is a first-class compile target — `sdk/rust/Cargo.toml` carries a `[target.'cfg(target_os = "windows")'.dependencies]` block with `windows-sys` 0.61 and a `windows-native` keyring backend, and `sdk/rust/lib/setup/windows.rs` is 11 KB of host-setup logic. The blocker is `sdk/rust/build.rs`, whose header reads "Build script — downloads prebuilt msb + libkrunfw to `$MSB_HOME` (or `~/.microsandbox/`) under `{bin,lib}/`" and whose Windows arm is:

```rust
#[cfg(all(feature = "prebuilt", windows))]
fn install_prebuilt(_base_dir: PathBuf) {
    println!(
        "cargo:warning=skipping microsandbox prebuilt runtime install on Windows; build msb/libkrunfw locally"
    );
}
```

Compiling the SDK on Windows therefore produces a library with no runtime behind it until `msb.exe` and `libkrunfw.dll` are built or installed separately. Upstream marks Windows support as preview with Windows 11 as the tested path, and requires the `HypervisorPlatform` optional feature (https://docs.microsandbox.dev/troubleshooting/windows.md).

A related detail worth knowing: the `microsandbox` crate also ships a `[[bin]] microsandbox`, and `sdk/rust/bin/main.rs` describes itself as a "Thin shim that forwards all arguments to the installed `msb` binary", resolving through `MSB_PATH` then `~/.microsandbox/bin/msb` and self-healing by shelling out to `curl` and `tar` if absent. Even the SDK distribution assumes an installed `msb` on disk.

## 3. SDK versus CLI capability gap

**CONFIRMED for four of five probes; the ephemeral-port read-back is UNKNOWN and probably unsupported on both sides.**

Because `microsandbox-cli` depends on the `microsandbox` SDK crate, the CLI is by construction a subset: it can do nothing the SDK cannot. The real question is what the CLI cannot reach.

**Piped stdin exec — both, with an important shape difference.** `msb exec` supports it, though the flag table at https://docs.microsandbox.dev/cli/sandbox-commands.md does not say so. `crates/cli/lib/commands/exec.rs` detects a non-terminal stdin, reads it to EOF, and forwards the buffer through the SDK's `.stdin_bytes(...)`. The same file also exposes an **undocumented `--stream` flag** ("Stream stdin/stdout bidirectionally without a PTY") that forwards host stdin into the guest incrementally via `.stdin_pipe()` and `handle.take_stdin()`, and which rejects a terminal stdin with "`--stream` requires piped (non-terminal) stdin". The distinction matters for us: the buffered default is a one-shot write-then-EOF, which suits `tar_capture`/`tar_inject` in `src-tauri/src/credentials/mod.rs`, but the interactive agent-auth session in `src-tauri/src/agents/mod.rs` — which reads prompts and writes responses in turn — needs `--stream`, an undocumented flag, or the SDK's `exec_stream_with(...).stdin_pipe()`. Treat `--stream` as unstable surface and verify it against an installed binary in spike stage 2.

**Detached exec — both.** CLI: `msb run -d`/`--detach` "Run the resolved image command in the background and print the sandbox name". SDK: `Sandbox::start_detached(name)`, `.create_detached()`, `.detached(bool)`, and `sb.detach()` which releases the handle while the sandbox "continues running as a background process".

**Reading back an ephemeral published host port — neither, as far as anything documented goes.** The SDK's builder signatures are `fn port(self, host_port: u16, guest_port: u16)` and `fn port_bind(self, host_bind: IpAddr, host_port: u16, guest_port: u16)`; nothing returns an assigned port. The CLI's `parse_port_mapping` in `crates/cli/lib/commands/common.rs` parses `HOST:GUEST[/udp]` or `BIND:HOST:GUEST[/udp]` into a plain `u16` with no special case for `0`, and there is no `msb port` command corresponding to `podman port`. A `0` would parse syntactically, but no documented or source-visible path reports back what the kernel then chose. **Plan on the app allocating explicit host ports itself for microsandbox sandboxes**, and treat "does `-p 0:8000` work and is the result readable from `msb inspect --format json`" as a spike stage 2 experiment. (Exception: `msb ssh serve --port 0` *does* work and *is* readable — see question 5.)

**Streaming logs and lifecycle events — logs yes on both, events on neither.** `msb logs -f` follows by "200 ms polling, rotation-aware" and `--json` emits JSON Lines; the SDK exposes `logs(&LogOptions)` described as a "sync file read" of `<sandbox-dir>/logs/exec.log`, plus `metrics_stream(interval)`. Streaming exec output is first-class on the SDK (`exec_stream`, `ExecEvent`, `ExecHandle`). There is **no lifecycle event stream anywhere** — no `msb events`, no `Sandbox::watch`. The only continuous surfaces are `msb metrics --watch` (default `--interval 1s`) and `msb logs -f`, both polling. `listen_events` (`src-tauri/src/backends/types.rs`) has no microsandbox counterpart; declare `event_stream: false` in `ProviderCapabilities` or poll `msb ls`, as `01_ideas.md` already anticipates.

**Filesystem read/write API — SDK only, and this is the largest gap.** The CLI offers `msb copy` (host↔sandbox, plus same-sandbox and cross-sandbox forms) and nothing else. The SDK's `sb.fs()` handle offers `read`, `read_to_string`, `read_stream`, `write`, `write_stream`, `open_file`, `open_dir`, positional `read_handle`/`write_handle` with offsets, `read_dir`, `list`, `stat`/`fstat`, `set_stat_handle`, `mkdir`, `remove`, `remove_dir`, `remove_empty_dir`, `copy`, and `rename` (https://docs.microsandbox.dev/sdk/rust/filesystem.md). Anything the app wants to do to guest files through the CLI has to be expressed as a `msb copy` of a whole file or an `msb exec` of a shell command.

**Other SDK-only capabilities worth recording:** in-process SSH client and server (question 5), live reconfiguration with a dry-run plan (`sb.modify()` returning `live` / `next start` / `requires restart` / `unsupported` classifications — the CLI's `msb modify` exposes a `--format json` plan but not the typed surface), `sb.ping()`, `sb.touch()`, `sb.drain()`, `sb.wait_until_stopped()`, `Snapshot::reindex`, and `all_sandbox_metrics_local`. **CLI-only capabilities: none** — the CLI adds argument parsing, spinners, and terminal rendering over SDK calls.

## 4. `Sandbox::get(name)` interop

**CONFIRMED, structurally — the CLI *is* the SDK, so interop is not a compatibility claim but an identity.**

`crates/cli/Cargo.toml` declares `microsandbox.workspace = true` among its dependencies and defines `[[bin]] name = "msb"`. Its feature flags forward directly (`keyring = ["microsandbox/keyring"]`, `ssh = ["microsandbox/ssh", "dep:russh"]`, `net = [..., "microsandbox/net", ...]`). The `msb` binary is a `clap` front end over the same crate `cargo add microsandbox` installs.

The shared entry point is visible in `crates/cli/lib/commands/mod.rs`, whose `resolve_and_start` helper — used by `msb exec`, `msb copy`, `msb ssh`, and `msb ssh serve` alike — begins:

```rust
pub async fn resolve_and_start(name: &str, quiet: bool) -> anyhow::Result<Sandbox> {
    let handle = Sandbox::get(name).await?;
```

That is the same `Sandbox::get` documented for the SDK at https://docs.microsandbox.dev/sdk/rust/sandbox.md. State is shared through one `MSB_HOME`-rooted SQLite database: both `microsandbox` and `microsandbox-cli` depend on `microsandbox-db` and `microsandbox-migration`, and `resolve_home()` resolves the root identically for both. A sandbox created either way is visible and controllable the other way, provided both processes see the same `MSB_HOME`.

The docs corroborate at the usage level — the snapshot page's quick start creates a sandbox with `msb run --name baseline`, then reads it back with the SDK's `Sandbox::get("baseline")` in the very next example — but the source is the stronger evidence.

Two practical caveats. First, `Sandbox::get` returns a `SandboxHandle`, not a live `Sandbox`: it "provides status, configuration, and lifecycle control **without** an active connection to the guest agent. You cannot `exec` or `fs` on a handle - call `.start()` or `.connect()` to upgrade". Second, `resolve_and_start` carries a compatibility warning for sandboxes started before microsandbox 0.5 — "exec/shell still work temporarily, but filesystem and SFTP need stop/start" — evidence that the agent wire protocol has already broken once across minor versions. A sandbox created by one `msb` version and attached by a differently versioned SDK is not guaranteed to work.

## 5. `msb ssh serve` process model

**CONFIRMED from source — foreground, blocking, exactly one sandbox per invocation, no detach flag; the sandbox survives the listener's exit unless the serve command itself started it; `--port 0` yields an ephemeral port that the command prints; `--stdio` is the better fit for our use case.**

`crates/cli/lib/commands/ssh.rs` settles every part of this. The argument struct is:

```rust
pub struct SshServeArgs {
    pub sandbox: String,                                    // required positional, exactly one
    #[arg(long, conflicts_with = "stdio")] pub host: Option<String>,
    #[arg(long, conflicts_with = "stdio")] pub port: Option<u16>,
    #[arg(long)] pub stdio: bool,
}
```

There is no `--detach`, no `--daemon`, and no way to name more than one sandbox. `--host` and `--port` are mutually exclusive with `--stdio`. Defaults come from the SDK constants `DEFAULT_SSH_HOST` (`127.0.0.1`) and `DEFAULT_SSH_PORT` (`2222`).

**Listener mode is a foreground accept loop.** `run_serve` binds a `TcpListener`, prints the bound address, then loops on a `tokio::select!` between `listener.accept()` and `tokio::signal::ctrl_c()`, spawning a task per accepted connection and breaking only on Ctrl-C. It blocks for the process's whole lifetime. One `msb ssh serve` process therefore serves many *connections* to exactly one *sandbox*; N sandboxes need N processes on N distinct ports, all of which the app would have to allocate, supervise, and reap.

**An ephemeral port is available, and this is the one place read-back works.** Because `port` is an `Option<u16>` passed straight to `TcpListener::bind`, `--port 0` delegates the choice to the kernel, and the command then reports it:

```rust
let addr = listener.local_addr()?;
ui::success("SSH listening", &addr.to_string());
```

So `msb ssh serve <name> --port 0` prints the chosen port on stdout, parseable by the app. This is undocumented and untested upstream; verify before relying on it.

**The sandbox survives the listener exiting — conditionally.** After the loop ends, `run_serve` calls `super::maybe_stop(&sandbox)`, documented as "Stop the sandbox if we own its lifecycle (i.e., we started it). When connecting to an already-running sandbox, this is a no-op." and gated on `sandbox.owns_lifecycle()`. The rule for us: **start the sandbox with `msb start` first, then attach `msb ssh serve` to it**, and killing the serve process leaves the sandbox running. Let `msb ssh serve` start a stopped sandbox and its exit takes the sandbox down with it — a foot-gun worth encoding in `MsbProvider`.

**Prefer `--stdio` over the listener for Mutagen and host tools.** `msb ssh serve <name> --stdio` "carries a single SSH connection over stdin/stdout instead of a TCP listener", and upstream documents the OpenSSH integration explicitly (https://docs.microsandbox.dev/sandboxes/ssh.md):

```sshconfig
Host devbox.msb
  User root
  ProxyCommand msb ssh serve devbox --stdio
```

with the follow-on claim that `ssh devbox.msb`, `sftp devbox.msb`, `scp`, `rsync -av ./src/ devbox.msb:/app/`, and VS Code Remote-SSH all then work against the alias. OpenSSH spawns and reaps the `ProxyCommand` child per connection, so the app supervises nothing, binds no port, and cannot leak a listener across an app crash. The cost is that the app must write a `~/.ssh/config` entry per sandbox — which `docs/features/host-tool-ssh-access/` is already shaped around — and that `msb` must be on `PATH` for whatever process OpenSSH runs under.

**If the SDK is ever adopted, the supervision problem vanishes entirely.** `sb.ssh().server()` / `prepare_server()` returns a cloneable `SshServer` that "can serve many connections" via `serve_connection(stream)` against any `AsyncRead + AsyncWrite`. The app could bind its own `TcpListener` inside the Tauri process on a port it chose, with no child process at all. That is the single strongest v2 argument for the SDK.

Unsupported in every mode: reverse forwarding (`-R`) and stream-local forwarding. Local (`-L`) and dynamic (`-D`) forwarding do work, with connections originating inside the guest, so `127.0.0.1` in a forward target means the sandbox's loopback, not the host's.

## 6. Authorized keys scope, guest user, and sftp/rsync

**CONFIRMED — keys are global to the microsandbox installation via the CLI, but per-sandbox-scopable via the SDK; clients land as `root` by default; SFTP is first-class and rsync is documented.**

**Scope: global under the CLI.** Both https://docs.microsandbox.dev/sandboxes/ssh.md ("Keys are appended to `<MSB_HOME>/ssh/authorized_keys`") and https://docs.microsandbox.dev/cli/ssh-commands.md ("The default authorization file is `<MSB_HOME>/ssh/authorized_keys`, or `~/.microsandbox/ssh/authorized_keys` when `MSB_HOME` is unset. The file is created with private permissions.") describe one installation-wide file. Source confirms there is no per-sandbox branch: `run_authorize` in `crates/cli/lib/commands/ssh.rs` resolves `microsandbox::LocalBackend::lazy().config().ssh_dir().join("authorized_keys")` with no sandbox name in scope at all, dedupes by parsed public key, and appends. `msb ssh authorize` takes no sandbox argument. **Any key in that file can reach any sandbox served on that installation.**

Host keys, by contrast, *are* per sandbox: `<sandbox-dir>/ssh/host_ed25519`, "created on first serve". Expect a host-key-verification prompt or `known_hosts` churn the first time each sandbox is served — relevant to Mutagen, which will fail rather than prompt. Plan on `StrictHostKeyChecking` handling in the generated `~/.ssh/config` entry.

Per-sandbox key scoping exists but only through the SDK: `SshServerOptionsBuilder` exposes `authorized_keys_path()`, `authorized_key()` (in-memory), `host_key_path()`, and `host_key()`, and `server_with` is documented as failing on an empty key set. The CLI exposes none of these.

**Guest user: `root` by default.** Every documented client example is `ssh -p 2222 root@127.0.0.1` / `sftp -P 2222 root@127.0.0.1`, and the `ProxyCommand` config block sets `User root`. The SDK reference is explicit: `SshClientOptionsBuilder` has "Defaults: user `root`, terminal from `$TERM` (falling back to `xterm`), SFTP enabled", and `SshServerOptionsBuilder.user()` "Override the guest user used for exec requests". `msb ssh serve` has no `--user` flag, so under the CLI the login user is `root` (matching Podman sandboxes, whose workspace is `/root/work`) and overriding it requires the SDK.

**SFTP and rsync: documented working; not empirically verified here.** SFTP is enabled by default on both the client and the server builders, `russh-sftp` is a real dependency behind the `ssh` feature, and the docs show `sftp -P 2222 root@127.0.0.1` against the listener plus `sftp devbox.msb`, `scp ./build.tar devbox.msb:/tmp/`, and `rsync -av ./src/ devbox.msb:/app/` against the `ProxyCommand` alias. Mutagen's requirement is an `ssh` transport that can run a remote agent binary and stream over it, which the documented `ssh devbox.msb` remote-command path satisfies; it does not strictly need SFTP. Nothing in the docs states an SFTP protocol-version floor, and Mutagen's agent installation over the transport is unverified — keep it as a spike stage 3 item.

## 7. Snapshots in depth

**CONFIRMED on every sub-question except size and timing figures, which are UNKNOWN.**

Primary source: https://docs.microsandbox.dev/sandboxes/snapshots.md.

**Syntax.** CLI: `msb snapshot create <name> --from <sandbox>`, with `--label KEY=VALUE`, `--dest-dir PATH`, and `--integrity`; plus `msb snapshot ls`, `inspect <name> [--verify]`, `rm <name> [--force]`, `reindex`, `verify <name>`, `save <name> <archive.tar.zst> [--with-image] [--plain-tar]`, and `load <archive>`. Booting: `msb run --name <new> --from-snapshot <snapshot>`. SDK: `Sandbox::get("baseline").await?.snapshot("after-pip-install").await?` returning a handle with `.digest()`; the builder form `Snapshot::builder(name).from_sandbox("baseline").record_integrity().create()`; `Snapshot::list()`, `Snapshot::get(name)` ("By name, digest, or path"), `Snapshot::remove(name, force)`, `Snapshot::reindex(path)`, and `snap.verify()`. Booting: `Sandbox::builder("worker").from_snapshot("after-pip-install").create()`, with `from_snapshot` "mutually exclusive with the image source" because "A snapshot already pins its image".

**Many concurrent sandboxes from one snapshot: yes, explicitly.** "Multiple sandboxes from one snapshot are independent; each copy of the upper layer diverges on its own", and booting "gives the new sandbox its own writable copy". This is exactly the base-image semantics `MsbProvider::create_instance` needs, and it is called out as an intended use case ("Reusable build state. Install dependencies once, snapshot, then `msb run --from-snapshot ...` repeatedly without paying the install cost").

**Stopped requirement: confirmed, and broader than `01_ideas.md` records.** "Snapshots are **disk-only** and require a sandbox that is not running. Stopped and crashed sandboxes can be snapshotted; running, draining, and paused sandboxes are rejected." So `Draining` and `Paused` are rejected too, not only `Running`. The specific error name `SnapshotSandboxRunning` cited in `01_ideas.md` does not appear on the current snapshots page or in the SDK error reference — treat the error identifier as UNKNOWN while the behaviour it describes is CONFIRMED. The `MsbProvider` sequence stays `msb stop` then `msb snapshot create`.

**Storage.** `~/.microsandbox/snapshots/<name>/` by default, relocatable per artifact with `--dest-dir DIR` (landing at `DIR/<name>`). "That whole directory is the snapshot" and "The snapshot directory is the whole artifact; there is no hidden daemon state" — so it is movable with plain `mv` or `scp -r`. Because `resolve_home()` reads `MSB_HOME`, an e2e run's snapshots relocate with the rest of microsandbox state. A separate local index backs `list` and `get` for fast lookup, rebuildable with `msb snapshot reindex` when it drifts from what is on disk.

**Encoding a base-image version: two viable mechanisms, both parseable.** The snapshot *name* is its identity and is a free-form string, so `sandbox-mio-base-<BASE_IMAGE_VERSION>` works directly and mirrors how `parse_image_version` (`src-tauri/src/backends/mod.rs`) already recovers `:N` from a Podman tag. Labels are also supported at creation — `msb snapshot create after-pip-install --from baseline --label stage=ready`, and the captured set explicitly includes "Optional labels and integrity hash" — with `msb snapshot inspect <name>` to read them back. **Recommendation: put the version in the name** (it is the lookup key, so `base_image_ready` becomes a name existence check with no parsing of inspect output) **and mirror it into a label** for the update prompt. Note the CLI label flags differ by object: `msb run --label` says `KEY=VALUE, or bare KEY`, whereas `msb snapshot create --label` is documented only as `<key>=<value>`.

**Size and time figures: none published.** The snapshots page gives no numbers for artifact size, creation duration, or boot-from-snapshot latency, and none appear elsewhere in the docs. What is documented is that booting from a snapshot "is a cold boot of a fresh VM", not a memory restore — so microsandbox's headline sub-100ms boot claim is the relevant figure, not a resume time. Measuring the snapshot of a Containerfile-equivalent Debian base, and the boot time from it, belongs in spike stage 3.

**Integrity.** Default creation "records enough metadata to validate the artifact without hashing the full writable layer"; `--integrity` / `.record_integrity()` adds a content hash, and `msb snapshot load` verifies it when the archive carries one. Not needed for a locally produced base image, but relevant if the base image is ever shipped between machines.

## 8. Build capability

**CONFIRMED — there is no build command. `msb load` of a `docker save` or OCI-layout tarball, plus snapshots, plus pre-boot rootfs patches, are the only routes to a reusable image.**

Searching the complete documentation corpus for `msb build` or `msb image build` returns nothing. The CLI overview's quick reference lists the image commands as exactly `msb pull`, `msb images`, `msb rmi`; https://docs.microsandbox.dev/cli/image-commands.md documents only `msb pull`, `msb load`, and `msb save`. The source agrees: `crates/cli/lib/commands/` contains `image.rs` and `pull.rs` but no `build.rs`, and the full module list (`common completion context copy create exec image inspect install list logs metrics modify ping ps pull registry remove restart run self_cmd snapshot ssh start stop touch uninstall volume`) has no build entry. Upstream is explicit about the boundary: "microsandbox cannot read Docker's private local image store directly. If you build an image locally with `docker build`, export it with `docker save` and load it into microsandbox" (https://docs.microsandbox.dev/recipes/docker/local-images.md).

The three available routes:

1. **`msb load`** — `docker save my-image:latest | msb load`, or `msb load --input my-image.tar`, or an OCI Image Layout archive from BuildKit, Skopeo, or ORAS with `--tag` when the layout lacks an `org.opencontainers.image.ref.name` annotation. Caveat from https://docs.microsandbox.dev/images/overview.md: the import is "a regenerated archive: manifest digest and layer digests can differ from the original registry image", so digest-based identity does not survive the round trip. This is the bridge `01_ideas.md` correctly proposes for development only, since it reintroduces the Podman dependency the feature exists to remove.
2. **Bootstrap-and-snapshot** — the path `01_ideas.md` chooses, and the one upstream itself names first under "Reusable build state".
3. **Pre-boot rootfs patches** — a partial third route not accounted for in `01_ideas.md`. `msb run` and `msb create` accept `--copy`, `--copy-file`, `--copy-dir`, `--mkdir`, and `--rm` to "Patch the rootfs before boot", and the SDK exposes the same through `.patch(|p| p...)` ("Rootfs modifications (write files, mkdir, copy, symlink)"). This cannot run a `RUN` step — no command executes during a patch — so it cannot replace `apt-get install`, but it can seed configuration files and directories that the Containerfile currently produces with `COPY`, without a snapshot rebuild. Useful for reducing what must be baked into the snapshot, and therefore how often `BASE_IMAGE_VERSION` has to bump.

The conclusion for `MsbProvider::ensure_base_image` is unchanged: replay the Containerfile's `RUN` steps as `msb exec` calls against `debian:12-slim`, stop, snapshot under a version-keyed name. Rootfs patches are an optimisation for the `COPY`-shaped lines, not an alternative to the snapshot.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A search for how to start or connect to the microsandbox daemon finds a `microsandbox-server` crate and daemon-shaped documentation | The `0.2.x` architecture had a host daemon; that crate was last published 2025-06-05 as `0.2.6` and no longer exists in the repository, but stale pages and third-party write-ups survive | Treat `0.6.x` as daemon-free. `BackendKind::Local` in `sdk/rust/lib/backend/mod.rs` "Spawns microVMs on the calling host"; supervise nothing |
| Linking `microsandbox` into `src-tauri` compiles on Windows but every sandbox operation fails at runtime | `sdk/rust/build.rs` skips the prebuilt runtime install on Windows with only a `cargo:warning`, so `msb.exe` and `libkrunfw.dll` are never placed under `%USERPROFILE%\.microsandbox` | Install the runtime separately with `irm https://install.microsandbox.dev/windows \| iex`, or stay on the `msb` CLI for v1 |
| Killing an `msb ssh serve` process also kills the sandbox | `run_serve` ends with `maybe_stop`, which stops the sandbox when `owns_lifecycle()` is true — that is, when the serve command started a stopped sandbox | Always `msb start <name>` before attaching `msb ssh serve <name>`, so the serve process never owns the lifecycle |
| The app leaks `msb ssh serve` listeners, or two sandboxes collide on port 2222 | `msb ssh serve` is a foreground accept loop taking exactly one required positional sandbox, with no `--detach` and a fixed `2222` default | Use `ProxyCommand msb ssh serve <name> --stdio` in a generated `~/.ssh/config` entry so OpenSSH spawns and reaps the bridge per connection; if a listener is genuinely needed, pass `--port 0` and parse the printed `SSH listening <addr>` line |
| Mutagen or `rsync` fails on first connection to a sandbox with a host-key error | Host keys are per sandbox at `<sandbox-dir>/ssh/host_ed25519`, created on first serve, so every new sandbox is an unknown host | Handle `known_hosts` explicitly in the generated SSH config entry rather than relying on an interactive prompt that Mutagen will never answer |
| An authorized key added for one sandbox grants access to all of them | `msb ssh authorize` takes no sandbox argument and appends to the installation-wide `<MSB_HOME>/ssh/authorized_keys` | Accept installation-wide scope under the CLI, or use the SDK's `SshServerOptionsBuilder.authorized_key()` / `authorized_keys_path()` for per-sandbox scoping |
| `msb snapshot create` is rejected on a sandbox that `msb ls` does not show as running | `Draining` and `Paused` are rejected alongside `Running`; only `Stopped` and `Crashed` can be snapshotted | Stop and wait for a terminal state before snapshotting, rather than checking only for `Running` |
| The interactive agent-auth session hangs against a microsandbox sandbox although credential capture works | The default `msb exec` piped-stdin path reads host stdin to EOF and forwards it as one buffer, so a prompt-then-respond exchange deadlocks | Use the undocumented `msb exec --stream` flag for incremental forwarding, and verify it against an installed binary before depending on it |
| A published port cannot be discovered after creation the way `podman port` reports one | Both `-p HOST:GUEST` and the SDK's `port(host_port: u16, guest_port: u16)` take an explicit host port, and no command reports an assigned one | Allocate host ports in the app for microsandbox sandboxes; do not assume a `127.0.0.1::<port>` equivalent exists |
| A minor microsandbox upgrade breaks a running sandbox the app created earlier | The agent wire protocol has already broken once — `resolve_and_start` warns that pre-0.5 sandboxes need stop/start before filesystem and SFTP work — and all `microsandbox-*` crates pin siblings with exact `=` requirements | Stop sandboxes across an `msb self update`, and pin the `msb` version the app supports rather than tracking latest |
