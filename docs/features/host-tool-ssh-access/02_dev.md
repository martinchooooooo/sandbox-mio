---
type: dev-plan
title: "Host-Tool SSH Access Dev"
feature: host-tool-ssh-access
---
# Host-Tool SSH Access Dev

This is the build plan for `docs/features/host-tool-ssh-access/01_ideas.md`: letting a host-machine SSH client — the Claude Code desktop app's "Add SSH connection" feature foremost, and any other `~/.ssh/config`-aware tool after it — attach to a sandbox over the SSH transport the app already maintains for Mutagen. **Status: nothing in this plan is implemented; every phase below is `Remaining`.** The transport it builds on is complete (`docs/features/podman-exec-transport/02_dev.md`, extended to `msb` sandboxes by `docs/features/microsandbox-provider/02_dev.md`), which is why this plan is far smaller than the revision it replaces: the container-side server, the per-sandbox keypair, the authorized-keys install, and the per-sandbox `Host` alias all ship today.

Reviewed against the tree on 2026-08-17. Most of the earlier plan's phases described a second, independent SSH path that the exec transport superseded; "Superseded plan" below records each dropped item and why, so a future session does not rebuild it.

## The concrete model

Every exec-capable sandbox already has everything a host tool needs: an `sshd` in the base image reached through `podman exec`/`msb exec` by `/usr/local/bin/mio-sshd`, an ed25519 keypair under `crate::paths::app_data_root(app)/ssh-keys/<name>/`, its public half in the container's `/etc/ssh/authorized_keys.container`, and a `Host <sandbox name>` block in `~/.ssh/sandbox-mio/config` carrying `ProxyCommand`, `User root`, `IdentityFile`, `UserKnownHostsFile`, `IdentitiesOnly yes`, and `StrictHostKeyChecking accept-new` — included from `~/.ssh/config`. This feature adds no transport, no port, no daemon, and no second identity. It adds four things:

1. A command that guarantees the block is current and reports the handoff values (`sandbox_ssh_access`), because the block's `ProxyCommand` encodes the machine the sandbox currently runs on and the app rewrites it only on paths that open a forward.
2. A `PATH` fix in the base image, so a non-interactive SSH session sees the sandbox's mise-managed toolchain.
3. Keepalives and quoted paths in the rendered alias block.
4. A panel that shows the user the alias, plus alias/key teardown at destroy so a dead sandbox stops leaving a resolvable handle behind.

There is no enabled state, no `.mio` record, and no startup restore: nothing is provisioned per sandbox at enable time, so there is nothing to persist or restore. The one hard precondition is that the container must be running, since the `ProxyCommand` is an `exec` into it.

## Superseded plan (do not reintroduce)

The earlier revision of this document specified a parallel SSH path. Each item below was dropped because the exec transport already covers it or made it counterproductive:

- **`SSH_PORT = 22`, a third `--publish 127.0.0.1::22` in `create`/`deploy`, and the matching constant in `runtime/mod.rs`.** An exec-capable sandbox publishes nothing at all (`PublishModel::ExecTunnelled`, `src-tauri/src/provider/types.rs`); reintroducing a publish would resurrect the model the project deliberately left behind.
- **`ServiceKind::Ssh` plus `forward_name`, `open_forward`, and frontend-union work.** No forward is involved: the client's own SSH connection *is* the transport. `ServiceKind` keeps `Terminal`, `AgentWeb`, and `Exposed(port)`.
- **The relay-scanner exclusion of `ZELLIJ_WEB_PORT`/`PASEO_WEB_PORT`/`SSH_PORT` by literal value.** `poll_ports_file` (`src-tauri/src/network/mod.rs`) never sees a container-side 22, because no `sshd` listens on a port — `sshd -i` speaks over the exec pipe's stdio. The relay pool itself now applies only to pre-image-6 sandboxes.
- **An `ssh-` prefix arm in `sweep_stale_forwards` (`src-tauri/src/sync/mod.rs`).** No Mutagen session named `ssh-<sandbox>` exists to sweep.
- **`RuntimeManager::prepare_ssh`** with its `/run/sshd` creation, `PermitRootLogin yes` drop-in, `ssh-keygen -A`, `authorized_keys` injection, and idempotent daemon start. `mio-sshd` creates `/run/sshd` on every connection, host keys are baked, `install_sandbox_key` installs the key at create/deploy, and `sshd_config.container` sets `UsePAM no` with pubkey auth — root login already works, as every Mutagen session against a sandbox proves. No runtime trait method, no `RuntimeError` variant, no mock addition.
- **An app-scoped keypair generated in-process, and the `MIO_SSH_KEY_DIR` override for it.** `ensure_sandbox_identity` (`src-tauri/src/backends/mod.rs`) generates a *per-sandbox* key by shelling out to the host's `ssh-keygen`, under `crate::paths::app_data_root`, which already honours the per-worktree `MIO_APP_DATA_DIR` the e2e harness sets. The in-process-generation argument lost to shipped code; the host `ssh-keygen` dependency is now a pre-existing fact of `create`, not something this feature introduces.
- **`HostKeyAlias` and a `scrub_known_hosts` helper running `ssh-keygen -R`.** The alias pins its own `UserKnownHostsFile` beside the sandbox's identity, and `forget_sandbox_host_key` deletes it whenever a fresh container takes a reused name. The user's own `known_hosts` is never touched, so nothing needs scrubbing there and no e2e harness change against it is needed.
- **`.mio/ssh.json`, the startup restore, and the running-only guard on it.** There is no per-sandbox state to persist.
- **Extending `CONFIG_LOCK` over `write_alias`/`remove_alias`.** Done since, and more thoroughly: `with_config_lock` (`src-tauri/src/ssh/mod.rs`) holds both the in-process mutex and a cross-process file lock for the whole read-modify-write.
- **A `sandbox-mio-ssh-<name>` alias class.** The alias is the sandbox name (`alias_name`, `src-tauri/src/services/routing.rs`). Its namespace cost is recorded in `01_ideas.md` and `04_improvements.md`.

Two items from the earlier plan survive, because they are still true of `render_block` today: the unquoted `IdentityFile` value, and the absent keepalives. Both are Phase C.

## Resolved Decisions

Restating every Open Question from `01_ideas.md`:

- **Does a third-party client resolve an `Include`d `ProxyCommand` alias, and what is the fallback?** Unresolved by design — this is spike S1 (Phase 0), and it gates Phases A and D. The decision it forces is recorded here rather than guessed: if S1 fails on `Include` only, the fallback is to write the managed block into `~/.ssh/config` itself for exec-capable sandboxes; if it fails on `ProxyCommand` support, no fallback exists inside this model and the feature stops at "the panel shows the block for the user to paste into their own client's config", with the publish-plus-forward model reconsidered as a separate feature. Do not start Phase D's copy until S1 has an answer.
- **Toggle or read-only panel?** Read-only panel with a refresh action. Nothing is started, provisioned, or torn down per sandbox, so a toggle would persist state that has no referent — the failure mode the earlier plan's `.mio/ssh.json` existed to work around, and which disappears entirely here. The panel's one piece of live state is whether the container is running.
- **Where does the panel live?** A section in `src/lib/workspace/WorkspaceDashboard.svelte`, sibling to `PortsManager`, rendered from a new `src/routes/projects/SshAccessCard.svelte`. The dashboard is the per-sandbox detail surface (`PackageManager`, `AgentManager`, `PortsManager` all live there); `SandboxItem.svelte` is a compact lifecycle row whose actions all fit one line, and copyable connection details do not.
- **How is a value copied?** Add `@tauri-apps/plugin-clipboard-manager` (plus the Rust crate and a `clipboard-manager:allow-write-text` entry in `src-tauri/capabilities/default.json`). No clipboard mechanism exists in `src/` today, so either route is new work; the plugin is chosen because it goes through the capability model every other privileged call in this app uses, rather than depending on the webview's own permission behaviour. `navigator.clipboard` may well work — the app's origin ends in `localhost`, which Chromium treats as a secure context — but nothing in the codebase exercises it, so it is the fallback to try only if the plugin is rejected in review, alongside a selectable read-only input.
- **Is the user's own public key authorized too?** No for v1. `install_sandbox_key` writes `/etc/ssh/authorized_keys.container` by `podman cp`, replacing it wholesale, and every target client accepts an explicit identity file. Recorded in `04_improvements.md`.
- **Alias and key teardown on destroy?** Yes — Phase E, which gives `remove_alias` its first caller.
- **Which host tools are validated?** Claude Desktop is the one exercised by hand (spike S1) and the one the panel's copy names. VS Code Remote-SSH and JetBrains Gateway need no project-side code, so they ship as "should work" and are spiked opportunistically in S3.
- **Credential handoff to a client's auto-installed agent?** Deferred, unchanged: it depends on `docs/features/shared-agent-auth/01_ideas.md` shipping and on confirming where that copy stores credentials.

