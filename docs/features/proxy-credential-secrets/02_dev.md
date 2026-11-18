---
type: dev-plan
title: "Proxy Credential Secrets: Build Plan"
feature: proxy-credential-secrets
---
# Proxy Credential Secrets: Build Plan

This document is the build plan for keeping the GitHub token out of Podman sandboxes that sit behind an iron-proxy sidecar. It continues `docs/features/proxy-credential-secrets/01_ideas.md` and supersedes it wherever the two disagree. Each such sandbox holds a fixed, GitHub-shaped placeholder, and the sidecar swaps the placeholder for the real token, read from the host credential store, only in the `Authorization` header of requests to GitHub's hosts. The mechanism was proven end to end by the 2026-11-18 spike (`docs/features/github-cli-auth/spike/proxy-swap.md`). Status: Phases A, B and C are implemented; phase D is `Remaining`.

## The Concrete Model

### The app-owned GitHub binding

The iron-proxy config renderer `render()` (`src-tauri/src/proxy/iron_config.rs`) gains a second input beside the workspace policy: a list of app-owned bindings, each an `AppBinding` struct (file name, placeholder, hosts, headers) defined in `src-tauri/src/proxy/iron_config.rs`. The GitHub binding is the only app-owned binding. The renderer emits each app-owned binding as one more entry in the `secrets` transform, in the same shape as a workspace binding:

- **Source:** `file` at `/etc/iron-proxy/secrets/mio-github` (`CONTAINER_SECRETS_DIR` in `src-tauri/src/proxy/iron_config.rs`). The file name `mio-github` contains a lowercase letter and a hyphen, so the file name can never collide with a workspace binding's file, whose name is a `[A-Z0-9_]` environment variable.
- **Rules:** `github.com`, `api.github.com`, `uploads.github.com`, `gist.github.com`, `raw.githubusercontent.com`.
- **Replace:** `proxy_value` is `GITHUB_PLACEHOLDER`, `match_headers` is `["Authorization"]`, `match_query` and `match_body` are `false`.

The binding never appears in `.mio/net-policy.json`, never passes through `net_policy::validate()` (`src-tauri/src/net_policy.rs`), and never adds an allow rule: the workspace policy alone decides whether GitHub is reachable at all.

### When the binding is rendered

`github_binding()` in `src-tauri/src/proxy/iron.rs` decides, for one sandbox, whether the binding is rendered and returns the token when it is. The binding is rendered when both hold:

- A GitHub credential is saved (`credentials::load_secret(app, GITHUB_SERVICE)` in `src-tauri/src/credentials/mod.rs` returns a non-empty value).
- The sandbox is not opted out (`github::load_state()` in `src-tauri/src/credentials/github.rs` over `paths::sandbox_github_state_path()` in `src-tauri/src/paths.rs`).

`apply_policy_locked()` in `src-tauri/src/proxy/iron.rs` calls `github_binding()` on every apply, so a create, a policy-editor apply and a GitHub refresh all render the same desired state. When the binding is rendered, the token is written to `<app_data_root>/proxy/<sandbox>/secrets/mio-github` with its exact bytes; when the binding is not rendered, the file is deleted.

### The placeholder

`GITHUB_PLACEHOLDER` in `src-tauri/src/credentials/github.rs` is `ghp_MioPlaceholder0000000000000000000000`: `ghp_` plus 36 characters, the value the spike ran with. Every sandbox shares the constant. The placeholder carries no secret; only a sidecar holding the GitHub binding can turn the placeholder into the token, and only for the four GitHub hosts.

### Sidecar mode versus plaintext mode

A sandbox is in **sidecar mode** when `proxy::sidecar_engine()` (`src-tauri/src/proxy/mod.rs`) reports `ProxyEngine::IronProxy`; every other sandbox (microsandbox, Podman without a proxy, a pre-boundary mitmproxy sidecar) stays in **plaintext mode**, today's behaviour. The helper `github_sidecar_mode(app, sandbox)` in `src-tauri/src/services/github_access.rs` is the one place that decision is made.

