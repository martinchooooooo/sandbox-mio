---
type: spike
title: "Hands-On Spike: `msb` 0.6.6 on the Windows Development Host"
feature: microsandbox-provider
---
# Hands-On Spike: `msb` 0.6.6 on the Windows Development Host

Empirical spike run 2026-08-09 against `msb 0.6.6` already installed at `C:\Users\krist\.microsandbox\bin\msb.exe` on the Windows 11 development host, which also runs the Podman/WSL2 stack this project depends on today. Every result below is observed command output, not documentation. Findings marked BLOCKER contradict an assumption recorded in `docs/features/microsandbox-provider/01_ideas.md`. All spike sandboxes, volumes, and images were removed at the end of the run; the residual host changes are listed under Host Side Effects.

## Summary

| # | Question | Result |
|---|---|---|
| 1 | Does WHP coexist with Podman/WSL2 on one host? | **YES** — `msb doctor` reports "Host setup is ready" on the Podman host |
| 2 | Bind-mount a Windows host path into the guest? | **YES** for content, **BLOCKER** for POSIX semantics — see finding 4 |
| 3 | Reads on a virtiofs mount return correct data? | Content yes, **exit code 1 always** — BLOCKER |
| 4 | Disk-backed (`virtio-blk`/ext4) volumes? | **WORK CORRECTLY** — the viable workspace path |
| 5 | Ephemeral published host port? | Accepted and genuinely bound, but **`msb` never reports the resolved port** — BLOCKER for readback |
| 6 | Guest server reachable from Windows? | **YES**, but only when the guest binds `0.0.0.0`, not loopback |
| 7 | Piped stdin into `msb exec`? | **YES** via `--stream` |
| 8 | Exit-code propagation from `msb exec`? | **YES** — faithful, including 127 for a missing command |
| 9 | Labels persisted and queryable? | **YES** via `inspect`, **NOT** in `list` — N+1 cost |
| 10 | Snapshot usable as a base image via the CLI? | **NO** — BLOCKER, contradicts `01_ideas.md`; retested against an indexed snapshot, same result |
| 11 | `msb load --tag sandbox-mio/base:4`? | **YES** — the viable base-image route |
| 12 | `msb modify` can add a published port? | **NO** — the relay-pool design is still required |
| 13 | SSH into a sandbox with an ordinary OpenSSH client? | **YES** via `ProxyCommand ... --stdio` |
| 14 | Default idle timeout that would stop a dev sandbox? | **NONE** — `idle_timeout_secs: null` |

## 1. Host viability: WHP and WSL2 coexist

`msb doctor` on the Podman development host reports Platform `Windows x86_64`, `MSB_HOME: C:\Users\krist\.microsandbox`, and three green checks — `msb`, `libkrunfw` (`lib\libkrunfw.dll`), and `Hypervisor  Windows Hypervisor Platform` — ending in "Host setup is ready." The `HypervisorPlatform`-versus-`VirtualMachinePlatform` mutual-exclusion risk recorded as spike stage 1 in `docs/features/microsandbox-provider/01_ideas.md` is disproved on this host: both stacks are enabled simultaneously and both report healthy. `msb doctor` also prints `MSB_HOME`, confirming the variable is a first-class runtime concept rather than an SSH-only path.

## 2. State layout

The microsandbox home contains `bin`, `cache`, `db`, `lib`, `sandboxes`, and (after the SSH test) `ssh`. Sandbox state is a local database plus per-sandbox directories, not a daemon-held registry.

## 3. `msb create` surface relevant to this project

`msb create <IMAGE>` accepts `--name`, `--cpus`, `--memory`, `--volume SOURCE:DEST[:OPTIONS]`, `--mount-dir`, `--mount-file`, `--mount-disk`, `--mount-named`, `--workdir`, `--env`, `--label KEY=VALUE`, `--replace`, `--port`, `--pull`, `--user`, `--hostname`, `--entrypoint`, `--shell`, `--tmpfs`, `--idle-timeout`, `--max-duration`, and a pre-boot rootfs-patching group (`--copy`, `--copy-file`, `--copy-dir`, `--mkdir`, `--rm`, `--script`, `--script-path`). The `<IMAGE>` argument accepts a registry reference, a `./rootfs` directory, or a `./disk.qcow2` file.

