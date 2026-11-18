---
type: improvements
title: "Host-Tool SSH Access — Deferred Improvements"
feature: host-tool-ssh-access
---
# Host-Tool SSH Access — Deferred Improvements

Work identified while planning `docs/features/host-tool-ssh-access/02_dev.md` and `03_e2e.md` that is out of the feature's v1 scope, plus seam limitations the plan works around rather than fixes. Reviewed against the tree on 2026-08-17, after `docs/features/podman-exec-transport/02_dev.md` and `docs/features/microsandbox-provider/02_dev.md` shipped the transport this feature hands to the user. Nothing here blocks implementation; `02_dev.md` remains the record of what v1 builds.

## Namespaced per-sandbox alias

The alias a user pastes into their SSH client is the bare sandbox name (`alias_name`, `src-tauri/src/services/routing.rs`), and `write_alias` prepends `Include sandbox-mio/config` to the top of `~/.ssh/config`. OpenSSH takes the first matching keyword, so a sandbox named after a host the user already has an entry for shadows that entry for every SSH client on the machine — not just for this app.

The fix is a prefixed alias (`sandbox-mio-<name>`, or a name the user cannot collide with) plus either a reserved-name check in `validate_name` (`src-tauri/src/backends/mod.rs`) or both. Deferred because the alias name is load-bearing for every shipped Mutagen path: sync beta URLs (`msb_sync_beta`), forward `remote_alias` arguments, `agent-web.spec.ts`'s assertions, and the `Proxied` block itself all key on it, and renaming it is a cross-cutting change with no user-visible gain until a collision actually bites. Documented as a gotcha in `01_ideas.md` and `02_dev.md` instead.

## Authorize the user's own public key

`install_sandbox_key` (`src-tauri/src/backends/mod.rs`) writes `/etc/ssh/authorized_keys.container` with `podman cp`, which replaces the file wholesale, so exactly one key — the app's per-sandbox identity — is ever authorized. A user who wants to reach a sandbox with their own key, from a client that does not let them pick an identity file, cannot.

Supporting it means an append-and-dedup install step (read the container's current file, add the user's key if absent), a setting for which key to offer, and a decision about what happens when the app's own key is missing from a file the user edited by hand. Deferred: every client this feature targets accepts an explicit identity file, and the alias already supplies one.

## Auto-start a stopped sandbox on an incoming connection

A stopped sandbox fails at the `ProxyCommand`, before SSH: `podman exec`/`msb exec` has nothing to exec into. Nothing in the connection path is the app's process — the client runs the `ProxyCommand` itself — so there is no hook where the app could start the container first. Closing this would mean shipping a wrapper the alias execs instead, which starts the instance and then hands over to `mio-sshd`; that wrapper is a new host-side binary in the connection path, with its own failure and supervision story. Recorded in `02_dev.md`'s lifecycle section as an accepted v1 gap.

## Non-root sandbox user

Sessions log in as `root` (`CONTAINER_USER`, `src-tauri/src/backends/mod.rs`), matching the base image's root-only layout (`WORKDIR /root/work`). Introducing a non-root sandbox user is a project-wide change — workspace mount ownership, the mise data directory, every `podman exec` path, the agent-setup credential locations, and `sshd_config.container`'s authorized-keys path — that this feature alone should not drive. Revisit if a multi-tenant or shared-host scenario appears; a single-tenant, pasta-isolated container does not motivate it.

## Credential handoff to a client's auto-installed agent

Claude Desktop installs its own Claude Code copy inside the container on first SSH connect, and authenticates it through its own login flow; VS Code and JetBrains install their own server components. Injecting host-side credentials so the user is not asked to log in a second time depends on `docs/features/shared-agent-auth/01_ideas.md` shipping *and* on confirming where that copy stores credentials — which `02_dev.md`'s spike S3 would establish as a side effect. Deferred as a resolved decision in `02_dev.md`.

## Remote-deployed SSH access spec

The `ProxyCommand` carries `--connection <machine>`, so a deployed sandbox is reachable the same way with no extra code — the exec pipe simply nests inside the SSH connection to the deploy host. `03_e2e.md` scopes its first spec to the local path per `e2e/AGENTS.md`'s tier-selection rule. A second spec covering deploy, a login against the deployed sandbox, undeploy, and a login against the returned-local sandbox (proving the block's `--connection` value was rewritten) belongs in `standard` once the local spec is green. Deferred to keep the first spec sized to the path that proves the feature works.

## Orphaned remote processes after a client dies

The in-container `sshd` itself is no longer the leak: killing the Windows OpenSSH client also kills its `podman` `ProxyCommand` child, so `sshd` exits within a second, and a hung client is reaped by the image's `ClientAliveInterval 15`/`ClientAliveCountMax 3` (image 7) after roughly two minutes ([spike/s2-path.md](spike/s2-path.md), S4). What survives is the session's own remote process tree, re-parented to PID 1 until the container stops. A desktop client's remote agent makes that more likely than Mutagen did. The candidate fix is a reaping, process-group-aware PID 1 for the container, which would also clear the bounded `[sshd] <defunct>` accumulation. Deferred until a real client shows the leftovers matter.

