---
type: dev-plan
title: "Tech Debt Sweep — Build Plan"
feature: techdebt-sweep
---
# Tech Debt Sweep — Build Plan

Build plan for resolving the tech debt found by a 2026-11-07 audit of `NOTES.md`, `docs/techdebt.md`, every `04_improvements.md`, and the code's `TODO` markers, each checked against the code at commit `f11be30`. There is no `01_ideas.md`: the audit findings and the maintainer's decisions are recorded here directly. Status: Phases A–K implemented. Phases A–J landed as one commit each on `feat/proxy-traffic-viewer`, and Phase K closed the sweep with the `standard` tier run of 2026-11-10.

## The Concrete Model

The sweep is eleven phases grouped into six work units, each unit owning a disjoint area of the code so parallel worktrees conflict as little as possible:

| Unit | Phases | Area |
|---|---|---|
| 1 | A, B | `src-tauri/src/process/mod.rs`, `src-tauri/src/bootstrap.rs`: msb staging and msb/Mutagen call bounds |
| 2 | C | `src-tauri/src/machine/mod.rs`, `src-tauri/src/commands/machine.rs`: Podman machine bring-up |
| 3 | D, E | `src-tauri/src/network/`, `src-tauri/src/provider/`, `src-tauri/src/services/routing.rs`, `src-tauri/src/transport/`, `src-tauri/resources/Containerfile`: reachability model and base image |
| 4 | F, G | `src-tauri/src/services/panes.rs`, `src-tauri/src/services/sandbox.rs`, `src-tauri/src/ssh/mod.rs`: secrets on disk and destroy cleanup |
| 5 | H | `src-tauri/src/provider/types.rs` exec options, `src-tauri/src/credentials/`, `src/routes/credentials/+page.svelte`: secret exec environment and the GitHub token |
| 6 | I, J | `src/lib/models/catalogue.ts`, `messages/`, the Svelte files carrying `TODO(i18n)`: frontend-only hygiene |

Phase K runs after all six units merge: a stale-reference sweep, the remaining `TODO(i18n)` markers, one `standard` tier run, and the feature close-outs.

Items found by the audit to be already fixed, and therefore not in any phase: the direct `Command::new("podman")` in `src-tauri/src/network/mod.rs`, the `BackendError` dependency in `src-tauri/src/agents/config.rs`, the unlocked `ssh::write_alias`, the `PodmanCliBackend` rename, and `NOTES.md` item 5 (every process spawn now goes through `src-tauri/src/process/`).

## Resolved Decisions

1. **The publish+relay reachability model is deleted outright.** Every sandbox built from a base image older than version 6 has been deleted (maintainer decision, 2026-11-07), so no migration, compatibility branch or image-version predicate remains; a pre-version-6 sandbox, if one somehow exists, must be recreated.
2. **msb and Mutagen one-shot calls are bounded: `msb create` at 60 seconds, every other msb and Mutagen call at 30 seconds** (maintainer decision, 2026-11-07). The bound applies to calls through `SystemMsbRunner` and `SystemMutagenRunner` (`src-tauri/src/process/mod.rs`); spawned or streamed children (`exec_piped`, the Mutagen daemon, `listen_events`) stay unbounded. The decision supersedes the "unbounded" choice of `docs/features/podman-process-isolation/02_dev.md`, Resolved Decision 4.
3. **The GitHub token comes from the credential store** (maintainer decision, 2026-11-07). The token is a single-value credential under `store_dir()` (`src-tauri/src/credentials/mod.rs`), entered on the Credentials page and never sent back over IPC; it reaches every in-sandbox `mise` call that resolves or downloads tools as both `GITHUB_TOKEN` and `MISE_GITHUB_TOKEN`. The base-image build is not covered.
4. **Secret values never appear in a command line.** `SystemPodmanRunner::run` (`src-tauri/src/process/mod.rs`) logs the full argv to `mio.log`, so a secret passed as `--env NAME=value` is written to the log; secrets travel as a name-only `--env NAME` flag with the value in the child process environment. `PASEO_PASSWORD` (`prepare_agent_web` in `src-tauri/src/runtime/mod.rs`) moves to the same mechanism.
5. **msb runs from a staged copy in app data, keyed by content hash.** The bundled `msb/` resource directory is copied once into `app_data_dir()/msb/<hash>/` (`src-tauri/src/paths.rs`), which honours `MIO_APP_DATA_DIR`. The staged copy is still the bundled msb, so the "only the bundled msb ever runs" invariant of `msb_executable()` (`src-tauri/src/process/mod.rs`) holds.
6. **The guarded bring-up sequence wins.** The single Podman bring-up keeps both guards of `start_machine()` (`src-tauri/src/commands/machine.rs`): no `podman machine start` on a running machine, and no `wsl --shutdown` while another machine runs.
7. **The Zellij token file is deleted, not relocated.** The app never reads `.mio/zellij.json`; the token reaches the webview only through the pane's init script. The Paseo password, which the app does read back, moves to app data.
8. **Destroy-time cleanup is best-effort.** Removing a sandbox's SSH alias, key directory, known-hosts entry and pane state logs a warning on failure and never fails the destroy.
9. **One base-image bump carries both container hardening changes.** The sshd keepalive and the reaping PID 1 ship together as `BASE_IMAGE_VERSION` 7.
10. **Model checksums come from Hugging Face's LFS metadata** for each pinned repository and file. An upstream re-upload then fails the download loudly, which is the intended behaviour.
11. **New user-facing strings follow the `feature-implement` skill.** A phase adding UI writes `TODO(i18n)` markers; Phase K translates whatever markers remain after the merge.

