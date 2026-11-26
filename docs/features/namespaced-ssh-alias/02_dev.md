---
type: dev-plan
title: "Namespaced SSH Alias — Build Plan"
feature: namespaced-ssh-alias
---
# Namespaced SSH Alias — Build Plan

Build plan for giving every sandbox an SSH alias of the form `<name>.mio` instead of the bare sandbox name, so a sandbox can never take over a host entry in the user's own SSH config. It continues the "Namespaced per-sandbox alias" section of `docs/features/host-tool-ssh-access/04_improvements.md` and task T3 in `docs/improvement-backlog.md`; no `01_ideas.md` exists because the design was settled before planning. Status: Phases A to E implemented. The branch `ssh-alias-shadow` held a stopgap (a dot ban and a `Host`-clash refusal); Phase C removed the clash refusal, and the dot ban stays per Resolved Decision 5.

## The concrete model

**Problem.** `write_alias` (`src-tauri/src/ssh/mod.rs`) prepends `Include sandbox-mio/config` to the top of `~/.ssh/config`. OpenSSH keeps the first value it finds per keyword, so a managed `Host <name>` block overrides any user entry for the same host, for every SSH client on the machine. Today the alias is the bare sandbox name.

**Model.** The sandbox name and the SSH alias become two separate things. One function derives the alias from the name by appending the suffix `.mio`. Every place that writes, removes, quotes or looks up an alias goes through that function. A sandbox named `github.com` gets the alias `github.com.mio`, which no real host equals, so shadowing is structurally impossible and the stopgap guard has no job left.

**What does not change.**
- The sandbox name still names the container, the workspace path, the key directory under app data (`crate::paths::sandbox_key_dir`), the pane state directory, and the `ProxyCommand`'s `podman exec` target. Only the OpenSSH `Host` token changes.
- Mutagen sync sessions do not use the sandbox alias. Their beta endpoint is `<connection>:<path>`, keyed on the `Direct` alias that `deploy` writes for the Podman connection (`src-tauri/src/backends/mod.rs`), and `resolve_machine` (`src-tauri/src/services/routing.rs`) reads that connection back from the session's beta host as the machine id. The `Direct` alias is out of scope; see Resolved Decision 4.
- Mutagen forwards do use the sandbox alias (`forward_create` and `forward_create_reverse` in `src-tauri/src/sync/mod.rs` take it as `remote_alias`), but each open terminates and recreates the session, so a forward picks up the new alias on its next open.

**Seams that key on the bare name today and must derive the alias instead.**

| Seam | File | Role |
|---|---|---|
| `SandboxRoute.forward_alias` set in `SandboxRouter::locate` | `src-tauri/src/services/routing.rs` | Alias handed to forwards and to `SshAccess` |
| `expose_port` passes the bare sandbox name to `forward_create` as the alias | `src-tauri/src/network/mod.rs` | Found by spike A; it takes no alias, so it needs the derivation or an alias parameter |
| `ensure_sandbox_alias` | `src-tauri/src/ssh/identity.rs` | Writes the `Proxied` block; its `alias` field is the bare name |
| `render_known_hosts` and `pin_host_keys` | `src-tauri/src/ssh/identity.rs` | Pinned `known_hosts` lines are keyed `<alias>,[<alias>]:*` |
| `remove_alias` call in `cleanup_host_state` | `src-tauri/src/services/sandbox.rs` | Destroy teardown |
| `remove_alias` call in `rollback_create` | `src-tauri/src/backends/mod.rs` | Failed-create teardown |
| `ssh::connect` host argument, passed `req.name` by the terminal host | `src-tauri/src/terminal/mod.rs`, `src-tauri/src/terminal/ssh.rs` | The russh client looks the server key up in the pinned file under this host string |
| `SshAccess.alias` | `src-tauri/src/services/ssh_access.rs`, `src-tauri/src/ssh/types.rs` | What the "Connect from desktop apps" panel shows |