Decisions forced by the current tree rather than restated from `01_ideas.md`:

- **The command refreshes the alias; it does not start the container.** `SandboxRouter::resolve` (`src-tauri/src/services/routing.rs`) rewrites the managed block, which is required because the `ProxyCommand` embeds `--connection <machine>` and a deploy or undeploy moves the sandbox. Starting the container is deliberately *not* part of the command: the panel reports the stopped state and points at the existing start control, so opening a panel never boots a sandbox the user left stopped.
- **A non-exec-capable sandbox returns a payload, not an error.** `Sandbox::supports_exec_transport` is false for a Podman sandbox created before base image 6, and that sandbox's alias is the shared *connection* alias — a login through it lands on the Podman machine. The command returns `execCapable: false` so the panel can explain the recreate prerequisite; no new `AppError` variant is added.
- **`PATH` is fixed in the image, not per session.** The client's remote agent spawns its own subprocesses, so nothing host-side can wrap them. Which mechanism lands is spike S2's job: `SetEnv` in the baked `sshd_config.container` is preferred over `/root/.bashrc`, because `sshd -i` runs over a pipe with no peer address, and bash's "source `~/.bashrc` for a remote command" special case may not trigger without the `SSH_CLIENT`/`SSH_CONNECTION` variables sshd normally derives from the socket. Either mechanism needs a `BASE_IMAGE_VERSION` bump.
- **`EXEC_TRANSPORT_MIN_IMAGE_VERSION` does not move.** Phase B bumps `BASE_IMAGE_VERSION` (`src-tauri/src/backends/mod.rs`) for the image change; the capability threshold stays at 6, so sandboxes on image 6 keep their transport and only miss the `PATH` fix.
- **Keepalives apply to every `Proxied` block, with no opt-in field.** The earlier plan made them optional to keep Mutagen's aliases unchanged; that distinction no longer exists, since every `Proxied` alias is a per-sandbox exec-transport alias and Mutagen's own long-lived forwards benefit from the same directives. One fewer field on `SshAlias`.
- **The light e2e tier really does write `~/.ssh` and a real keypair.** `default_backend` returns `CliBackend` regardless of `MIO_MOCK_BACKEND`, so a light-tier `create` runs `ensure_sandbox_identity` (real `ssh-keygen`) and `ensure_sandbox_alias` (real managed config) against a scripted `podman`, whose unknown-key default (`default_output`, `src-tauri/src/mockctl/mod.rs`) makes `podman cp` succeed silently. A light spec for the panel is therefore feasible, and the stale comment in `e2e/wdio.conf.ts`'s `onComplete` claiming a light run writes no SSH alias is recorded in `04_improvements.md`.

## Parallel-worktree constraints

- **New path the app reads or writes:** none. The keypair and pinned `known_hosts` already live under `crate::paths::app_data_root`, which honours `MIO_APP_DATA_DIR`.
- **New host port:** none — the whole point of the exec transport.
- **New host-global external resource:** none *new*, but the feature's value depends on one that already exists: the managed `~/.ssh/sandbox-mio/config` and the `Include` line in `~/.ssh/config`. `03_e2e.md`'s spec therefore stays in the host-locked `standard` tier, and any teardown touching `~/.ssh` stays inside the existing `!IS_LIGHT_RUN` gate in `e2e/wdio.conf.ts`.
- **New bundled binary:** none. The clipboard plugin is a crate and an npm package, not a `src-tauri/binaries/` asset.
- **New environment variable:** none.

## Phase 0 — gating spikes

S1 can invalidate the flagship use case; S2 decides Phase B's mechanism. Run both before Phase A. Record scripts and captured output under `docs/features/host-tool-ssh-access/spike/` and update this section's status line.

Status: `Remaining` (no spike has been run for this feature; the transport spikes it relies on are recorded in `docs/features/podman-exec-transport/02_dev.md`, Phases A and F).

