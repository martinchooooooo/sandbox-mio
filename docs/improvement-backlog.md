---
type: backlog
title: "Improvement Backlog"
description: "Interim task list of the most valuable open improvements, mined from every 04_improvements.md, review and in-flight feature.md; delete once every task is resolved."
---
# Improvement Backlog

This document is an interim task list, compiled on 2026-11-23 at commit `2e0e665` from every `docs/features/*/04_improvements.md`, both review documents (`docs/features/podman-egress-boundary/05_dev_review.md`, `docs/features/fastest-time-to-wow/04_review.md`), the `feature.md` of every unfinished feature, and `NOTES.md`. Tasks are ranked most valuable first. Line numbers refer to commit `2e0e665` and drift as the code changes; search for the named function when a line number no longer matches.

## How to work a task

- Take one task per agent, in its own git worktree. Read the task's source document and the root `AGENTS.md` before editing.
- A task marked **Decision needed** has an open product or design choice: present the options to the user and stop before building. A task marked **Feature** is large enough for the `feature-ideas` → `feature-dev` → `feature-implement` pipeline under `docs/features/<name>/`. Every other task is a direct fix.
- Verify with the commands in the root `AGENTS.md` (Verification Commands) that match the files touched: `pnpm check`, `cargo check` and `cargo test` in `src-tauri/`, `cargo xtask docs`, `cargo xtask e2e light`.
- On completion, tick the task's checkbox here and update the task's source document: delete the resolved entry from the `04_improvements.md`, or record the resolution against the review finding.
- Tasks sharing files are listed under "Overlapping tasks"; do not run overlapping tasks in parallel.
- When every task is ticked or moved into a feature directory, delete this file and its line in `docs/index.md`, then run `cargo xtask docs`.

## Tasks

### T1. Remote deploy is refused for every sandbox — Feature

- [x] Resolved
- **Problem:** every new sandbox gets an iron-proxy sidecar with no opt-out, and `deploy` refuses a proxied sandbox with `BackendError::ProxyDeployUnsupported` (`src-tauri/src/backends/mod.rs:622`). Remote deploy, a headline capability in the root `AGENTS.md`, is therefore unusable, and the remote e2e specs (`deploy-remote`, `base-image-remote`, the remote part of `runtime-management`) are skipped.
- **Source:** `docs/features/proxy-remote-deploy/feature.md` (status note). The existing `01_ideas.md` and `02_dev.md` there describe the removed mitmproxy sidecar and must not be implemented from.
- **Done when:** a proxied sandbox deploys to a remote Podman host with its internal network, dual-homed iron-proxy sidecar, per-binding secret files, policy reload and audit-log polling recreated on the target; the skipped remote specs run again.

### T2. A sandbox can rewrite its own egress policy and secret bindings

- [x] Resolved
- **Problem:** `apply_sandbox_policy` (`src-tauri/src/commands/net_policy.rs:66-86`) loads `.mio/net-policy.json` from the workspace, which code inside the sandbox can write. `commitSecretChange()` and `save()` in `src/lib/net-policy/NetPolicyEditor.svelte` (lines 106-119, 178-195) trigger that apply. An agent can add an attacker host to a secret binding's `hosts` plus an allow rule; the next time the user stores any secret, the sidecar swaps the real value into requests to the attacker host. A deployed sandbox has the same exposure: Mutagen syncs the remote workspace's policy file back to the laptop, and every apply is pushed live to the remote sidecar (`docs/features/proxy-remote-deploy/02_dev.md`).
- **Source:** finding 1 of `docs/features/podman-egress-boundary/05_dev_review.md` (deferred by the user to a separate feature).
- **Feature:** implemented in `docs/features/net-policy-review/` ([02_dev.md](features/net-policy-review/02_dev.md)): applies render only the accepted policy, the workspace file is a proposal reviewed as a diff, and secret values are per sandbox, which also closes a binding naming another sandbox's `secretId`.
- **Options:** (a) apply only from the app-side snapshot (`applied_policy::read_applied`, `src-tauri/src/applied_policy.rs`) and treat the workspace file as a proposal; (b) require the user to confirm a diff whenever `net_policy_drift` reports a change made on disk before any apply. Either option must also stop `apply_sandbox_policy` from re-reading the workspace file after the user reviewed it.
- **Done when:** no apply path can enforce a policy or binding host list the user has not seen; a light spec covers a drifted file with a widened binding.