The `ProxyCommand` builders (`podman_proxy_command`, `sandbox_proxy_command` in `src-tauri/src/ssh/identity.rs`) keep taking the bare name, because they address the container, not the alias.

## Resolved Decisions

1. **Which suffix?** `.mio`, appended as a trailing label (`<name>.mio`). A suffix keeps the name readable in a client's host list, and a dotted alias is an ordinary hostname form. Phase A checked `.mio` against the IANA root zone list and it is not delegated, so the suffix stays; the fallback was the reserved `.invalid` suffix of RFC 2606, which cannot resolve but reads poorly.
2. **Migrate sandboxes that exist before the change?** No. The user decided to skip legacy migration. A sandbox created before the change keeps its bare-alias block in `~/.ssh/sandbox-mio/config` until the block is removed by hand, and keeps shadowing whatever it shadowed. On its next pane open or panel fetch it also gains a `<name>.mio` block, and destroy removes only the `<name>.mio` block. Removing the orphaned bare block automatically is recorded in `04_improvements.md`.
3. **Keep the stopgap guard?** No. Phase C deletes the `Host`-clash scan, its error variant and its service check. The guard's name-validation behaviour is restored to what `main` had, subject to Resolved Decision 5.
4. **Namespace the `Direct` alias too?** No, and it stays unprotected. The `Direct` alias is named after the user's Podman connection, and the deployed-state logic reads the connection back from the Mutagen beta host, so renaming it breaks the round-trip. Recorded in `04_improvements.md`.
5. **Do dotted names become valid again?** Conditionally. `validate_name` in `src-tauri/crates/domain/src/lib.rs` always allowed dots and still does for existing sandboxes. The branch added `NameHasDot` to `validate_new_name` only to prevent shadowing, which the suffix makes pointless. Spike S2 showed a dotted name does not work end to end: Mutagen rejects `.` in the session names the app derives from the sandbox name (`agentweb-<name>`, `expose-<name>-<port>`, `inference-<name>`, and the deploy sync session `<name>`). So `NameHasDot` stays, with a reworded message that cites Mutagen session names instead of SSH hosts; Phase C rewords it rather than deleting it, and the folder-mode dot replacement in `CreateSandboxItem.svelte` stays. Lifting the ban needs a collision-free encoding of the name into every session name, recorded in `04_improvements.md`.
6. **Keep the `MIO_SSH_DIR` relocation seam?** Yes. `ssh_dir()` in `src-tauri/src/ssh/mod.rs`, `E2E_SSH_DIR` and `SSH_DIR_SPEC_MARKER` in `e2e/lib/ssh.ts`, and the gate in `spawnDriver` (`e2e/wdio.conf.ts`) stay: the rewritten light spec needs a seeded user config, and task T8 builds on the seam. Task T12 (non-UTF-8 user config overwrite) also stays in the backlog, with its `Source` line corrected because the lossy-read scan it cites is deleted.
7. **Rename the spec and marker?** No. `e2e/test/specs/light/ssh-alias-shadow-ui.spec.ts` keeps its filename, because `SSH_DIR_SPEC_MARKER` is a substring match on that path. Its contents are rewritten in Phase D.
8. **Frontend change?** None expected. The panel renders the alias string the backend returns (`src/lib/api/ssh.ts`, the `data-ssh-alias` element read by `e2e/test/pageobjects/workspace-dashboard.page.ts`), and no frontend code builds an alias from a name. Confirm with a search before closing Phase B.
9. **Parallel-worktree impact.** No new host-global resource, path, port, environment variable or bundled binary. The SSH alias was already host-global; only its spelling changes, and specs keep keying assertions on run-unique sandbox names, so the derived alias is run-unique too. The existing `~/.ssh` cleanup gating in `e2e/wdio.conf.ts` is untouched.

## Implemented: Phase A — Spike