| Consumer | Sidecar mode | Plaintext mode |
|---|---|---|
| `APPLY_SCRIPT` stdin (`src-tauri/src/credentials/github.rs`) | `GITHUB_PLACEHOLDER` | The real token |
| mise secret env (`GITHUB_TOKEN`, `MISE_GITHUB_TOKEN`) | `GITHUB_PLACEHOLDER` while the binding is rendered, empty when opted out (Resolved Decision 15) | The real token, opt-out ignored as today |
| Manage card (`src/lib/workspace/GithubAccess.svelte`) | "token kept securely" | "token stored in sandbox" |

### Refresh paths

- **Reconcile of one running sandbox** (`reconcile_locked()` in `src-tauri/src/services/github_access.rs`): in sidecar mode, an `Apply` or `Remove` action first calls `iron::refresh_github_binding()`, then runs the in-sandbox script. The binding is therefore live before `gh auth login` validates the placeholder online.
- **Credential save, login capture or delete** (`set_github_token`, `capture_github_login`, `delete_credential_bundle` in `src-tauri/src/commands/credentials.rs`): `spawn_reconcile_all()` first refreshes the binding of every sandbox in sidecar mode, running or stopped, then reconciles the running ones. A stopped sidecar gets its files rewritten without being started, so the sidecar boots with the current token or without one.
- **Sandbox start** (`start_sandbox` in `src-tauri/src/commands/sandbox.rs`): unchanged; the existing `spawn_reconcile()` covers the gh side, and the sidecar's files were already kept current by the credential-change refresh.

```mermaid
sequenceDiagram
    participant UI as Credentials page
    participant Cmd as commands/credentials.rs
    participant Rec as services/github_access.rs
    participant Iron as proxy/iron.rs
    participant Side as iron-proxy sidecar
    participant Box as sandbox
    UI->>Cmd: set_github_token
    Cmd->>Rec: spawn_reconcile_all
    loop every sidecar-mode sandbox
        Rec->>Iron: refresh_github_binding
        Iron->>Side: write mio-github + config.yaml, reload if running
    end
    loop every running sandbox
        Rec->>Box: APPLY_SCRIPT with GITHUB_PLACEHOLDER on stdin
        Box->>Side: gh validation request carrying the placeholder
        Side->>Side: swap placeholder for token (Authorization only)
    end
```

## Superseded Approach

The previous `02_dev.md` (removed in the 2026-11-18 rewrite of `01_ideas.md`) planned to extract the Claude OAuth access token from its credential bundle (`/root/.claude/.credentials.json` inside `home.tar.gz`) through a descriptor table in a new `src-tauri/src/credentials/secret_extract.rs`, a `SecretSource` enum in `src-tauri/src/proxy/types.rs`, and mitmproxy injection rules in a planned `src-tauri/src/proxy/rules.rs`; a throwaway `spike/tar_extract_spike/` crate proved the tar extraction. The approach was dropped for two reasons. First, `podman-egress-boundary` replaced the Podman mitmproxy sidecar with iron-proxy and deleted the injection seams the plan hooked into. Second, an OAuth agent refreshes its own token, and the refresh response carries a new token that a request-only swap cannot keep out of the sandbox. Do not reintroduce `secret_extract.rs`, `SecretSource` or tar extraction for this feature; the OAuth goal lives in `04_improvements.md`, "Agent OAuth tokens behind the sidecar".

## Resolved Decisions