`msb create` **auto-starts the sandbox**: `msb ls` reports it as `running` immediately afterwards, unlike `podman create`. A follow-up `msb start <name>` therefore exits 1 with `error: sandbox still running: cannot start sandbox '<name>': already running`.

Defaults observed in `msb inspect`: 1 CPU, 512 MiB memory, a 128 MiB `/tmp` tmpfs mounted automatically, `pull_policy: IfMissing`, `security_profile: default`, and an OCI writable overlay upper size of 4096 MiB.

## 4. BLOCKER: every read on a virtiofs mount exits non-zero

Reproduced deterministically. A sandbox created with `--volume <windows-path>:/root/work` mounts the host directory as `virtiofs` (confirmed in `/proc/mounts` as `root_work_<hash> /root/work virtiofs rw,relatime`). Content transfer is correct and bidirectional: a host-written file is readable in the guest with correct contents, and a guest-written file appears on the host with correct contents.

POSIX semantics are broken. Reading any file on the mount prints the correct bytes to stdout and then fails:

- `cat /root/work/host-file.txt` → correct content on stdout, `cat: /root/work/host-file.txt: Permission denied` on stderr, exit code 1.
- `head -c 100 <same file>` → correct content, `head: failed to close '<path>': Permission denied` — the failure is on `close()`, after a successful read.
- `stat <same file>` → `stat: cannot statx '<path>': Too many levels of symbolic links` (ELOOP), exit code 1.

The failure is a property of the mount, not of file provenance: a file written by the guest itself into the same mount fails identically on read (exit 1). A control read of a file on the overlay root (`/root/ctl.txt`, outside the mount) succeeds with exit code 0, isolating the defect to the virtiofs mount.

No mount option avoids the failure. All three legal `stat-virt` values (`strict`, `relaxed`, `off`) and both `host-perms` values (`private`, `mirror`) were tested individually; every combination reproduced both the `close()` permission failure and the `statx` ELOOP. The combination `stat-virt=off,host-perms=mirror` is rejected at config validation and cannot be tested.

Impact on this project: `check()` in `src-tauri/src/backends/mod.rs`, `src-tauri/src/runtime/mod.rs`, and `src-tauri/src/machine/mod.rs` all treat a non-zero exit as a command failure. Every `mise` operation, every `git` operation, and every dev-server file read against a workspace bind-mounted at `/root/work` would report failure while appearing to produce correct output.

## 5. Disk-backed named volumes work correctly

A named volume created with `msb volume create <name> --kind disk --size 1G` and mounted with `--mount-named <name>:/data:kind=disk,size=1G` mounts as `/dev/vdc /data ext4 rw,relatime`. On the ext4 volume, both the read and the `stat` that fail on virtiofs succeed with exit code 0. A `--kind dir` named volume mounts through virtiofs and reproduces the finding-4 failure exactly, confirming the defect tracks the *transport* (virtiofs) rather than the *bind-mount* concept.

Consequence for the design: a microsandbox sandbox cannot use the host-directory bind mount that `create` in `src-tauri/src/backends/mod.rs` relies on. The workspace must live on a disk-backed volume inside the sandbox, with host-to-guest file movement handled by the sync engine (`MutagenCli` in `src-tauri/src/sync/mod.rs`) over the SSH transport proved in finding 11 — which is the code path this project already uses for remote deploys.

## 6. Published ports: ephemeral binding works, readback does not

`msb create ... -p 127.0.0.1:0:8082` is accepted and exits 0. The port is genuinely bound: `Get-NetTCPConnection` against the owning `msb` process showed it listening on `127.0.0.1:64310`.