Throwaway reproduction under `docs/features/namespaced-ssh-alias/spike/` (`probe.sh`), recorded in `docs/features/namespaced-ssh-alias/spike/01_dotted-alias-and-names.md` (run 2026-11-26, verbatim output). Two questions, both on this Windows host:

- **S1 — does Mutagen accept a dotted alias in a forward URL?** Hand-write a managed block `Host probe.mio` with a `Proxied`-style `ProxyCommand` against a running sandbox, then run `mutagen forward create` with destination `probe.mio:tcp:127.0.0.1:<port>`. Also confirm `ssh -G probe.mio` resolves the block, and that Claude Desktop (or any strict client used in the host-tool-ssh-access spike) connects with the alias. Also check `.mio` against the IANA root zone list; if it is delegated, adopt `.invalid` (Resolved Decision 1) and carry that suffix through every later phase.
- **S2 — does a sandbox with a dot in its name work end to end?** Create `a.b` through the app with the guard branch's dot ban temporarily disabled, then exercise terminal, agent-web, expose-port and destroy. The likely failure point is the Mutagen session name, which `forward_create` and `sync::create` set to the sandbox name. Settles Resolved Decision 5.

Exit criteria: S1 passes (or the suffix changes), and S2 has a recorded answer.

**Results.**
- **S1 passed.** Mutagen 0.18.1 created a forward to `probe.mio:tcp:127.0.0.1:7777` and carried data through it; `ssh -G probe.mio` resolved the block and both Git for Windows OpenSSH and Windows OpenSSH connected with the dotted alias. `.mio` is not delegated, so the suffix stays. Claude Desktop itself was not run.
- **S2 failed at Mutagen session names**, not at the alias, the container, the proxy network or Herdr, all of which accept a dotted name. `NameHasDot` therefore stays (Resolved Decision 5, updated above).
- **A seam the plan missed:** `expose_port` in `src-tauri/src/network/mod.rs` forwards through the bare sandbox name, so Phase B must derive the alias there too (added to the seam table).

## Implemented: Phase B — Alias derivation and call sites

1. Add one public function that maps a sandbox name to its alias, plus the suffix constant, in `src-tauri/src/ssh/identity.rs` (the module that already owns the per-sandbox alias and known-hosts logic). Doc comment names the invariant: the alias never equals a bare sandbox name.
2. `ensure_sandbox_alias` (`src-tauri/src/ssh/identity.rs`): the `Proxied` block's `alias` field becomes the derived alias. Its `ProxyCommand` still takes the bare name.
3. `render_known_hosts` and `pin_host_keys` (`src-tauri/src/ssh/identity.rs`): key the pinned lines by the derived alias. `pin_host_keys` already rewrites when the rendered text differs, so a pre-existing pinned file heals on the next panel fetch or start.
4. `SandboxRouter::locate` (`src-tauri/src/services/routing.rs`): set `forward_alias` from the derived alias. Update the stale comment "Every sandbox is reached through an alias named after itself."
5. `cleanup_host_state` (`src-tauri/src/services/sandbox.rs`) and `rollback_create` (`src-tauri/src/backends/mod.rs`): call `remove_alias` with the derived alias.
6. Native terminal: `TerminalHost::attach` (`src-tauri/src/terminal/mod.rs`) passes `req.name` as the russh host string, and `Client::check_server_key` (`src-tauri/src/terminal/ssh.rs`) looks that string up in the pinned file. Pass the derived alias instead, through a new or existing `AttachRequest` field (`src-tauri/src/terminal/types.rs`, populated in `src-tauri/src/services/panes.rs`). Keep `req.name` for every container-facing call (`exec_command`, `create_session`, `session_name`). The mock terminal (`src-tauri/src/terminal/mock.rs`) needs no change unless it asserts on the host string.
7. Update the unit tests in `src-tauri/src/ssh/identity.rs` (known-hosts rendering) and add tests for the derivation function: appends the suffix, never returns the input, leaves a dotted name's own dots intact.
8. `MutagenNetworkManager::expose_port` (`src-tauri/src/network/mod.rs`) takes no alias and used to pass the bare sandbox name to `forward_create`, found by spike A. It derives the alias with the same function, for both the forward destination and `ForwardInfo.connection`; the Mutagen session name (`forward_name`) stays built from the bare name, as do the sync session names, because Mutagen rejects `.` in session names.
9. Search `src/` for any string that builds an alias from a sandbox name (Resolved Decision 8); expect none.