## Implemented: Phase A — Stage the bundled msb outside the build output

Problem: `msb create` leaves a detached VM supervisor running from the bundled resource directory, which under a debug build is `src-tauri/target/debug/msb/`. The supervisor holds `libkrunfw.dll` open, so tauri-build cannot recopy the resource and `cargo check` fails with `os error 32` for as long as any microsandbox sandbox runs. `move_aside_locked_dev_files()` in the root `build.rs` works around the lock for `./build dev` only, not for `cargo check` or the e2e build.

- Add a staging function beside `msb_executable()` in `src-tauri/src/process/mod.rs`: hash `msb.exe`, copy every file of the bundled `msb/` directory into `app_data_dir()/msb/<hash>/` through a temporary sibling directory renamed into place (two app instances may stage at once), and skip the copy when the target directory exists.
- Call the staging function where `set_msb_resource_dir` is called (`src-tauri/src/bootstrap.rs`) and pass the staged directory instead of the resource directory.
- Prune sibling hash directories best-effort: a directory whose files cannot be opened for writing still serves a running VM supervisor and stays.
- Update the doc comments of `msb_executable()` and `msb_command()` to name the staged copy.
- Leave `move_aside_locked_dev_files()` unchanged: sandboxes started before Phase A still run from `target/debug/msb/`.

## Implemented: Phase B — Bound msb and Mutagen one-shot calls

- Give `run_capturing_via_files()` (`src-tauri/src/process/mod.rs`) a timeout argument, applied the way `run_via_blocking_process()` applies `call_timeout()`. On expiry, kill the direct child (never the detached VM supervisor) and return `std::io::ErrorKind::TimedOut` with a message naming the call and the bound.
- Add an msb timeout selector beside `call_timeout()`: 60 seconds for `create`, 30 seconds otherwise; use it in `SystemMsbRunner::run` and `SystemMsbRunner::run_with_env`.
- Bound `SystemMutagenRunner::run` at 30 seconds with the child killed on timeout.
- Log every msb and Mutagen call with its duration, matching the Podman seam's `podman starting` / `podman ok` lines, so the bounds can be tuned from `mio.log`.
- A timed-out msb or Mutagen call surfaces as an ordinary command failure; no circuit breaker like `PODMAN_UNRESPONSIVE` is added.
- Replace the "Unbounded by design" doc comment on `run_capturing_via_files()`, and mark the "Bound the `msb` and Mutagen process path" section of `docs/features/podman-process-isolation/04_improvements.md` done.
- Unit-test the timeout selector in `src-tauri/src/process/mod.rs`.

## Implemented: Phase C — One Podman machine bring-up sequence