Microsandbox never reports the resolved port. `msb inspect --format json` echoes the request verbatim as `{"guest_port": 8082, "host_bind": "127.0.0.1", "host_port": 0, "protocol": "tcp"}`, and `msb status --format json` renders the same as the string `"127.0.0.1:0->8082/tcp"`. There is no equivalent of `podman port <name> <port>/tcp`, which `prepare_terminal` and `prepare_agent_web` (`src-tauri/src/runtime/mod.rs`) and `expose_port` (`src-tauri/src/network/mod.rs`) all depend on.

Mitigation: allocate the host port on the app side and pass it explicitly. The app already owns host-port allocation machinery, indicated by `NetworkError::PortAllocation` in `src-tauri/src/network/types.rs`.

## 7. Guest servers must bind `0.0.0.0`, not loopback

With a sandbox created as `-p 127.0.0.1:47822:8082`, a guest HTTP server bound to `127.0.0.1:8082` is **not** reachable from Windows — `Invoke-WebRequest http://127.0.0.1:47822/` fails with the connection closed unexpectedly. The identical server bound to `0.0.0.0:8082` returns **HTTP 200** to the Windows host.

The behaviour is the direct analogue of the Podman `--network pasta:--host-lo-to-ns-lo` flag documented in `create` (`src-tauri/src/backends/mod.rs`), which exists precisely because zellij and paseo bind the container's loopback. Microsandbox exposes no equivalent flag, so under a microsandbox provider the in-guest services must be started with an explicit `0.0.0.0` bind instead. The host side stays bound to `127.0.0.1`, so nothing is exposed on the local network.

## 8. `msb exec`: stdin, exit codes, and the missing detach

`msb exec` accepts `--env`, `--workdir`, `--user`, `--tty`, `--no-tty`, `--timeout`, `--rlimit`, and `--stream`. The `--stream` flag streams stdin and stdout bidirectionally without a PTY, documented in the CLI help as byte-faithful and safe for JSON lines — the capability `tar_capture`/`tar_inject` (`src-tauri/src/credentials/mod.rs`) and the interactive auth session (`src-tauri/src/agents/mod.rs`) need, both of which currently use `podman exec -i` with piped stdin.

Exit codes propagate faithfully: `sh -c 'exit 42'` returns 42, and a non-existent command returns 127 with a diagnostic naming the missing binary.

There is **no detached exec flag**. `podman exec -d`, used to start the paseo daemon in `prepare_agent_web` (`src-tauri/src/runtime/mod.rs`), has no direct equivalent; backgrounding must be done inside the guest shell (`nohup ... &`), which worked in the port test, or via the `--script` registration mechanism.

## 9. JSON shapes and label retention

`msb list --format json` returns an array of objects with exactly four fields: `created_at`, `image`, `name`, `status`. Observed `status` values are `"Running"` and `"Stopped"` — capitalised, unlike Podman's lowercase `created`/`running`/`exited` that `translateStatus()` (`src/lib/sandbox.ts`) maps to `status_<value>` locale keys.

Labels set with `--label sandbox-mio.workspace=<value>` are persisted and returned by `msb inspect --format json` under `active_config.labels`, but are **absent from `msb list --format json`**. Recovering the workspace path for every sandbox therefore costs one `msb inspect` per sandbox, where `PodmanCliBackend::list` (`src-tauri/src/backends/mod.rs`) gets labels from a single `podman ps --format json` call.

`msb inspect --format json` also exposes `active_config` versus `config` plus a `pending_changes` array, modelling configuration changes that require a restart.

## 10. BLOCKER: snapshots are not a base-image mechanism via the CLI

`msb snapshot create --from <stopped-sandbox> --label <k=v> <destination>` succeeds and is fast (1.3 s for a 4 GiB sparse artifact whose apparent host size is 1 byte). `msb snapshot list --format json` reports `artifact_path`, `digest`, `format: "raw"`, `image_ref`, `name`, `parent_digest`, and `size_bytes`.