**As built.** The function is `sandbox_alias` and the suffix constant `ALIAS_SUFFIX` in `src-tauri/src/ssh/identity.rs`. `pin_host_keys` keeps its bare-name signature and derives the alias inside, so its callers did not change. The terminal gets the alias through a new `AttachRequest.alias` field filled from `SandboxRoute.forward_alias` in `src-tauri/src/services/panes.rs`; `ssh::connect` receives it as the host string. The `src/` search found no alias construction: the panel renders `access.alias` only.

## Implemented: Phase C — Delete the stopgap guard

Everything below was added on `ssh-alias-shadow` and is removed here. Commits on the branch may be reverted or the files edited back; the end state is what matters.

- `src-tauri/src/ssh/mod.rs`: delete `UserHostMatch`, `user_host_matching`, `find_user_host_in`, `scan_file`, `resolve_include`, `scan_config`, `split_args`, `host_line_match`, `wildcard_match`, the include-depth constant, and the unit tests covering them. Keep `ssh_dir()` and the `MIO_SSH_DIR` doc comment.
- `src-tauri/crates/domain/src/types.rs`: delete `ShadowsSshHost`. `NameHasDot` stays per Resolved Decision 5 with its message reworded to cite Mutagen session names; the `InvalidName` message keeps its branch wording.
- `src-tauri/crates/domain/src/lib.rs`: `validate_new_name` is the reserved-suffix check plus the dot check, with a doc comment that no longer mentions SSH hosts; `new_name_tests` stays.
- `src-tauri/src/services/sandbox.rs`: delete the clash check in `create`.
- `src/routes/projects/CreateSandboxItem.svelte`: unchanged, the dot replacement stays because `NameHasDot` stays.
- Confirm with a search that no remaining reference names `user_host_matching`, `ShadowsSshHost` or the removed helpers.

**As built.** Done as listed; the svelte dot replacement stays, with its comment corrected. Phase E removed the remaining references in the backlog, the host-tool-ssh-access docs and `docs/architecture/transport.md`; removed identifiers now survive only in this plan. Phase D rewrote `e2e/test/specs/light/ssh-alias-shadow-ui.spec.ts`, which had still asserted the old refusal cases. `cargo test` passes for the workspace.

## Implemented: Phase D — Tests

- `e2e/lib/ssh.ts`: add `sandboxAlias(name)` mirroring the Rust derivation, with a comment naming the Rust constant to keep in step. Make `sshAliasBlock` take the sandbox name and look up the derived alias, or have call sites pass `sandboxAlias(name)`; pick one and apply it everywhere.
- `e2e/test/specs/standard/agent-web.spec.ts`: assertions on the managed block and on destroy removal use the derived alias. This is also the real-Mutagen, real-`podman exec` proof that forwards work over a dotted alias.
- `e2e/test/specs/light/host-tool-ssh-access-ui.spec.ts`: the three `sshAlias()` assertions compare against `sandboxAlias(name)` (two local, one deployed).
- `e2e/test/specs/light/sandbox-create-rollback-ui.spec.ts`: the two `sshAliasBlock` assertions follow the same helper change.
- `e2e/test/specs/light/ssh-alias-shadow-ui.spec.ts`: rewrite. Seed the relocated user config (still gated by `SSH_DIR_SPEC_MARKER`) with entries such as `Work-Laptop` and `github.com`. Cases: create a sandbox named like a seeded host and expect success; the managed config holds a `<name>.mio` block and no bare block; the user config is byte-identical apart from the one leading `Include` line the app adds; `ssh -F <seeded config> -G <seeded host>` still reports the seeded `hostname` (offline, no connection made); `ssh -F <seeded config> -G <name>.mio` reports the sandbox's `proxycommand`; the panel alias equals `sandboxAlias(name)`. Drop the refusal cases and the legacy-dotted-sandbox case. `NameHasDot` stays, so the spec instead asserts the exact refusal of a dotted name.
- Run `cargo test` for `ssh`, `domain` and `terminal`, `npm run check` if the Svelte file changed, and the light tier; the standard-tier `agent-web.spec.ts` run is the end-to-end gate and needs the host lock.