Problem: `PodmanMachineManager::prepare` (`src-tauri/src/machine/mod.rs`, welcome flow) and `provision_podman` / `start_podman_machine` (`src-tauri/src/commands/machine.rs`, projects-page recovery) implement the same sequence twice, and `prepare` lacks both guards of `start_machine()`. A machine that is running with a dead socket reads as not ready, the welcome flow calls `podman machine start`, the start fails with "already running", and `prepare` then runs `wsl --shutdown`, which stops every WSL distribution on the host.

- Write the sequence once in `src-tauri/src/machine/mod.rs` as a free function over `&dyn MachineManager` with a step callback: WSL check and install, machine init when missing, guarded start, socket activation, `poll_ready`. Step ids stay `wsl-install`, `machine-init`, `machine-start`, `socket-activate` so `src/lib/machine/MachineSteps.svelte` needs no change.
- `prepare()` translates the callback into `MachineSetupEvent`s; `provision_podman` and `start_podman_machine` translate it into `activity` task progress and keep their `AppError` kinds (`PodmanRebootRequired`, `PodmanUnavailable`). `start_podman_machine` enters the sequence at the start step.
- Fix the reboot message in `provision_podman` (`src-tauri/src/commands/machine.rs`), which carries a run of spaces from a lost line continuation.
- Delete the "Independent of `provision_podman`/`start_podman_machine` by design" doc comment on `prepare()`, and mark the "Merge `PodmanMachineManager`'s `prepare()`" section of `docs/features/microsandbox-provider/04_improvements.md` done.

## Implemented: Phase D — Remove the publish+relay reachability model

Delete every branch that exists only for pre-version-6 sandboxes:

- `PublishModel` and `InstanceSpec.publish_model` (`src-tauri/src/provider/types.rs`), `publish_model()` (`src-tauri/src/backends/mod.rs`), the publish-relay arm of `PodmanProvider::create_instance` (`src-tauri/src/provider/mod.rs`), and the relay-rejection test in `src-tauri/src/provider/msb.rs`.
- `EXEC_TRANSPORT_MIN_IMAGE_VERSION` and every exec-capability computation: the image-version predicate in `src-tauri/src/backends/types.rs`, `sandbox_facts`, and `deploy` in `src-tauri/src/backends/mod.rs`.
- `SandboxRoute.exec_capable` and the non-exec branches of `alias_name` and `ensure_forward_alias` (`src-tauri/src/services/routing.rs`). Keep `ensure_connection_alias()` (`src-tauri/src/backends/mod.rs`): deploy and `src-tauri/src/commands/inference.rs` still use it.
- The `exec_capable` parameters of `RuntimeManager::prepare_terminal` and `prepare_agent_web` (`src-tauri/src/runtime/types.rs`), `port_transport()` (`src-tauri/src/runtime/mod.rs`), the `podman port` transport (`src-tauri/src/transport/`), and the matching code in `src-tauri/src/runtime/mock.rs`.
- The relay pool in `src-tauri/src/network/mod.rs`: `RELAY_PORT_POOL_START`, `RELAY_POOL_SIZE`, `relay_pool_publish_args()`, `is_relayed()`, `free_slot()`, the socat relay start and kill in `expose_port` / `unexpose_port`, the no-free-slot `NetworkError` variant and relay fields of `ForwardInfo` (`src-tauri/src/network/types.rs`), the `exec_capable` parameters of `expose_port` and `watch_ports_file`, the slot bookkeeping in `src-tauri/src/network/mock.rs`, and the callers in `src-tauri/src/commands/network.rs`, `src-tauri/src/commands/panes.rs` and `src-tauri/src/services/panes.rs`.
- Relay-only assertions in `e2e/test/specs/standard/expose-port.spec.ts` and `e2e/test/specs/standard/agent-web.spec.ts`, and any `KNOWN_CALLS` entry in `src-tauri/src/mockctl/mod.rs` only the relay path used.
- Docs: replace the legacy-model paragraph of `docs/architecture/transport.md` with the invariant that every Podman sandbox uses the exec transport; note in `docs/features/expose-ports/04_improvements.md` and `docs/features/podman-exec-transport/04_improvements.md` that the relay-path items are obsolete; remove `NOTES.md` item 2.

## Implemented: Phase E — sshd keepalive and a reaping PID 1