- **S1 — does a third-party client resolve an `Include`d alias and honour its `ProxyCommand`?** Create a sandbox, start it, and confirm the baseline first: `ssh <sandbox name> whoami` from `cmd.exe` or PowerShell — **not** Git Bash, whose `PATH` and path conversion mask the failure modes a real client hits — must print `root`. That single command proves the `Include`, the `ProxyCommand`, the identity, and root login in one step, and it is also the exact path any client that shells out to the system client takes. Then add the same alias name in Claude Desktop's "+ Add SSH connection" and open a session. Record which of the three outcomes holds: the alias resolves; the alias is invisible (an `Include` gap — fallback is writing the block into `~/.ssh/config`); or the alias resolves but `ProxyCommand` is refused or unsupported (no fallback inside this model — see Resolved Decisions).
- **S2 — do mise-managed tools appear on `PATH` in a non-interactive session, and which mechanism fixes it?** With the same alias: `ssh <name> "which node; mise --version; echo $PATH"` after installing a tool through the app's package manager. Then determine whether `~/.bashrc` is read at all under `sshd -i` over a pipe (echo a marker from it), since that decides between the two candidate fixes: `SetEnv PATH=…` in the baked `sshd_config.container`, which is independent of shell startup files, or a shims branch in `/root/.bashrc`. Confirm the chosen fix with a rebuilt image before Phase B commits to it.
- **S3 — does a client's own remote bootstrap complete inside the container?** Claude Desktop installs its own Claude Code copy on first connect; VS Code Remote-SSH and JetBrains Gateway install their own server components. Each downloads a runtime over the sandbox's egress and writes under `/root`. Nothing in the project exercises that path. Capture what gets installed and where, since it also decides whether the deferred credential handoff has a target.
- **S4 — session durability.** Leave a session idle past any keepalive interval Phase C would set and confirm whether it survives; stop the sandbox mid-session and record what the client reports; kill the client abruptly and check for a lingering in-container `sshd` (`podman exec <name> pgrep -a sshd`), refining the finding already recorded in `docs/features/podman-exec-transport/02_dev.md`, Phase F.

## Implementation phases

### Remaining: Phase A — the handoff command

- New `SshAccess` payload in `src-tauri/src/ssh/types.rs`: `Serialize`, `rename_all = "camelCase"`, carrying the alias name, the identity path, the pinned `known_hosts` path, the literal `ProxyCommand` string, `exec_capable`, and `running`. Kept in the `ssh` module rather than in a command file, so the alias's shape and the payload describing it stay in one place.
- New `src-tauri/src/services/ssh_access.rs` (peer of `services::panes` and `services::routing`): `access(name, router, store, app)` calls `SandboxRouter::resolve` for an exec-capable sandbox — which rewrites the managed block — or `SandboxRouter::locate` otherwise, reads `status` from `SandboxBackend::list`, and assembles the payload. The `ProxyCommand` string must be produced by the same helper the alias write uses rather than re-formatted here; extract it from `ensure_sandbox_alias`/`msb_proxy_command` (`src-tauri/src/backends/mod.rs`) so the panel can never display a command that differs from the one in the file.
- New `src-tauri/src/commands/ssh.rs` holding one thin `#[tauri::command] sandbox_ssh_access(name, ...)`, matching the one-file-per-domain layout of `src-tauri/src/commands/`. Register `ssh::sandbox_ssh_access` in the `tauri::generate_handler![]` inventory in `src-tauri/src/commands/mod.rs` — a command missing there is rejected at `invoke()`.
- New `src/lib/ssh.ts`: the `SshAccess` interface plus a typed `sandboxSshAccess(name)` wrapper around `invoke`, mirroring how `src/lib/network/state.svelte.ts` wraps its commands (without a store — this payload is fetched per panel, not pushed).

### Remaining: Phase B — non-interactive `PATH` in the base image

- `src-tauri/resources/Containerfile`: apply the mechanism S2 confirmed. Preferred shape is a `SetEnv PATH=…` line in the baked `/etc/ssh/sshd_config.container`, listing mise's shims directory (`/root/.local/share/mise/shims`, derived from the `MISE_DATA_DIR` the image already sets) ahead of the default system path. The `/root/.bashrc` alternative branches on shell interactivity: today's `mise activate bash` for an interactive shell, mise's shims activation for a non-interactive one. A `/etc/profile.d` entry is not an option — a non-interactive, non-login shell reads neither profile nor login files.
- Comment the new line with the constraint, not the mechanism: the sandbox's toolchain must be visible to a client's remote agent, which runs commands non-interactively.
- Bump `BASE_IMAGE_VERSION` (`src-tauri/src/backends/mod.rs`); leave `EXEC_TRANSPORT_MIN_IMAGE_VERSION` at 6. A sandbox on image 6 keeps working over SSH and only misses the `PATH` fix, which is the right degradation: the alternative would strand it entirely.
- Verification: rebuild the base image, create a sandbox, install one tool through the app, then `ssh <name> "which node"` from `cmd.exe`.

