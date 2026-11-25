---
type: ideas
title: "Net Policy Review: The Sandbox Cannot Widen Its Own Boundary"
feature: net-policy-review
---
# Net Policy Review: The Sandbox Cannot Widen Its Own Boundary

This document scopes a fix for finding 1 of [the podman-egress-boundary review](../podman-egress-boundary/05_dev_review.md) and task T2 of [the improvement backlog](../../improvement-backlog.md): code inside a sandbox can write the workspace policy file `.mio/net-policy.json`, and several apply paths enforce that file on the iron-proxy sidecar without the user seeing the change. The policy model lives in `src-tauri/src/net_policy.rs`, the apply in `src-tauri/src/proxy/iron.rs`, the commands in `src-tauri/src/commands/net_policy.rs` and `src-tauri/src/commands/proxy.rs`, the create and deploy orchestration in `src-tauri/src/services/sandbox.rs` and `src-tauri/src/backends/mod.rs`, and the editor in `src/routes/projects/NetPolicyEditor.svelte`.

## Current State

### Two copies of the policy

- **Workspace file** `.mio/net-policy.json`, read by `net_policy::load()` and written by `net_policy::save()` (`src-tauri/src/net_policy.rs`). The workspace is bind-mounted into the sandbox at `/root/work`, so any process in the sandbox can rewrite the file. For a deployed sandbox, Mutagen (`--sync-mode=two-way-resolved`, `src-tauri/src/sync/mod.rs`) syncs the remote workspace's file back to the laptop.
- **Applied snapshot** `<app_data_root>/net-policy/<sandbox>/applied.json` (`AppliedPolicy`, `read_applied()`/`write_applied()` in `src-tauri/src/net_policy.rs`), never mounted into the sandbox. `finish_apply()` (`src-tauri/src/proxy/iron.rs`) rewrites the snapshot after every successful apply, and `prepare_egress()` (`src-tauri/src/backends/mod.rs`) writes it at create.
- **Secret values** live in one app-global map, `<app_data_root>/proxy/secrets.json` (`load_secrets()`, `set_secret()`, `delete_secret()` in `src-tauri/src/proxy/mod.rs`), keyed by a binding's `secretId`. The editor builds a new id as `<sandbox>-<envvar>` (`saveSecret()` in `src/routes/projects/NetPolicyEditor.svelte`); the backend enforces no ownership.

### Every path that puts a policy or binding into a sidecar

| Path | Seam | Policy source | Safe today |
|---|---|---|---|
| Editor Save, including Save enabled by drift alone | `save()` in `NetPolicyEditor.svelte` → `apply_sandbox_policy` (`src-tauri/src/commands/net_policy.rs`) | Re-reads the workspace file | No |
| Editor add, edit or remove secret | `commitSecretChange()` in `NetPolicyEditor.svelte` → `set_net_policy`, `set_proxy_secret`, `apply_sandbox_policy` | Writes the editor's copy, then `apply_sandbox_policy` re-reads the workspace file | No: the editor's copy came from the file at mount, and the re-read is a second window |
| Create | `prepare_egress()` (`src-tauri/src/backends/mod.rs`), then `apply_policy_and_inject_ca()` (`src-tauri/src/services/sandbox.rs`) | Workspace file, read twice | No for an imported folder or a pre-existing workspace directory: its bindings and rules apply unseen |
| Deploy | `deploy_to_target()` (`src-tauri/src/services/sandbox.rs`) | Workspace file at `deployment.alpha` | No: an unreviewed edit is enforced on the remote sidecar |
| Deploy picker secret count | `deploy_secret_count()` (`src-tauri/src/proxy/iron.rs`) | Workspace file | Read-only; the count describes the file, not what deploys |
| GitHub binding refresh (credential save or delete, login capture, opt-out switch, start, create) | `refresh_github_binding()` (`src-tauri/src/proxy/iron.rs`) from `reconcile_locked()` and `spawn_reconcile_all()` (`src-tauri/src/services/github_access.rs`) | Applied snapshot | Yes, if the snapshot is trusted; the refresh re-reads current secret values, so a snapshot binding whose value was missing becomes live when a value appears |
| Sidecar start (start, undeploy, deploy rollback, restart) | `ensure_sidecar_running()` (`src-tauri/src/proxy/iron.rs`), `restart_proxy_container()` (`src-tauri/src/proxy/mod.rs`) | On-disk config and staged secret files of the last apply | Yes, if the last apply was |
| Stopped sidecar refresh | `write_offline()` (`src-tauri/src/proxy/iron.rs`) | Applied snapshot | Yes, if the snapshot is trusted |
| Startup sweep | `proxy::sweep()` (`src-tauri/src/proxy/mod.rs`) | None: removes orphans | Yes |