**As built.** `sshAliasBlock(name)` takes the sandbox name and derives the alias itself, so the rollback, agent-web and shadow call sites pass the bare name; `readAliasBlock(alias, file)` takes a literal alias and an optional file, for the bare-block checks and the relocated managed config. `sandboxAlias` and `ALIAS_SUFFIX` in `e2e/lib/ssh.ts` mirror `src-tauri/src/ssh/identity.rs`. `agent-web.spec.ts` also asserts that no bare-name block is written. The rewritten shadow spec seeds `Work-Laptop`, `github.com` and a `Host *` entry, creates a sandbox named `Work-Laptop`, and asserts the `Work-Laptop.mio` block, no bare block, a user config equal to the seed plus the one leading `Include` line, and the panel alias. It resolves hosts with `ssh -F config.probe -G <host>` where the probe is the seeded user config with the managed file inlined at the `Include` line: ssh resolves a relative `Include` against the real `~/.ssh`, and Git's MSYS `ssh` cannot read the drive-letter path an absolute `Include` would need, so neither form is portable. The refusal case uses `github.com` and the exact `NameHasDot` text. Verified by the light specs and the standard `agent-web` run.

## Implemented: Phase E — Documentation

- `docs/architecture/transport.md`: the invariants state the positive rule (a sandbox's SSH alias is derived from its name with a namespace suffix and never equals a real host name) and keep the dot rule with Mutagen session names as its reason; the Exec Transport section says the alias is the name plus the suffix; `sources` lists this plan and `generated` is refreshed.
- `docs/improvement-backlog.md`: T3 is rewritten as resolved by this plan, T12's `Source` and fix shape no longer cite the removed `user_host_matching`, and the overlapping-tasks row drops the resolved T3 and T4.
- `docs/features/host-tool-ssh-access/04_improvements.md`: the "Namespaced per-sandbox alias" section is a pointer to this feature, and its stale gotcha row about renaming the alias is replaced. The gotcha rows in `docs/features/host-tool-ssh-access/01_ideas.md` and `docs/features/host-tool-ssh-access/02_dev.md` that told readers to avoid naming a sandbox after a real host are marked superseded.
- `src-tauri/AGENTS.md`: the name-validation gotcha row is back to its `main` wording plus the dot rule, the `ssh/` module description names the alias derivation, and a gotcha row covers passing a bare name where an alias is wanted. `AGENTS.md` (root) keeps its `MIO_SSH_DIR` line, and `e2e/AGENTS.md` gained one gotcha row, found during the Phase D standard run, for `forwardList` running without a staged `tools/mutagen.exe`.
- `docs/features/namespaced-ssh-alias/04_improvements.md` holds the orphaned bare-block, Mutagen session-name and `Direct` alias sections, and needed no correction.
- `feature.md` is `implemented` and `docs/features/log.md` carries the `**Implement**` entry.

## Lifecycle and cross-cutting concerns

- **Create.** `provision_exec_transport` writes the derived alias; `rollback_create` removes it. Both must use the same derivation, or a failed create leaves a block behind.
- **Every pane and port open.** `SandboxRouter::resolve` rewrites the block on each call; the derived alias keeps that idempotent.
- **Deploy and undeploy.** The `Proxied` block's `ProxyCommand` carries `--connection`, and the alias token is unchanged by either, so the block survives undeploy as before. The `Direct` alias is untouched.
- **Destroy.** `cleanup_host_state` removes the derived alias block only.
- **Pre-existing sandboxes.** See Resolved Decision 2: they gain a second block on first use and keep their bare block.
- **Host-key pinning.** A pinned `known_hosts` file written under the bare name is rewritten under the derived alias the next time `pin_host_keys` sees a running container.
- **Live forwards.** A forward created before the change keeps the bare-alias URL and keeps working while the bare block exists; its next terminate-and-create moves it to the derived alias.

## Out-of-scope improvements (for `04_improvements.md`)

- **Remove the orphaned bare-alias block of a pre-existing sandbox.** A one-line `remove_alias` of the bare name when the derived alias is written would end the residual shadowing. Deferred by Resolved Decision 2.
- **Names Mutagen rejects as session names.** alidate_name (src-tauri/crates/domain/src/lib.rs) allows _ and a leading digit, which Mutagen session names reject, so such a sandbox cannot create its agent-web or expose-port forwards (orward_name in src-tauri/src/network/mod.rs), and cannot deploy. Closing it needs a collision-free encoding of the name into session names, which would also let NameHasDot go. Deferred because it predates this feature.
- **Namespace the `Direct` alias.** Needs the machine id decoupled from the Mutagen beta host before the alias can be renamed. Deferred by Resolved Decision 4.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A new code path passes the sandbox name where an alias is wanted and the bare name leaks back into `~/.ssh/sandbox-mio/config` | The sandbox name and the SSH alias are different strings, but both are plain `String` values at every call site | Derive the alias with the single function in `src-tauri/src/ssh/identity.rs` for anything that writes, removes, quotes or looks up an alias; pass the bare name only to container-facing calls |
| Native terminal handshake fails with a host-key error on a sandbox whose pinned file looks correct | The russh client in `src-tauri/src/terminal/ssh.rs` looks the server key up under the host string it is given, and the pinned lines are keyed by the derived alias | Pass the derived alias as the host string in `src-tauri/src/terminal/mod.rs`; keep `req.name` for `exec_command`, `create_session` and `session_name` |
| `ssh <name>` no longer reaches a sandbox created after the change | Only the derived alias `<name>.mio` has a managed block; the bare name is no longer written for new sandboxes | Connect with `<name>.mio`, the value the "Connect from desktop apps" panel shows |
| A sandbox created before the change still shadows a user host after the change | No migration: its bare `Host <name>` block stays in `~/.ssh/sandbox-mio/config` and destroy removes only the derived block | Delete the stale block by hand, or implement the improvement recorded in `04_improvements.md` |
| `forward create` fails for `<name>.mio` while `ssh <name>.mio` works | A Mutagen URL parser or session-name rule rejecting the dotted form, which only a real Mutagen run shows | Spike S1 passed on Mutagen 0.18.1 (`docs/features/namespaced-ssh-alias/spike/01_dotted-alias-and-names.md`); after a Mutagen upgrade, re-run `probe.sh` and, if the dotted form fails, change the suffix and carry that choice through every call site |
| A light spec's seeded SSH config is ignored | `MIO_SSH_DIR` is applied only when the spec path contains `SSH_DIR_SPEC_MARKER` (`e2e/lib/ssh.ts`, `spawnDriver` in `e2e/wdio.conf.ts`) | Keep the rewritten spec's filename `ssh-alias-shadow-ui.spec.ts`, or change the marker and the gate together |
| `sandboxAlias` in `e2e/lib/ssh.ts` drifts from the Rust suffix and specs fail on alias mismatches | The TypeScript helper mirrors a Rust constant by hand | Keep the comment naming the Rust constant; the panel-alias assertion in `host-tool-ssh-access-ui.spec.ts` fails loudly when the two disagree |