- In `src-tauri/resources/Containerfile`, add `ClientAliveInterval 15` and `ClientAliveCountMax 3` to the generated `sshd_config.container`, so a forwarding `sshd -i` orphaned by an abruptly killed client exits within about 45 seconds.
- Install `tini` and run it as the entrypoint ahead of the existing command, so exited `sshd` sessions are reaped instead of left as zombies.
- Confirm how the microsandbox provider starts the same image (`src-tauri/src/provider/msb.rs`) and that a microsandbox sandbox still boots with the new entrypoint.
- Bump `BASE_IMAGE_VERSION` to 7 (`src-tauri/src/backends/mod.rs`); update `e2e/test/specs/standard/base-image-recipe.spec.ts` if its static guard matches the changed lines.
- Mark both sshd sections of `docs/features/podman-exec-transport/04_improvements.md` done.

## Implemented: Phase F — Keep login secrets out of the workspace

- Delete the `zellij.json` write in `prepare_terminal_webview()` (`src-tauri/src/services/panes.rs`).
- In `e2e/test/specs/standard/embedded-workspace.spec.ts`, replace `waitForToken` and the "zellij.json absent before any Terminal pane" assertion with a probe that still proves the Zellij daemonize and token mint ran, without the file.
- Move the Paseo password from the workspace's `.mio/paseo.json` to `app_data_dir()/panes/<sandbox>/paseo.json`: change `read_paseo_password()` and the write in `prepare_agent_web_pane()` (`src-tauri/src/services/panes.rs`). When only the workspace copy exists, read it once, write the app-data copy, and delete the workspace copy. Confirm first that nothing inside the container reads `.mio/paseo.json`. Update the password path in `e2e/test/specs/standard/agent-web.spec.ts`.
- Delete the Zellij entry from `docs/techdebt.md`, keeping the backlog file and its introduction.

## Implemented: Phase G — SSH identity and alias cleanup on destroy

Problem: `ssh::remove_alias` (`src-tauri/src/ssh/mod.rs`) has no caller, so `destroy()` (`src-tauri/src/services/sandbox.rs`) leaves the sandbox's `Host <name>` block in `~/.ssh/sandbox-mio/config` and its key directory under app data.

- Add one cleanup helper in `src-tauri/src/services/sandbox.rs` that removes the alias with `ssh::remove_alias`, deletes the `ssh-keys/<name>/` directory created by `ensure_sandbox_identity()` (`src-tauri/src/backends/mod.rs`; share one path helper between the two), forgets the known-hosts entry, and deletes the Phase F `panes/<name>/` directory. Every step is best-effort and warn-logged.
- Call the helper from both branches of `destroy()` after the container is removed, and from the failed-create rollbacks in `create()`.
- Assert in one existing `standard` spec that destroys a sandbox that the alias block and key directory are gone afterwards.

## Implemented: Phase H — Secret exec environment and the GitHub token

- Add a secret-environment field to `ExecOpts` (`src-tauri/src/provider/types.rs`). `PodmanProvider::exec` (`src-tauri/src/provider/mod.rs`) and `MsbProvider::exec` (`src-tauri/src/provider/msb.rs`) render each entry as a name-only `--env NAME` flag and pass the values through `ProcessRunner::run_with_env`. Verify that `msb exec` accepts a name-only `--env`; if not, use whichever msb mechanism keeps the value off the command line.
- Implement `run_with_env` on `SystemPodmanRunner` (`src-tauri/src/process/mod.rs`) and on the scripted runners (`src-tauri/src/mockctl/mod.rs`), recording names but never values.
- Move `PASEO_PASSWORD` in `prepare_agent_web` (`src-tauri/src/runtime/mod.rs`) to the secret environment.
- Store the GitHub token as service `github` under `store_dir()` (`src-tauri/src/credentials/mod.rs`) with a `CredentialManifest`, so `list_credential_bundles` shows it and `delete_credential_bundle` removes it. Add a `set_github_token` command (value in, never out) in `src-tauri/src/commands/credentials.rs` and register it in `generate_handler!` (`src-tauri/src/commands/mod.rs`).
- Add a GitHub token card to `src/routes/credentials/+page.svelte`: masked input, save, delete, and the saved date.
- Pass `GITHUB_TOKEN` and `MISE_GITHUB_TOKEN` through the secret environment on every in-sandbox `mise` call that resolves or downloads: `install` and `reproduce_remote` in `src-tauri/src/runtime/mod.rs` and the agent installs in `src-tauri/src/agents/`. A missing token leaves behaviour unchanged.
- Unit-test the exec argv rendering: the name appears, the value does not.
- Record in `docs/features/sandbox-runtime-management/02_dev.md` that Phase D's remaining item (2) is implemented.