The snapshot is only as trustworthy as the apply that wrote it: today `apply_sandbox_policy` writes whatever the workspace file held into `applied.json`, so the "safe" rows inherit an unreviewed policy after the first unsafe apply.

### Aggravating details

- **Open mode needs no allow rule.** `validate()` (`src-tauri/src/net_policy.rs`) checks a binding host against the rules only in allowlist mode, and open is the create dialog's default (`src/routes/projects/CreateSandboxItem.svelte`). In open mode an attacker only appends a host to a binding's `hosts`.
- **Cross-sandbox secret theft.** A binding may name any `secretId`, and `materialise_secret_files()` (`src-tauri/src/proxy/iron.rs`) resolves it from the app-global store. A sandbox that guesses another sandbox's name can bind `<other>-anthropic_api_key` to its own host and receive the other sandbox's value. A diff review alone does not reveal the risk, because the editor never shows `secretId`.
- **The editor shows the file, not what runs.** `refresh()` in `NetPolicyEditor.svelte` loads the workspace file through `get_net_policy`; the drift banner says "save to apply it", and Save is enabled by drift alone. The light spec `e2e/test/specs/light/podman-egress-boundary-ui.spec.ts` asserts exactly that drift-only Save applies an on-disk binding.
- **No file watch.** The editor reads the file and the drift only when the component mounts.

### How the workspace file is meant to be shared