A snapshot captures only the writable upper layer and cannot be booted standalone. Creating a sandbox from the snapshot's artifact path succeeds at create time but every exec then fails with `failed to resolve guest uid 0: No such file or directory` — the parent image is absent, so `/etc/passwd` does not exist. Creating a sandbox from the indexed snapshot *name* is not supported either: `msb create base-v4` resolves the argument as a registry reference and fails against Docker Hub. `msb create --help` documents no snapshot source, and there is no `commit` command under `msb image`.

The result was retested against a *correctly indexed* snapshot to rule out the custom artifact path as the cause. A snapshot written to the default location (`C:\Users\krist\.microsandbox\snapshots\mio-base-v4`) is fully indexed — `msb snapshot inspect mio-base-v4` reports `Image: python`, `Image Manifest: sha256:b7289e1fd56c…`, `Filesystem: ext4`, `Upper File: upper.ext4`, `Upper Size: 4.0 GiB`, and `Labels: mio.base_image_version=4`. Creating from that indexed name nonetheless fails identically: `msb create mio-base-v4` resolves the argument against Docker Hub and returns `registry error: Not authorized: url https://index.docker.io/v2/library/mio-base-v4/manifests/latest`.

The snapshot metadata proves microsandbox *does* pin the parent image and *does* retain a version label — the capability exists, but the CLI exposes no way to consume it at create time.

The Rust SDK does expose `Sandbox::builder(...).from_snapshot(...)`, which the SDK documentation states re-pins the original image reference and digest. The bootstrap-and-snapshot base-image approach chosen in `docs/features/microsandbox-provider/01_ideas.md` is therefore **not implementable through the CLI** and would require the SDK — which the same document rejected for v1 on `ProcessRunner` grounds.

A parallel documentation-and-source spike (`docs/features/microsandbox-provider/spike/cli-surface.md`) concluded that snapshots "fully support the bootstrap-and-snapshot base image". That conclusion is **contradicted by direct execution** and should not be relied on: the snapshot *creation* side is fully supported, the *consumption* side has no CLI entry point.

## 11. `msb load` accepts this project's exact image tag

`msb save <ref> --output <tar>` followed by `msb load --input <tar> --tag "sandbox-mio/base:4"` succeeds, and `msb image ls` then lists `sandbox-mio/base:4`. A sandbox created from the loaded tag boots, and `msb list --format json` reports its `image` as the full `"sandbox-mio/base:4"` string — so `parse_image_version` (`src-tauri/src/backends/mod.rs`), which parses the `:N` suffix, maps over unchanged.

`msb load` is therefore the only registry-free base-image route available through the CLI, and it requires the archive to be produced by an external builder (`podman save` or `docker save`), because microsandbox has no build command.

## 12. `msb modify` cannot add a published port

`msb modify <NAME>` exposes `--restart` ("Restart if needed so restart-required changes become active now") but no port flag. The open question in `docs/features/microsandbox-provider/01_ideas.md` asking whether `msb modify --port` could retire the relay-pool plus in-container `socat` design is answered **no**: ports are fixed at create time, exactly as with Podman, so `relay_pool_publish_args()` (`src-tauri/src/network/mod.rs`) and the `socat` dependency in `src-tauri/resources/Containerfile` remain necessary.

## 13. SSH works with an ordinary OpenSSH client over `ProxyCommand`

`msb ssh` provides `connect`, `serve`, and `authorize`. `msb ssh authorize --file <pubkey>` appends to `C:\Users\krist\.microsandbox\ssh\authorized_keys`. `msb ssh serve <sandbox>` takes `--host` and `--port` for a TCP listener, and `--stdio` to serve one SSH transport over stdin and stdout.

The `--stdio` mode was proved end-to-end with the Windows OpenSSH client:
`ssh -o "ProxyCommand=msb ssh serve <sandbox> --stdio" -i <key> root@<sandbox> "id; uname -a"` returned `uid=0(root)` and `Linux <sandbox> 6.12.91 ... x86_64 GNU/Linux`, exit code 0.