### Remaining: Phase C — alias block hardening (`src-tauri/src/ssh/mod.rs`)

- `render_block`'s `Proxied` arm gains `ServerAliveInterval` and `ServerAliveCountMax`. No new field on `SshAlias`: every `Proxied` alias is a per-sandbox exec-transport alias, and Mutagen's own long-lived forwards over the same block benefit identically.
- Quote the `IdentityFile` and `UserKnownHostsFile` values, which `render_block` emits bare today. Both paths come from `crate::paths::app_data_root`, so a home directory containing a space (`C:\Users\John Smith\…`) splits the value into two arguments and the alias silently stops authenticating. The `Direct` arm's `IdentityFile` gets the same treatment — it is a Podman-managed key path with the same exposure.
- `e2e/lib/ssh.ts`'s `sshAliasBlock` consumers assert on keywords, so quoting does not break `agent-web.spec.ts`; any helper that reads a *value* back out of the block must strip the quotes (`03_e2e.md`, harness change 2).

### Remaining: Phase D — the "Connect from desktop apps" panel

- New `src/routes/projects/SshAccessCard.svelte`, rendered by `src/lib/workspace/WorkspaceDashboard.svelte` after `PortsManager`, taking the same `sandboxName`/`workspacePath`/`action` props its siblings take. It fetches `sandboxSshAccess` on mount and on an explicit refresh, using `createActionRunner` (`src/lib/actions.svelte`) for busy/error state and `parseError` for messages.
- Display the alias name first, then the identity path, the pinned `known_hosts` path, and the `ProxyCommand`, each with a copy button (clipboard plugin, per Resolved Decisions). Which of those a user actually needs depends on S1's outcome: the alias alone if the client resolves it, the whole block's values if the user must paste them into their own config.
- State the two preconditions in the panel: the container must be running (the payload's `running` flag drives the wording, and the panel links to nothing — the row's existing start control is the action), and a sandbox with `execCapable: false` needs recreating on the current base image.
- Strings ship hard-coded English with `TODO(i18n)` markers; the Paraglide pass is deferred until after the user approves the working feature, matching `feature-implement`'s i18n rule and the `agent-web-ui` precedent.

### Remaining: Phase E — alias and identity teardown

- `services::sandbox::destroy` (`src-tauri/src/services/sandbox.rs`) gains, in both its branches (the no-session early return and the deployed path), a best-effort `crate::ssh::remove_alias(name)` and removal of `crate::paths::app_data_root(app)/ssh-keys/<name>/`. Both are best-effort: neither failure should block a destroy the user asked for.
- Remove `#[allow(dead_code)]` from `remove_alias` (`src-tauri/src/ssh/mod.rs`) once wired; it is the caller that function's comment has been waiting for. `ensure_include`'s own `#[allow(dead_code)]` is unrelated and stays.
- One teardown covers both providers: a microsandbox sandbox uses the same alias name and the same key directory layout (`ensure_msb_sandbox_alias`, `ensure_sandbox_identity`).
- `undeploy` deliberately gets no teardown: the sandbox comes back as a local container under the same name, and its alias must survive — only its `ProxyCommand`'s `--connection` value changes, which the next `SandboxRouter::resolve` rewrites.
- This closes the cleanup candidate recorded in `docs/features/podman-exec-transport/02_dev.md`'s lifecycle section.

### Remaining: Phase F — regression pass and doc handoff

- `cargo check`, `cargo clippy`, `cargo test`, `pnpm check`, `./build e2e light`, then `./build e2e` (`standard`) — `agent-web.spec.ts` included, since Phase C changes the block it asserts on and Phase B bumps the image every standard-tier spec builds against.
- Write the new spec per `03_e2e.md`, and update `docs/features/podman-exec-transport/02_dev.md`'s lifecycle note to point at Phase E as the cleanup that shipped.

## Lifecycle / cross-cutting concerns