The file is the "shared, provider-neutral" policy format (`docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 12): shared between providers, not between people. Nothing inside the sandbox reads it (no consumer under `src-tauri/resources/` or `paseo-plugins/`). No plan describes committing it for a team, and committed bindings would not be portable anyway, because each `secretId` embeds the sandbox name. The file does arrive from outside the app in two ways: an imported folder (`existingPath` in `CreateSandboxItem.svelte`), typically a git clone, and a leftover default workspace directory reused by a same-name create. Both reach `prepare_egress()` today.

## Proposed Model

### The accepted policy is the only apply input

`applied.json` becomes the **accepted policy**: the one policy the user has seen, and the only input any path renders into a sidecar. `apply_sandbox_policy` takes the policy object the editor shows instead of re-reading the workspace file. Create, deploy and the deploy secret count read the accepted policy. The GitHub refresh and sidecar start paths already do.

### The workspace file is a proposal

When the workspace file differs from the accepted policy, the editor shows a **proposal**: a structured diff produced by a new pure `net_policy::diff()` (`src-tauri/src/net_policy.rs`). The diff flags every change that widens reach: egress switched to open, an allow rule added, a deny rule removed, a binding added, a binding host added, a binding's `secretId` changed, a substitution position (`query`, `body`, a header) added. The user applies the proposal in a review dialog that sends back exactly the policy it displayed, or discards the proposal, which rewrites the workspace file from the accepted policy.

### The app mirrors the accepted policy into the workspace

After every successful apply, the app writes the accepted policy to `.mio/net-policy.json`, so the file keeps matching what runs and a user's hand edit in an IDE becomes a proposal. Mutagen's `two-way-resolved` mode lets the laptop's mirror win a conflict with a remote edit.

### Secret values belong to one sandbox

Each sandbox resolves binding values only from its own value store, so a binding can never reach another sandbox's secret, whatever `secretId` it names.

## Technology Choices

### Where authority lives: accepted snapshot, diff review, or both

- **Chosen: a hybrid.** The accepted snapshot is the sole apply input (backlog option a), and the diff review of backlog option b is the only way a workspace change enters the snapshot.
- **Option a alone (snapshot authority, file ignored)** was not chosen outright: without a review path, an imported folder's policy and a user's hand edit could never be adopted, and the file would silently diverge.
- **Option b alone (confirm a diff, keep the file authoritative)** was rejected: every background path (GitHub refresh, start, deploy) would need the same gate, and any apply that re-reads the file after the confirmation reopens a time-of-check gap. Keeping authority app-side closes the gap structurally.

### How the reviewed policy reaches the apply

- **Chosen: the frontend sends the policy object it displayed.** The webview is trusted, and the object is the one the user saw.
- **Rejected: a hash of the reviewed file plus a backend re-read.** A mismatch only turns a silent attack into a confusing failure, and the hash check adds a second code path.

### Diff computation

- **Chosen: a pure Rust `net_policy::diff()`** with unit tests, returned over IPC. Policies carry no secret values, so the diff is safe to send.
- **Rejected: a TypeScript diff in the editor.** The widening classification is a security rule and belongs beside `validate()`; `cargo test` covers it without a webview.

### Secret ownership

- **Chosen: a per-sandbox value store** under `<app_data_root>/proxy/<sandbox>/`, removed with the sidecar's state directory at destroy (`remove_proxy()`, `src-tauri/src/proxy/mod.rs`).
- **Alternative: validate that every `secretId` starts with `<sandbox>-`.** Cheaper, but existing ids that predate the naming scheme (for example the seeded ids in the e2e specs) would fail validation, and a prefix rule is easy to bypass by a sandbox whose name is another's prefix plus a hyphen.

Both options are app-data paths through `crate::paths::app_data_root`, which honours `MIO_APP_DATA_DIR`, so the feature adds no host-global state and its specs stay per-worktree.

## Validation

No spike is needed: every seam is in-process file I/O, and the light tier already applies a policy against the scripted Podman runner and writes `config.yaml` and the secret files under the per-run app-data root (`e2e/test/specs/light/podman-egress-boundary-ui.spec.ts`). The attack was reproduced by reading the code only; the same light spec's drift test demonstrates the unreviewed apply.

## Open Questions

1. **Authority model**: snapshot authority, diff confirmation, or the hybrid.
2. **Review experience**: what the user sees and must click to adopt a proposal.
3. **Editing while a proposal is pending**: whether editor changes and secret stores are allowed before the proposal is resolved.
4. **Workspace mirror**: whether the app keeps writing the accepted policy into the workspace file.
5. **Create from an existing policy file**: what an imported folder's or reused directory's policy does at create, and whether the starter policy needs review.
6. **Background applies while a proposal is pending**: what the GitHub refresh, start and secret-store applies do.
7. **Deployed sandboxes**: how a synced-back file and the deploy-time apply are treated.
8. **Secret value scoping**: whether the cross-sandbox `secretId` hole is fixed in this feature, and how.
9. **Migration**: how existing sandboxes, their snapshots and their stored values carry over.
10. **Concurrent editors**: whether an apply must name the accepted revision it started from.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A binding host appears in the sidecar's `config.yaml` that the user never added | An apply path rendered the workspace `.mio/net-policy.json`, which code inside the sandbox can write | Render only the accepted policy (`net_policy::read_applied()`, `src-tauri/src/net_policy.rs`) or a policy object the user just reviewed |
| A sandbox's requests carry another sandbox's secret value | A workspace binding named another sandbox's `secretId`, and `materialise_secret_files()` (`src-tauri/src/proxy/iron.rs`) resolved it from the app-global `proxy/secrets.json` | Resolve values from the sandbox's own value store only |
| A widened binding passes validation without any new allow rule | `validate()` (`src-tauri/src/net_policy.rs`) checks binding hosts against rules only in allowlist mode | Flag every added binding host in `net_policy::diff()` regardless of egress mode |
| The GitHub refresh turns a dormant binding live after a secret is stored | `refresh_github_binding()` (`src-tauri/src/proxy/iron.rs`) re-materialises current values for every binding in the snapshot | Keep the snapshot trusted: no unreviewed binding may enter `applied.json` |