1. **Placeholder shape** (`01_ideas.md` Open Question "Placeholder shape"): the GitHub-shaped constant `ghp_MioPlaceholder0000000000000000000000`, stored as `GITHUB_PLACEHOLDER` in `src-tauri/src/credentials/github.rs`; `placeholder()` in `src-tauri/src/net_policy.rs` is not reused, because only the GitHub-shaped form was proven with gh and nothing is gained by testing the other form.
2. **`require` on the GitHub binding** (Open Question "`require`"): not set; requests to GitHub hosts without the placeholder pass through untouched, so a credential the user brings themselves and anonymous public clones keep working.
3. **Host list** (Open Question "Host list"): `github.com`, `api.github.com`, `uploads.github.com`, `gist.github.com` and `raw.githubusercontent.com`; `raw.githubusercontent.com` is added by Resolved Decision 15, and `gist.github.com` is added because `gh auth setup-git` registers the gh credential helper for `https://gist.github.com` too, so a gist clone or push sends the placeholder there, while `codeload.github.com` is left out because archive downloads reach it only through a signed redirect from `api.github.com` that carries no `Authorization` header. The `gist.github.com` swap is unproven by the spike and is covered by `03_e2e.md`.
4. **microsandbox** (Open Question "microsandbox"): stays in plaintext mode; msb's `--secret` Basic-auth handling is untested, and microsandbox moves to the placeholder only when microsandbox sandboxes run iron-proxy, tracked in `04_improvements.md`, "microsandbox start environment merge".
5. **Traffic panel** (Open Question "Traffic panel"): no change; `translate()` (`src-tauri/src/proxy/audit.rs`) reduces the audit row to a `secret_injected` boolean, so a swapped GitHub request already shows the generic secret marker and the token file's path never reaches the frontend.
6. **Binding location**: app-rendered in `render()` (`src-tauri/src/proxy/iron_config.rs`), never stored in `.mio/net-policy.json`, because code inside the sandbox can write the workspace and could rebind the credential to a host it controls.
7. **Value source**: the credential store, read by `github_binding()` at every apply; no copy goes into `<app_data_root>/proxy/secrets.json`, so login, rotation and delete keep a single plaintext store plus the per-sidecar materialised file.
8. **Allowlist interaction**: the GitHub binding adds no allow rule; a workspace policy that blocks `api.github.com` makes the placeholder login fail, and the failure is shown as the existing `Failed` outcome on the Manage card.
9. **Migration of a sandbox holding the plaintext token**: `ApplyOutcome::Applied` gains a `swapped: bool` field (serde default `false`), and `plan()` in `src-tauri/src/services/github_access.rs` returns `Apply` when `swapped` differs from the sandbox's mode, so the next reconcile re-runs `APPLY_SCRIPT`, which deletes `hosts.yml` before logging in with the placeholder.
10. **Rotation exec**: a new credential stamp still triggers `APPLY_SCRIPT` in running sidecar-mode sandboxes even though the placeholder is unchanged, because gh's online validation through the sidecar doubles as the end-to-end check that the new token works and refreshes the card's outcome stamp.
11. **Opt-out and mise**: an opted-out sidecar-mode sandbox gets an empty mise env instead of the placeholder, because an unswapped placeholder makes GitHub answer `401` where no token gets the unauthenticated rate limit. Decision 15 restores this after a detour through an empty env for every sidecar-mode sandbox.
12. **Removal when a reload fails**: when the desired state drops the GitHub binding and the sidecar reload fails, `mio-github` stays deleted rather than being restored with the rest of the snapshot by `restore_secret_files()` (`src-tauri/src/proxy/iron.rs`); the running sidecar keeps the token in memory until its next successful reload or restart, and the failure is logged and surfaced as the reconcile's error.
13. **Lock order**: `RECONCILE_LOCK` (`src-tauri/src/services/github_access.rs`) is taken before the per-sandbox `apply_lock()` (`src-tauri/src/proxy/iron.rs`) and never the reverse; `apply_policy()` reads the opt-out state file without taking `RECONCILE_LOCK`.
14. **Parallel worktrees**: the feature adds no host-global state. The token file lives under `<app_data_root>/proxy/<sandbox>/secrets/` via `state_dir()` (`src-tauri/src/proxy/mod.rs`), which honours `MIO_APP_DATA_DIR`; no host port, external resource, bundled binary, teardown or harness environment variable is added.