## Implemented: Phase I — Model checksums

- Fill every empty `sha256` in `src/lib/models/catalogue.ts` from Hugging Face's LFS metadata for the entry's `repo` and `file`, and delete the `TODO(bring-up)` comment. `download_gguf` (`src-tauri/src/inference/mod.rs`) already verifies any non-empty value.
- Check the e2e test model (`MIO_TEST_MODEL`, `e2e/wdio.conf.ts`) for the same gap.

## Implemented: Phase J — Translation pass

- Replace every `TODO(i18n)` literal in `src/routes/projects/NetPolicyEditor.svelte`, `src/lib/machine/MachineSteps.svelte`, `src/routes/projects/ProxyPanel.svelte`, `src/routes/projects/TrafficPanel.svelte`, `src/lib/welcome/WelcomeFlow.svelte`, `src/routes/projects/CreateSandboxItem.svelte`, `src/routes/+page.svelte`, `src/lib/activity/activity.svelte.ts` and `src/lib/providers.ts` with Paraglide keys in `messages/en.json`, `messages/de.json` and `messages/ru.json`, per the root `AGENTS.md` Translations section.
- No `TODO(i18n)` marker remains in those nine files.

## Implemented: Phase K — Integration and close-out

Runs after the six work units merge.

- Fix the two e2e harness defects that made parallel `light` runs fail on unrelated specs (found while verifying the work units; both reproduce on the untouched `19f18b1`):
  - tsx caches transformed files under the OS temp directory keyed by content, but bakes each file's own path in (`import.meta.url`), so worktrees sharing one temp directory ran each other's `e2e/wdio.conf.ts` and `e2e/lib/run-identity.ts` paths: one run slug, port window and app binary. The `e2e` arm of the root `build.rs` now gives the tier a per-worktree `TEMP`/`TMP` (`e2e/.tmp`, gitignored) through a new `run_env()`.
  - The `before` hook in `e2e/wdio.conf.ts` pinned the session to the first window whose path did not start with `/workspace`, which an unloaded `about:blank` webview satisfies; every later `execute` then failed with "Origin header is not a valid URL". The hook now waits for a loaded, non-`about:` app page, read through WebDriver's `getUrl` rather than a script.
- Repoint code comments that cite `src-tauri/src/lib.rs` for code that moved to `src-tauri/src/commands/` or `src-tauri/src/services/`: `src-tauri/src/backends/mod.rs`, `src-tauri/src/machine/mod.rs`, `src-tauri/src/provider/msb.rs`, `src-tauri/src/provider/types.rs`, `src-tauri/src/workspace/types.rs`, `src-tauri/src/mockctl/mod.rs`, `e2e/test/pageobjects/project-row.component.ts` and `e2e/test/specs/standard/agent-web.spec.ts`.
- Translate any `TODO(i18n)` marker added by Phase H.
- Correct the `docs/features/sandbox-runtime-management/feature.md` status note, which claims `runtime-management.spec.ts` still asserts flox.
- Run one `standard` tier covering `podman-startup-recovery`, `expose-port`, `agent-web`, `embedded-workspace`, `deploy-remote`, `runtime-management`, `base-image-build`, `base-image-recipe` and `microsandbox-egress-allowlist`; optionally the `heavy` `local-llm` spec for Phase I.
- When `deploy-remote` and `runtime-management` pass, set `sandbox-runtime-management` to `implemented`.
- Update `docs/architecture/` where a phase changed the picture and the phase did not already: `providers.md` (staged msb, bounded calls), `transport.md` (single reachability model), `agents.md` (GitHub token credential), `workspace-ui.md` (no secret in the workspace).
- Set this feature to `implemented` and add the `**Implement**` log entry.