### T3. Sandbox SSH aliases can shadow the user's own SSH hosts

- [x] Resolved
- **Problem:** the managed SSH `Host` entry is the bare sandbox name, inside an `Include sandbox-mio/config` line prepended to `~/.ssh/config` (`src-tauri/src/ssh/mod.rs`). OpenSSH takes the first match, so a sandbox named `github.com` hijacks `git@github.com` for every SSH client on the machine. `validate_name()` and `validate_new_name()` (`src-tauri/crates/domain/src/lib.rs`) allow dots and check no collisions.
- **Source:** "Namespaced per-sandbox alias" in `docs/features/host-tool-ssh-access/04_improvements.md`.
- **Fix shape:** the cheap fix is in `validate_new_name()`: refuse new names containing a dot, and refuse names matching a `Host` pattern already in the user's `~/.ssh/config` outside the managed blocks. A prefixed alias (`sandbox-mio-<name>`) is the complete fix but touches every Mutagen path keyed on the alias (see the gotchas table in the source document); treat the prefix as Decision needed.
- **Done when:** creating a sandbox whose name would shadow an existing or dotted host is refused with a clear error; existing sandboxes stay addressable.

### T4. A failed create leaves the container, alias and key behind

- [x] Resolved
- **Problem:** `CliBackend::create` calls `self.provision_exec_transport(...).await?` (`src-tauri/src/backends/mod.rs:562`) after the container exists, with no rollback. A failure in `install_sandbox_key` or `ensure_sandbox_alias` leaves the container, the alias block and `ssh-keys/<name>/`, so a retry with the same name fails.
- **Source:** "A failure inside `store.create` leaves the container, alias and key behind" in `docs/features/techdebt-sweep/04_improvements.md`.
- **Done when:** a failed `provision_exec_transport` removes the container, the alias (`crate::ssh::remove_alias`) and the key directory before returning the error; a light spec scripts the failure through `configureMock` (`e2e/lib/mock.ts`) and asserts a same-name retry succeeds.

### T5a. Secret stdin is written to a plaintext temp file

- [x] Resolved
- **Problem:** `run_with_stdin_file()` (`src-tauri/crates/process/src/lib.rs:663-676`) writes a call's stdin to `%TEMP%`, so the GitHub token and Claude credential injection briefly land on host disk. Since proxy-remote-deploy, every remote policy apply and every remote sidecar start also pushes all staged secret values through this path (`push_secret_files`, `src-tauri/src/proxy/remote_push.rs`).
- **Source:** "Feed secret stdin without a temp file" in `docs/features/github-cli-auth/04_improvements.md` (the entry is now titled "Capture secret stdout without a temp file" and keeps the stdout half).
- **Done when:** stdin reaches the child through a pipe or in-memory handle while keeping the guard, breaker, timeout and logging of the Podman runner; the comment explaining why the temp file existed is updated or removed.

### T5b. Credential store is unencrypted at rest — Decision needed

- [ ] Resolved
- **Problem:** credential bundles (`src-tauri/src/credentials/mod.rs`) and the sidecar's materialised secret files are plaintext under the app-data root.
- **Source:** "Encryption at rest" in `docs/features/proxy-credential-secrets/04_improvements.md` and `docs/features/shared-agent-auth/01_ideas.md`.
- **Options:** Windows DPAPI for the store; the sidecar's files must stay readable by the sidecar container, which limits what DPAPI can cover.

### T6. No CI runs the verification commands

- [ ] Resolved
- **Status:** `.github/workflows/ci.yml` is in place and runs only on a pushed tag or a manual dispatch, a deliberate narrowing of "on push and pull request" below to save runner minutes. The `checks` job has passed on a hosted runner, and the `e2e-light` job has passed a single spec there; tick this task after the first full light run reports its outcome.
- **Problem:** `.github/workflows/` holds only `release.yml`; nothing runs `pnpm check`, `cargo test`, `cargo xtask docs` or the `light` e2e tier on push.
- **Source:** "CI pipeline for the tier cadence" in `docs/features/scriptable-mock-backend/04_improvements.md`; the interim `windows-latest` option in "Headless light tier for CI" in `docs/features/parallel-worktree-e2e/04_improvements.md`.
- **Done when:** a workflow on `windows-latest` runs `pnpm check`, `pnpm --filter paseo-plugins typecheck`, `cargo check`, `cargo test` and `cargo xtask docs` on push and pull request; a second job runs `cargo xtask e2e light`. If the light job cannot pass on a hosted runner, record why in the parallel-worktree-e2e document and keep the first job. Update "Tier cadence" in the root `AGENTS.md`, which states no CI exists.