## `e2e/wdio.conf.ts` claims a light run writes no SSH alias

`onComplete`'s comment gating the `~/.ssh` cleanup states that "a light run writes neither the SSH alias nor the remote machine". The first half is stale: `default_backend` (`src-tauri/src/backends/mod.rs`) returns `CliBackend` regardless of `MIO_MOCK_BACKEND`, so a light-tier `create` runs `ensure_sandbox_identity` (real host `ssh-keygen`) and `ensure_sandbox_alias` (real managed config) against a scripted `podman`, whose unknown-key default makes the `podman cp` of the authorized-keys file succeed silently. Light runs therefore accumulate managed alias blocks and per-sandbox key directories that only a non-light run's cleanup removes.

Two separable pieces of work: correct the comment, and decide whether a light run should write host-global `~/.ssh` state at all (a `mock_backend_enabled()` guard around the alias write would stop it, at the cost of making the light tier diverge from the real create path). Recorded here rather than fixed in this feature because it is harness and mock-seam scope, and because `02_dev.md` depends on knowing the current behaviour either way.

## SSH access is not surfaced in the Ports view

`PortsManager.svelte` filters on `kind.kind === "exposed"`, and SSH access has no `ForwardInfo` at all — no forward is involved. Intentional for v1, since Phase D's own panel is its surface. If users start reasoning about "which ways into this sandbox are open" as one list, the Ports view is where that belongs.

## Tools resolve only inside `/root/work`

Phase B puts mise's shims first on an SSH session's `PATH` (`SetEnv PATH` in `sshd_config.container`, `src-tauri/resources/Containerfile`). A shim resolves the tool version from the directory the tool runs in, and the only mise config in a sandbox is the workspace's `/root/work/mise.toml` (`WORKSPACE_MOUNT`, `src-tauri/src/runtime/mod.rs`). A client that works anywhere else, `$HOME` included, finds the shim but gets `mise ERROR No version is set for shim: node`. Phase D's panel has to tell the user to pick `/root/work` as the remote folder.

Spike S2 tried a global fallback ([spike/s2-path.md](spike/s2-path.md), "The working-directory caveat"): symlinking `/root/.config/mise/config.toml` to `/root/work/mise.toml` made node resolve from `/root` as well. It was rejected for v1: `mise use -g` would then write into the workspace manifest the app owns, and its interaction with `mise.lock` (which `MISE_LOCKFILE=1` maintains) was not checked. Revisit if a target client insists on `$HOME` as its working directory.

## Running status for a deployed sandbox

`sandbox_ssh_access` reports `running` from `SandboxRoute::running` (`src-tauri/src/services/routing.rs`), which is true only for a sandbox the backend lists as running on its provider's local machine. A deployed sandbox therefore always reports `running: false`, even when its remote container is up and the alias works. Reporting the remote state means asking the deploy target's Podman connection for the container's status, an extra remote call per panel refresh. Deferred because the alias itself is correct for a deployed sandbox, and Phase D can word the not-running state so it does not claim a deployed sandbox is stopped.

## Withdrawn items

Four items recorded here in earlier revisions were deleted rather than deferred, because the exec transport removed the conditions that motivated them. Do not reintroduce them without first re-reading `02_dev.md`'s "Superseded plan" section: a stable per-sandbox host port (no host port exists), an `MIO_SSH_KEY_DIR` override (the key directory already routes through `crate::paths::app_data_root`, which honours `MIO_APP_DATA_DIR`), a `known_hosts` scrub keyed by `HostKeyAlias` (the alias pins its own `UserKnownHostsFile`, reset by `forget_sandbox_host_key`), and the relay-pool scanner's blindness to fixed container ports (no fixed port is published, and `poll_ports_file` sees no `sshd` listener because `sshd -i` speaks over the exec pipe's stdio).

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A picked-up deferral reintroduces a published port or a Mutagen forward for SSH | The withdrawn items above read as open work when read out of context | Read `02_dev.md`'s "Superseded plan" first; an exec-capable sandbox publishes nothing by construction (`PublishModel::ExecTunnelled`) |
| Renaming the alias to a namespaced form breaks sync and forwards | The sandbox name doubles as the Mutagen `remote_alias` and appears in `msb_sync_beta`, forward creation, and `agent-web.spec.ts` | Change `alias_name` (`src-tauri/src/services/routing.rs`) and every consumer in one pass, and re-run the full `standard` tier |
| Tightening `ClientAliveInterval` starts dropping idle desktop sessions | The server-side probe and the client's `ServerAlive*` (Phase C) are paired: both are 15 s × 3 today | Change `sshd_config.container`'s `ClientAlive*` and `render_block`'s `ServerAlive*` together |