- **The container must be running.** The `ProxyCommand` is an `exec` into the instance; a stopped sandbox fails at the transport, before SSH. Nothing in the connection path can start it — there is no host-side listener in this model, only the client's own process — so auto-start stays out of scope (`04_improvements.md`).
- **The alias is rewritten, never accumulated.** `write_alias` replaces the block keyed by the sandbox name under a cross-process lock, so repeated panel refreshes, forward opens, and app restarts all converge on one block.
- **A deploy or undeploy moves the sandbox** and changes the `ProxyCommand`'s `--connection` value. Any client holding the alias keeps working after the next refresh; a client holding a *copy* of the `ProxyCommand` string does not, which is why the panel's copyable command is a diagnostic aid rather than the recommended handoff.
- **Authorized keys survive stop/start, not destroy.** `/etc/ssh/authorized_keys.container` lives in the instance's writable layer, installed once at create/deploy. A recreate re-runs the install and `forget_sandbox_host_key`, so a reused name does not trip a host-key mismatch.
- **One key per sandbox, host-side only.** The private half never leaves `app_data_root`; the app hands its path to the client rather than the key material.
- **An abruptly-killed client can leave one in-container `sshd`.** Recorded in `docs/features/podman-exec-transport/02_dev.md`, Phase F: the exec pipe does not propagate the client's death and `sshd_config.container` sets no `ClientAliveInterval`. It costs a process-table slot until the container stops. S4 measures whether a third-party client makes it common enough to fix.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `sandbox_ssh_access` succeeds but the client cannot connect | The container is stopped: the `ProxyCommand` execs into it, so the failure is in the transport, not in SSH | Start the sandbox; the payload's `running` flag is what the panel must surface for exactly this case |
| The panel shows a working handoff for an old sandbox, and the login lands somewhere else | The sandbox predates base image 6, so `Sandbox::supports_exec_transport` is false and its alias is the shared Podman *connection* alias, which resolves to the machine | Return and render `execCapable: false` (Phase A/D) and tell the user to recreate the sandbox |
| The alias worked yesterday and fails after a deploy | The `ProxyCommand` embeds `--connection <machine>`; a deploy or undeploy moves the sandbox and the stale block execs against the wrong machine | Re-fetch the handoff (the command refreshes the block through `SandboxRouter::resolve`); never cache the command string |
| A client's remote agent starts but cannot find node or any installed tool | mise activation is interactive-only in `/root/.bashrc`, and its hook needs a prompt | Phase B's image fix; note it only reaches sandboxes created after the `BASE_IMAGE_VERSION` bump |
| The `PATH` fix landed in `/root/.bashrc` and still does not apply | `sshd -i` runs over a pipe with no peer address, so bash's remote-command `~/.bashrc` special case may never trigger | Prefer `SetEnv` in `sshd_config.container`, which does not depend on shell startup files (spike S2 decides) |
| Authentication fails only for one developer | `render_block` emits `IdentityFile`/`UserKnownHostsFile` unquoted, and their `app_data_root`-derived paths contain a space under a home directory like `C:\Users\John Smith` | Quote both values (Phase C), and strip the quotes in any helper that reads a value back |
| A long-idle desktop session drops silently | No `ServerAliveInterval` in the rendered block, over a path that ends in a `podman exec` pipe (nested inside an SSH connection for a deployed sandbox) | Add the keepalive directives to the `Proxied` arm (Phase C) |
| A destroyed sandbox's alias still resolves, and its key directory grows collateral | `remove_alias` has no caller and `destroy` leaves `app_data_root/ssh-keys/<name>/` behind | Wire both into `services::sandbox::destroy` (Phase E), best-effort in both branches |
| Adding the clipboard call throws at runtime with a permission error | Tauri 2 gates plugin commands per capability; the plugin crate and npm package alone are not enough | Add `clipboard-manager:allow-write-text` to `src-tauri/capabilities/default.json`, whose `webviews` list already covers `main-navigation-panel` |
| A `light` e2e run leaves an alias block and a keypair behind | `default_backend` returns `CliBackend` under `MIO_MOCK_BACKEND=1`, so `create` writes a real alias and a real key against a scripted `podman`; `onComplete`'s `~/.ssh` cleanup runs only outside the light tier, by design | Key light-tier assertions on a run-unique alias, never on the absence of blocks; see `04_improvements.md` for the stale harness comment |
| A new sandbox shadows one of the user's own SSH hosts | The managed `Include` is prepended to `~/.ssh/config` and OpenSSH takes the first matching keyword | Avoid naming a sandbox after a real host; a namespaced alias is a recorded improvement, not a v1 change (it would move every shipped Mutagen path) |