### T7. Light e2e app instances sometimes never load the UI

- [ ] Resolved
- **Problem:** in a full `cargo xtask e2e light` run, some app instances stay on `about:blank` for the 30-second `before` hook in `e2e/wdio.conf.ts`; `mio.log` stops at "cleared inheritable handles at startup". Which spec fails depends on its predecessor, and each failing spec passes when run without that predecessor. The unconfirmed suspect is the WebView2 user-data folder that every instance shares beside `src-tauri/target/debug/sandbox-mio.exe`.
- **Source:** "Some e2e app launches never load the UI" in `docs/features/techdebt-sweep/04_improvements.md`; "Light spec `activity-center-ui` fails on a clean checkout" in `docs/features/manage-dashboard-redesign/04_improvements.md`.
- **CI:** the `e2e-light` job in `.github/workflows/ci.yml` carries `continue-on-error: true` until this task is fixed; remove that line (and its `TODO(T7)` comment) once three consecutive runs pass.
- **Fix shape:** give each app session its own WebView2 user-data folder (for example through `WEBVIEW2_USER_DATA_FOLDER` set per session in `beforeSession`, `e2e/wdio.conf.ts`), then confirm with several consecutive full light runs.
- **Done when:** three consecutive full light runs pass with no `about:blank` launch failure.

### T8. The light tier writes the real `~/.ssh` and runs real `ssh-keygen` — Decision needed

- [ ] Resolved
- **Problem:** under `MIO_MOCK_BACKEND=1`, a light-spec create still runs `ensure_sandbox_identity()` and `ensure_sandbox_alias()` (`src-tauri/src/ssh/identity.rs`) against the developer's real `~/.ssh/sandbox-mio/config`. The `onComplete` comment in `e2e/wdio.conf.ts` wrongly claims a light run writes no SSH alias.
- **Source:** "`e2e/wdio.conf.ts` claims a light run writes no SSH alias" in `docs/features/host-tool-ssh-access/04_improvements.md`; first section of `docs/features/techdebt-sweep/04_improvements.md`.
- **Options:** gate the SSH writes with `mock_backend_enabled()` (the light tier then diverges from the real create path), or relocate the SSH config root per run as `MIO_APP_DATA_DIR` relocates app data (the `MIO_SSH_DIR` relocation seam now exists in `ssh_dir()`, `src-tauri/src/ssh/mod.rs`, and the harness sets it today only for `light/ssh-alias-shadow-ui.spec.ts`). Correct the `onComplete` comment either way.

### T9. Translation backlog and the unmeasured time-to-wow — Decision needed

- [ ] Resolved
- **Problem:** 131 `TODO(i18n)` markers under `src/` leave German and Russian users reading English. Five features wait on the user's copy approval before their Paraglide pass: `fastest-time-to-wow`, `github-cli-auth`, `local-models-ux`, `onboarding-story`, `sandbox-resource-monitor` (see each `docs/features/<name>/feature.md`). Phase A of `docs/features/fastest-time-to-wow/02_dev.md`, the spike measuring the baseline time-to-wow, never ran, so the metric that feature optimises is unmeasured.
- **Next step:** ask the user to approve each feature's English copy, then run each Paraglide pass through `feature-implement`; run the Phase A spike separately.

### T10. Small UX improvements

Each sub-task is a direct fix unless marked.