15. **`raw.githubusercontent.com` joins the binding, so mise gets the placeholder** (verified from mise's source at `main`, 2026-11-19, and one anonymous `curl` check): mise attaches `GITHUB_TOKEN`/`MISE_GITHUB_TOKEN` to `api.github.com` and to `raw.githubusercontent.com` (`host_auth_headers()` in `crates/mise-util/src/http.rs`, `get_headers()` in `crates/mise-util/src/github.rs`); it never sends one to `github.com` or to the release-asset hosts (`resolve_token_inner()` returns no token for `objects`, `objects-origin` and `release-assets.githubusercontent.com`). mise's aqua backend, which serves most registry tools, reads its registry files and content from `raw.githubusercontent.com`. While that host was outside the binding, an unswapped placeholder there was rejected: `curl` with the placeholder as a Bearer token got `404` where an anonymous request got `200`, and Phase C briefly fell back to an empty mise env. The user decided to add `raw.githubusercontent.com` to `GITHUB_BINDING_HOSTS` (`src-tauri/src/proxy/iron_config.rs`), so the placeholder is swapped there and mise's rate-limit lift is restored. `AppToolchainSecrets::env()` (`src-tauri/src/services/toolchain_secrets.rs`) returns `GITHUB_PLACEHOLDER` for both variables in sidecar mode when `iron::github_binding()` yields a token (credential saved, sandbox not opted out), an empty env when opted out, and the real token otherwise. The decision sits in `services/` because `credentials/` cannot depend on `services/`; `credentials::github_env()` builds the variable list. No live sidecar run was made.

## Implemented: Phase A — The GitHub Binding in the Sidecar

Proxy-side only; no in-sandbox behaviour changes until Phase B.

- `src-tauri/src/proxy/iron_config.rs`:
  - New `AppBinding` struct (`file`, `placeholder`, `hosts`, `headers`) and constants `GITHUB_BINDING_FILE` (`mio-github`) and `GITHUB_BINDING_HOSTS`.
  - `render(policy, proxy_ip, with_value, app_bindings: &[AppBinding])` appends one `secrets` entry per app binding after the workspace bindings; the `secrets` transform is still omitted when both lists are empty.
  - `github_app_binding()` builds the GitHub `AppBinding` from `GITHUB_PLACEHOLDER` and `GITHUB_BINDING_HOSTS`.
  - Tests: extend `every_rendered_key_was_run_against_iron_proxy` with a render that includes the GitHub binding; add a test that the GitHub entry has exactly the five hosts, `match_headers: ["Authorization"]`, and `match_query`/`match_body` `false`.
- `src-tauri/src/credentials/github.rs`: new `pub const GITHUB_PLACEHOLDER`.
- `src-tauri/src/proxy/iron.rs`:
  - `github_binding(app, sandbox) -> Result<Option<String>, ProxyError>`: the token when a credential is saved and the sandbox is not opted out.
  - `apply_policy_locked()`: calls `github_binding()`, writes `mio-github` with exact bytes through `write_atomic()` or deletes it, passes the GitHub `AppBinding` to `render()` when rendered, and adds `mio-github` to the keep list of `remove_stale_secret_files()`.
  - `restore_secret_files()` is called with a flag that skips restoring `mio-github` when the new desired state has no GitHub binding (Resolved Decision 12).
  - New `refresh_github_binding(app, registry, sandbox)`: takes `apply_lock()`, reads the applied snapshot via `net_policy::read_applied()` (`src-tauri/src/net_policy.rs`), and returns early when the sandbox has no snapshot. A running sidecar goes through `apply_policy_locked()`. A stopped sidecar gets `config.yaml` and the secret files written without a start, through a new `write_offline()` that shares the materialise and render steps. The function skips the reload when the rendered `config.yaml` and `mio-github` bytes equal what is on disk.
  - Tests: `mio-github` holds exact bytes and survives `remove_stale_secret_files()`; a removal deletes `mio-github` and a failed apply does not restore it.

## Implemented: Phase B — Placeholder Login for gh

- `src-tauri/src/credentials/github.rs`:
  - `ApplyOutcome::Applied { captured_at_ms, swapped }`, with `#[serde(default)]` on `swapped` so existing state files load as plaintext-mode applies.
  - `apply()` gains a `swapped: bool` argument recorded in the outcome; the caller chooses the stdin value. `APPLY_SCRIPT` and `REMOVE_SCRIPT` are unchanged.
- `src-tauri/src/services/github_access.rs`:
  - `github_sidecar_mode(app, name) -> bool` over `proxy::sidecar_engine()`.
  - `plan(credential_captured_at_ms, state, sidecar_mode)` returns `Apply` when the applied `swapped` differs from `sidecar_mode` (Resolved Decision 9); new unit tests cover the migration case and the unchanged-mode case.
  - `reconcile_locked()`: in sidecar mode, `Apply` calls `iron::refresh_github_binding()` and then `github::apply()` with `GITHUB_PLACEHOLDER`; a refresh error becomes `ApplyOutcome::Failed` with the error as `reason`. In sidecar mode, `Remove` calls `refresh_github_binding()` before `github::remove()`, so the credential is revoked even when the exec fails.
  - `spawn_reconcile_all()`: lists every user sandbox, calls `refresh_github_binding()` for each sandbox in sidecar mode whatever its status (warn-log and continue on error), then reconciles the running sandboxes as today.
  - `GithubSandboxStatus` gains `sidecar_mode: bool`, read host-side with no exec.
  - Built as planned; `github_sidecar_mode()` is `pub`, and the `Remove` refresh logs a failure and continues to the exec rather than aborting.
- `src-tauri/src/commands/credentials.rs`: no signature change; `delete_credential_bundle`'s doc comment changes to say a stopped sidecar loses the token file at once.

## Implemented: Phase C � mise Env in Sidecar Mode

Built as Resolved Decision 15 records: the placeholder in sidecar mode.

- `src-tauri/src/runtime/types.rs`: `ToolchainSecrets::env(&self, sandbox: &str)`.
- `src-tauri/src/runtime/mod.rs`: `network_opts(&self, name: &str)`; `install`, `reproduce_remote`, `prepare_agent_web` and `exec_in_container` pass their sandbox name.
- `src-tauri/src/services/toolchain_secrets.rs`: `AppToolchainSecrets::env()` returns, when `github_sidecar_mode()` is true, `GITHUB_PLACEHOLDER` for both variables while `iron::github_binding()` yields a token and an empty list otherwise; outside sidecar mode it returns the real token from `credentials::github_secret_env()`, which is unchanged.
- `MockRuntime` (`src-tauri/src/runtime/mock.rs`) needs no change: the mock carries no secrets.

## Implemented: Phase D — Manage Card Mode

- `src/lib/workspace/GithubAccess.svelte`: the `Outcome` type mirrors the new `swapped` field and `GithubStatus` mirrors `sidecarMode`. The `applied` line reads "GitHub: <login> · token kept securely" in sidecar mode and "GitHub: <login> · token stored in sandbox" in plaintext mode. A `data-github-mode` attribute (`sidecar`/`plaintext`) is added for e2e. The card strings are Paraglide keys (`github_*` in `messages/*.json`), added after user approval.
- An `applied` outcome whose `swapped` differs from `sidecarMode` displays as `out-of-date`, matching the reconcile's migration decision.

## Lifecycle and Cross-Cutting Concerns

- **Create**: `apply_policy_and_inject_ca()` (`src-tauri/src/services/sandbox.rs`) already calls `apply_policy()`, which now renders the GitHub binding, so the binding is live before the create's trailing `spawn_reconcile()` runs `APPLY_SCRIPT`.
- **Policy-editor apply**: `apply_sandbox_policy` (`src-tauri/src/commands/net_policy.rs`) re-renders the GitHub binding from current state, so an editor save never drops it.
- **Destroy**: `remove_proxy()` (`src-tauri/src/proxy/mod.rs`) removes the sidecar's state directory, `mio-github` included; `github_access::forget()` removes the opt-out file as today.
- **Startup sweep**: `proxy::sweep()` (`src-tauri/src/proxy/mod.rs`) removing orphaned sidecars removes their state directories, `mio-github` included; no new sweep is needed.
- **Derived state**: sidecar mode is derived from `meta.json` on every call and never stored; `swapped` in the outcome records only what the last apply delivered.
- **Architecture documents** (updated by `feature-implement` when the feature ships): `docs/architecture/network.md` gains the app-owned binding concept and the invariant "The GitHub credential is never resolved from a workspace binding"; `docs/architecture/agents.md`, "The GitHub Token", gains the sidecar-mode versus plaintext-mode split.
- **Out of scope**: response-side swap, agent OAuth tokens, credential-store values for workspace bindings, microsandbox, and encryption at rest stay in `04_improvements.md`.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The Manage card shows `Failed: … HTTP 401` right after a GitHub login on a sidecar-mode sandbox | `APPLY_SCRIPT` ran `gh auth login` with `GITHUB_PLACEHOLDER` before the sidecar reloaded with the GitHub binding, so gh's validation request reached GitHub unswapped | Call `iron::refresh_github_binding()` before `github::apply()` in `reconcile_locked()` (`src-tauri/src/services/github_access.rs`) |
| A sandbox that was running before the upgrade still holds the real token in `/root/.config/gh/hosts.yml` | `plan()` saw an `Applied` outcome with the current stamp and returned `None`, ignoring the mode change | `plan()` must compare `ApplyOutcome::Applied.swapped` against `github_sidecar_mode()` (Resolved Decision 9) |
| A stopped sandbox's sidecar still swaps the old or deleted token after the sandbox starts | The credential-change refresh skipped stopped sidecars, so `mio-github` on disk was stale when the sidecar booted | `spawn_reconcile_all()` must call `refresh_github_binding()` for every sidecar-mode sandbox regardless of status, writing files offline for a stopped sidecar |
| A workspace binding's secret file deletes or overwrites the GitHub token file | A secret file name collided with the GitHub binding's file name | Keep `GITHUB_BINDING_FILE` as `mio-github`, which no `[A-Z0-9_]` workspace `envVar` can equal (`src-tauri/src/proxy/iron_config.rs`) |
| `remove_stale_secret_files()` deletes `mio-github` after every apply | The keep list held only workspace binding names | Add `GITHUB_BINDING_FILE` to the keep list in `apply_policy_locked()` (`src-tauri/src/proxy/iron.rs`) when the binding is rendered |
| A swapped GitHub request fails upstream although `mio-github` exists | An iron-proxy `file` source sends the file's exact bytes, so a trailing newline becomes part of the token | Write `mio-github` through `write_atomic()` with the trimmed token from `credentials::load_secret()`, never with a newline |
| `gh auth login` fails with a proxy denial instead of a `401` | The workspace `.mio/net-policy.json` allowlist does not allow `api.github.com`, and the GitHub binding adds no allow rule | Allow GitHub's hosts in the workspace policy (the starter policy in `STARTER_BASE_HOSTS`, `src-tauri/src/net_policy.rs`, already does) |
| mise reports `404` or `401` fetching a registry file in a sidecar-mode sandbox | mise sent `GITHUB_PLACEHOLDER` to `raw.githubusercontent.com` while the binding was not rendered (opted out or no credential) or lacked that host | Keep `raw.githubusercontent.com` in `GITHUB_BINDING_HOSTS` (`src-tauri/src/proxy/iron_config.rs`), and have `AppToolchainSecrets::env()` (`src-tauri/src/services/toolchain_secrets.rs`) return the placeholder only when `iron::github_binding()` yields a token (Resolved Decision 15) |
| `reproduce_remote` sends the real token to a deployed sandbox | `AppToolchainSecrets::env()` saw no local iron-proxy sidecar for the sandbox name and chose plaintext mode | Expected: a sandbox behind a sidecar is refused for deploy (`src-tauri/src/backends/mod.rs`), so a deployed sandbox is always in plaintext mode |
| After a credential delete, a running sidecar still swaps the token | The sidecar reload failed, and iron-proxy keeps the token loaded in memory until a successful reload or restart | Re-run the reconcile or restart the sidecar (`restart_proxy_container()` in `src-tauri/src/proxy/mod.rs`); `mio-github` itself is already deleted (Resolved Decision 12) |