Outcome (2026-11-10): the full `standard` tier passed 15 of 16 spec files. `podman-startup-recovery` failed only because its app instance never loaded the UI, and it passed 7 of 7 when re-run alone. Every Phase K spec therefore passed against real Podman, `microsandbox-egress-allowlist` on a real microVM. The intermittent never-loaded app launch also failed 4 and then 2 of 14 `light` spec files in two full runs, and each of those passed on re-run. It predates this sweep and is deferred in `docs/features/techdebt-sweep/04_improvements.md`. The optional `heavy` `local-llm` spec was not run. `docs/architecture/providers.md` also gained the guarded single bring-up of Phase C and was trimmed back under the word budget.

## Lifecycle and Cross-Cutting Concerns

**Merge order.** Unit 3 (Phases D, E) touches the most files and merges first; units 2, 1, 4, 5 and 6 follow. Expected conflicts: `src-tauri/src/services/panes.rs` (units 3, 4), `src-tauri/src/runtime/mod.rs` (units 3, 5), `src-tauri/src/process/mod.rs` (units 1, 5), `src-tauri/src/backends/mod.rs` (units 3, 4) and `messages/*.json` (units 5, 6).

**Verification tiers.** Each work unit runs `cargo check` and `cargo test` in `src-tauri/`, `pnpm check`, `./build docs` and `./build e2e light` in its own worktree. Only one `standard` or `heavy` run may run host-wide, so real-infrastructure verification happens once, in Phase K.

**Parallel-worktree constraints.** Every new path goes through app data: the staged msb directory (Phase A), the Paseo password (Phase F) and the GitHub token (Phase H, under `store_dir()`, which honours `MIO_CREDENTIALS_DIR`). No phase adds a host port, a bundled binary or a harness environment variable. Phase G removes blocks from the host-global `~/.ssh/sandbox-mio/config`, but only the destroyed sandbox's own block, which `create` wrote.

**Base-image bump.** Phase E's `BASE_IMAGE_VERSION` 7 shows the update prompt on every existing version-6 workspace. Version-6 sandboxes keep working without recreation: version 6 already carries the exec transport.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `cargo check` fails with `os error 32` on `binaries\msb\libkrunfw.dll` after Phase A | A microsandbox sandbox started by a pre-Phase-A build still runs its VM supervisor from `src-tauri/target/debug/msb/` | Stop that sandbox once, or build through `./build dev`, whose `move_aside_locked_dev_files()` (root `build.rs`) renames the locked file aside |
| A first `msb create` fails with "timed out after 60s" | The 60-second `msb create` bound (Resolved Decision 2) also covers a cold image pull on a fresh host | Retry once the image is cached; if cold creates regularly exceed the bound, raise the create allowance in the msb timeout selector in `src-tauri/src/process/mod.rs` |
| A Mutagen `sync flush` or another long exec inside a microsandbox sandbox fails after 30 seconds | Phase B bounds every non-create msb call and every Mutagen one-shot call at 30 seconds, including `msb exec` | Add a per-subcommand allowance to the msb or Mutagen timeout selector in `src-tauri/src/process/mod.rs`, as `call_timeout()` does for `podman build`. Package-install execs (`mise use`, `mise install`, an npm `install.cjs` postinstall) already have a 240-second allowance in both the podman and msb selectors, added after the first Agents open timed out on `mise use node npm:@getpaseo/cli` |
| A secret value appears in `mio.log` | The secret was passed as `--env NAME=value`, and `SystemPodmanRunner::run` (`src-tauri/src/process/mod.rs`) logs the full argv | Pass the secret through the secret-environment field of `ExecOpts` (`src-tauri/src/provider/types.rs`) added by Phase H |
| A sandbox's terminal or agent pane fails to connect after Phase D, with no exec transport available | The sandbox was built from a base image older than version 6, and the publish+relay fallback no longer exists (Resolved Decision 1) | Recreate the sandbox from the current base image |
| `mise install` in a sandbox fails with HTTP 403 from the GitHub API | No GitHub token is saved in the credential store, so the unauthenticated 60-requests-per-hour limit applies | Save a token on the Credentials page (`src/routes/credentials/+page.svelte`) |
| `embedded-workspace.spec.ts` waits forever for `.mio/zellij.json` | The spec still uses the token-file probe that Phase F removed together with the file | Use the Phase F probe in `e2e/test/specs/standard/embedded-workspace.spec.ts` |