- [ ] **T10a. Delete downloaded models.** No command removes a downloaded GGUF or reports disk use; add both to the Models shelf with a guard against removing the serving model. Source: "Managing downloaded Models", `docs/features/local-models-ux/04_improvements.md`.
- [ ] **T10b. Redirect `/` to Home after onboarding.** `src/routes/+page.svelte` still shows the welcome flow after onboarding, so a launch lands on a page no sidebar item highlights. Check the e2e landing assumptions first. Source: "Root route as Home", `docs/features/unified-sidebar-nav/04_improvements.md`.
- [ ] **T10c. Activity panel drawn under webview panes.** The activity panel (`src/lib/activity/ActivityCenter.svelte`) renders under a shown Terminal or Agents pane; the fix needs Rust help in `src-tauri/src/workspace/mod.rs`. Source: "Activity panel under an active webview pane", `docs/features/unified-sidebar-nav/04_improvements.md`.
- [ ] **T10d. Health chip on Home cards.** Show the resource-monitor health chip on `src/routes/projects/SandboxItem.svelte`. Source: "Health indicator on the Home page cards", `docs/features/sandbox-resource-monitor/04_improvements.md`.
- [ ] **T10e. "Stop exposing" re-appears as `Auto`** — Decision needed. `unexpose_port` (`src-tauri/src/network/mod.rs`) removes the forward, and the next scan re-adds a still-listening port. Decide whether to suppress re-detection for the sandbox session. Source: `docs/features/expose-ports/04_improvements.md`.

### T11. Documentation hygiene

- [ ] Resolved
- `NOTES.md` lists four TODOs that features such as `shared-agent-auth` may already have fixed; verify each against the app and delete the resolved lines, or the file.
- Several `04_improvements.md` entries describe microsandbox or mitmproxy code removed by `docs/features/remove-microsandbox/` and `docs/features/podman-egress-boundary/` (for example the msb items in `docs/features/techdebt-sweep/04_improvements.md`, `docs/features/local-llm-reachability/04_improvements.md` and `docs/features/sandbox-resource-monitor/04_improvements.md`); mark each obsolete or delete it.
- `docs/features/fastest-time-to-wow/04_review.md` breaks the `05_dev_review.md` naming convention in `docs/features/README.md`; rename it and fix links.

### T12. Adding the SSH include can overwrite a non-UTF-8 user config

- [ ] Resolved
- **Problem:** `ensure_include_locked()` (`src-tauri/src/ssh/mod.rs`) reads `~/.ssh/config` with `read_to_string(...).unwrap_or_default()`. A file that is not valid UTF-8, for example UTF-16LE written by Windows PowerShell 5.1's `>`, or one that cannot be read for another reason, reads as empty, so the next `write_alias` replaces it with only the `Include sandbox-mio/config` line and the user's entries are lost.
- **Source:** found while designing T3; no `04_improvements.md` entry yet. T3's clash check (`user_host_matching`) already reads the same file leniently with `String::from_utf8_lossy` and fails open.
- **Fix shape:** read the bytes, refuse to rewrite (return an `SshError`, leaving the file untouched) when the content is not valid UTF-8 or the read failed for any reason other than not-found, and keep the lossy read for the T3 scan only. Overlaps T8 (the same files and the same `~/.ssh` handling), so do not run them in parallel.
- **Done when:** a non-UTF-8 `~/.ssh/config` survives a sandbox create byte for byte and the create reports a clear error (or skips the include); a unit test over a temp directory covers it.

## Overlapping tasks

| Tasks | Shared files |
|---|---|
| T3, T4, T8, T12 | `src-tauri/src/backends/mod.rs`, `src-tauri/crates/domain/src/lib.rs`, `src-tauri/src/ssh/mod.rs`, `src-tauri/src/ssh/identity.rs` |
| T7, T8 | `e2e/wdio.conf.ts` |
| T1, T2 | `src-tauri/src/proxy/`, `src-tauri/crates/net-policy/`, `src-tauri/src/applied_policy.rs` |
| T5a, T6 | none in code; T6 should land first so T5a's change is checked by CI |

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `cargo xtask docs` fails with an unlinked-document error after deleting `docs/improvement-backlog.md` or reports a broken link | The root `docs/index.md` is hand-written and links `docs/improvement-backlog.md` | Delete the `Improvement Backlog` line from `docs/index.md` in the same change as the file |
| A task's line number points at unrelated code | Line numbers were recorded at commit `2e0e665` | Search for the named function or file instead of trusting the line number |
| A fix passes `cargo xtask e2e light` but breaks against real Podman | The light tier runs on in-memory mocks (`MIO_MOCK_BACKEND=1`) | Run `cargo xtask e2e` (`standard`) once for any task touching `src-tauri/src/backends/`, `src-tauri/src/proxy/` or `src-tauri/crates/process/` |