`--stdio` is the better integration shape than the TCP listener: it needs no allocated port and no supervised long-lived process, and it composes with the `ProxyCommand` directive in an OpenSSH config block — which is what `write_alias()` (`src-tauri/src/ssh/mod.rs`) already renders into `~/.ssh/sandbox-mio/config`. `SshAlias` (`src-tauri/src/ssh/types.rs`) carries `host`/`port`/`user`/`identity` and would need a `ProxyCommand` variant to express a microsandbox target.

Two further behaviours of the TCP listener mode were measured, both arguing against it:

- `msb ssh serve <sandbox> --port 0` **does** allocate and report an ephemeral port, printing `SSH listening 127.0.0.1:59091`. Ephemeral allocation is therefore reported for the SSH listener but not for `-p` publish (finding 6) — the two subsystems behave differently, and only the SSH listener is self-reporting.
- The listener **auto-starts a stopped sandbox and takes ownership of its lifecycle**. A sandbox in `Stopped` state moved to `Running` when `msb ssh serve` was launched against it; force-terminating the listener process left the sandbox in **`Crashed`** state, not `Stopped`. Any design that supervises a `msb ssh serve` child must therefore terminate it gracefully, or start the sandbox explicitly beforehand so the listener does not own it.

## 14. Lifecycle defaults suit long-lived dev sandboxes

`msb inspect --format json` reports `lifecycle: { ephemeral: false, idle_timeout_secs: null, max_duration_secs: null }` for a plain `msb create`. No idle timeout or maximum duration is applied by default, so a sandbox will not self-stop under a developer's idle editor. The `msb touch` command exists to refresh idle activity when `--idle-timeout` is set explicitly.

## Cross-Check Against the Documentation Spikes

`docs/features/microsandbox-provider/spike/cli-surface.md` and `docs/features/microsandbox-provider/spike/sdk-daemon-ssh.md` were produced from published documentation and upstream source rather than execution. Three of their claims conflict with direct measurement on `msb` 0.6.6; the executed result is authoritative in each case.

| Claim from a documentation spike | Measured result |
|---|---|
| Snapshots "fully support the bootstrap-and-snapshot base image" | **Contradicted.** No CLI path consumes a snapshot at create time, indexed or not (finding 10) |
| `-v` cannot take a Windows drive-letter path, so `--mount-dir` is mandatory for host directories | **Contradicted.** `-v "C:\…\work:/root/work"` mounted correctly and `msb inspect` reported `"type": "Bind"` with the intact Windows host path. `--mount-dir` also works; neither is mandatory |
| `msb exec` signals the whole guest process group on exit, making a `nohup … &` background start "actively hostile" | **Not reproduced.** A `python3 -m http.server` started with `nohup … &` inside an `msb exec` survived that exec's exit and served **HTTP 200** to the Windows host from a later, separate command (finding 7). The process-group behaviour may still apply to other signals or releases, but backgrounding did work here |

The three conflicts share a cause worth remembering for `02_dev.md`: reading the upstream source establishes what a subsystem *can* express, not what the shipped CLI *exposes*. Decisions that depend on a CLI entry point existing should be confirmed by running the binary.

## Addendum: Phase G Pre-Flight Verification (2026-08-11)

Run against the same `msb 0.6.6` binary, targeting the four unknowns `02_dev.md`'s Resolved
Decisions rested on without direct execution. All spike sandboxes/volumes were removed and the
throwaway SSH key revoked at the end.

| # | Item | Result |
|---|---|---|
| 1 | `msb volume create <name> --kind disk --size 1G` + `--mount-named <name>:/data:kind=disk,size=1G` at `create` time | **CONFIRMED.** Volume creates as `disk_fstype: "ext4"`; `msb inspect --format json` shows the mount as `"type": "Named"` under `active_config.mounts`. A read and a `stat` against a file on the mount both exit 0, matching finding 5 |
| 2 | `msb inspect --format json` label path | **CONFIRMED as `active_config.labels`** (also mirrored at `config.labels`), not `config.spec.labels`. Matches finding 9's executed result; the doc-only `cli-surface.md` claim remains wrong |
| 3 | Resolved Decision 4's exact wrapper, `sh -c '<cmd> >/tmp/out.log 2>&1 & disown'` | **PARTIALLY CONTRADICTED.** This image's `/bin/sh` is `dash`, which has no `disown` builtin: the wrapper exits **127** with `sh: 1: disown: not found`. The `&` still backgrounds the process before the failing `disown` runs, so the target process itself survives — confirmed via `ps -ef` showing it reparented to PID 1 — but `msb exec` reports the wrapper call itself as failed, which `MsbProvider::exec` would surface as an error even though backgrounding succeeded. Substituting `bash -c '<cmd> & disown'` (bash is present at `/usr/bin/bash` in this base image) exits 0 cleanly and the backgrounded process is likewise reparented to PID 1. **Action for Phase G: invoke the wrapper via `bash -c`, not `sh -c`.** |
| 4 | `msb ssh serve <name> --stdio` as an OpenSSH `ProxyCommand`, with a per-sandbox `known_hosts` file | **CONFIRMED end-to-end.** `ssh -o "ProxyCommand=msb ssh serve <name> --stdio" -o "UserKnownHostsFile=<per-sandbox path>" -o "StrictHostKeyChecking=accept-new" -i <key> root@<name> "id; uname -a"` pinned the host key into the fresh `known_hosts` file on first connect and returned `uid=0(root)` / `uname -a` output, exit 0. Also noted: `msb ssh authorize` has no per-sandbox scope — `--file <pubkey>` appends to the single host-wide `C:\Users\krist\.microsandbox\ssh\authorized_keys`, matching finding 13's description but worth flagging since the flag surface differs slightly from a first guess (no positional `<SANDBOX>` argument on `authorize`) |

The only actionable change from this pass is item 3: `02_dev.md` Resolved Decision 4 should read
`bash -c` instead of `sh -c` in the backgrounding wrapper before Phase G's implementation copies it
into `MsbProvider::exec`.

## Host Side Effects

Cleanup removed every spike sandbox (`msb list` reports none), both spike volumes, and the `sandbox-mio/base:4` image. Two changes were not reverted:

1. The pre-existing cached `python` image was re-registered by the `msb save`/`msb load` round-trip in finding 11: its digest changed from `sha256:6d8e23f4c3b1` (395.5 MiB) to `sha256:b7289e1fd56c` (1.1 GiB, the uncompressed form). The image still boots correctly. `msb rmi python` followed by `msb pull python` restores the original entry.
2. A spike public key remains in `C:\Users\krist\.microsandbox\ssh\authorized_keys` from finding 13. The private half is in this spike's scratchpad directory, outside the repository. Remove the line from `authorized_keys` to revoke it.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `msb exec <name> -- cat <file>` prints correct file contents but exits 1 with "Permission denied" | Reads on a virtiofs mount in `msb` 0.6.6 on Windows fail on `close()` after a successful read; no `stat-virt` or `host-perms` mount option avoids the failure | Do not bind-mount the workspace for a microsandbox sandbox; use a `--kind disk` named volume (ext4 over virtio-blk), which returns exit code 0 for both reads and `stat` |
| `stat` on a workspace file reports "Too many levels of symbolic links" although no symlink exists | `statx` returns ELOOP on a virtiofs mount in `msb` 0.6.6 on Windows | Move the workspace onto a disk-backed named volume; the same `stat` succeeds on ext4 over virtio-blk |
| A guest web server started on `127.0.0.1` is unreachable from Windows despite a published port | Microsandbox forwards a published port to the guest interface, not the guest loopback, and offers no equivalent of Podman's `--network pasta:--host-lo-to-ns-lo` | Start in-guest services with an explicit `0.0.0.0` bind; the host side of the publish stays on `127.0.0.1` so nothing is exposed to the local network |
| An ephemeral published port (`-p 127.0.0.1:0:<guest>`) is bound but the app cannot discover which host port it landed on | `msb inspect` and `msb status` echo the requested `host_port: 0` verbatim; microsandbox has no equivalent of `podman port <name> <port>/tcp` | Allocate the host port app-side and pass it explicitly in the `-p` argument, reusing the allocation path behind `NetworkError::PortAllocation` (`src-tauri/src/network/types.rs`) |
| A sandbox created from a snapshot artifact fails every exec with "failed to resolve guest uid 0" | `msb snapshot create` captures only the writable upper layer, so booting the artifact alone yields a rootfs with no `/etc/passwd` | Do not treat a CLI snapshot as a base image; build the base image externally and import it with `msb load --tag <ref>`, or use the Rust SDK's `from_snapshot`, which re-pins the parent image |
| `msb create sandbox-mio/base:4` fails with a Docker Hub "Not authorized" error after a snapshot was named `base-v4` | `msb create` resolves a bare `<IMAGE>` argument as a registry reference; indexed snapshot names are not a valid image source | Load the image into the local cache first with `msb load --input <tar> --tag sandbox-mio/base:4`, then create from the tag |
| The workspace path label set at create time is missing when enumerating sandboxes | `msb list --format json` returns only `created_at`, `image`, `name`, and `status`; labels appear only under `active_config.labels` in `msb inspect --format json` | Call `msb inspect --format json` per sandbox to recover labels, accepting the N+1 call cost that `podman ps --format json` avoids |
| A status badge shows a raw `Running` or `Stopped` string in non-English locales | Microsandbox reports capitalised status values, unlike Podman's lowercase `created`/`running`/`exited` that `translateStatus()` (`src/lib/sandbox.ts`) maps | Normalise microsandbox status values to the existing lowercase vocabulary in the provider implementation, or add `status_<value>` keys for the new values to every file in `messages/` |
| A `podman`-style `exec -d` has no effect under microsandbox | `msb exec` has no detach flag; only `--stream`, `--tty`, `--no-tty`, `--timeout`, `--workdir`, `--env`, `--user`, and `--rlimit` exist | Background the process inside the guest shell with `nohup <cmd> &`, or register the command with `msb create --script <name>=<body>` and invoke the registered script |
| `msb` invocations from a Git Bash shell fail with paths like `C:/Program Files/Git/root/work` | MSYS2 path conversion rewrites guest-absolute arguments such as `-w /root/work` into Windows paths | Invoke `msb` from PowerShell, or set `MSYS_NO_PATHCONV=1` for the call |
| A sandbox is left in `Crashed` state after an `msb ssh serve` process is terminated | `msb ssh serve` auto-starts a stopped sandbox and owns its lifecycle, so force-killing the listener crashes the sandbox it started | Start the sandbox explicitly with `msb start` before serving so the listener does not own it, or prefer the `--stdio` `ProxyCommand` mode, which OpenSSH spawns and reaps per connection |
| `msb start <name>` right after a successful `msb create` exits 1 with "already running" | `msb create` auto-starts the sandbox (finding 3), so nothing is ever in `Stopped` state between the two calls | Treat the `already running` stderr as success, as `MsbProvider::start` (`src-tauri/src/provider/msb.rs`) does, rather than dropping the start — a stopped instance still needs it before `msb ssh serve` can answer |
| A `msb ssh serve --port 0` listener reports its ephemeral port but a `-p 127.0.0.1:0:<guest>` publish does not | The SSH listener calls `local_addr()` and prints the resolved port; the port publisher never does, so `msb inspect`/`msb status` echo the requested `0` | Rely on the printed line only for `msb ssh serve`; allocate publish ports app-side, per the `-p` readback gotcha above |
