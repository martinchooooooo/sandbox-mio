---
type: ideas
title: "Proxy Credential Secrets: The GitHub Token as a Sidecar Placeholder Swap"
feature: proxy-credential-secrets
---
# Proxy Credential Secrets: The GitHub Token as a Sidecar Placeholder Swap

This document covers keeping the GitHub token out of sandboxes. Each sandbox holds a fixed, GitHub-shaped placeholder; the sandbox's iron-proxy sidecar swaps the placeholder for the real token, read from the host credential store, only in the `Authorization` header of requests to GitHub's hosts. Today `github-cli-auth` writes the real token in plaintext into each sandbox's `/root/.config/gh/hosts.yml` (`APPLY_SCRIPT` in `src-tauri/src/credentials/github.rs`). The user plans to give every sandbox a sidecar proxy, with iron-proxy as the main engine, so the sidecar becomes the place where credentials are attached. A spike on 2026-11-18 proved the mechanism end to end with gh and git (`docs/features/github-cli-auth/spike/proxy-swap.md`). Relevant code: `src-tauri/src/proxy/` (sidecar, config renderer, secret files), `src-tauri/src/net_policy.rs` (bindings and placeholders), `src-tauri/src/credentials/` (the host store and the gh apply script) and `src-tauri/src/services/github_access.rs` (the per-sandbox reconcile).

## Current State

- **Secret swapping exists for Podman sandboxes behind iron-proxy** (`docs/architecture/network.md`). A `SecretBinding` (`src-tauri/src/net_policy.rs`) in the workspace's `.mio/net-policy.json` names a `secretId`, an `envVar`, `hosts`, `headers`, `query` and `body`. Values live in `<app_data_root>/proxy/secrets.json` (`load_secrets()` in `src-tauri/src/proxy/mod.rs`, written by `set_proxy_secret` in `src-tauri/src/commands/proxy.rs`).
- **Rendering:** `apply_policy()` in `src-tauri/src/proxy/iron.rs` writes one value file per binding through `materialise_secret_files()`, renders a `secrets` `replace` entry per binding through `render()` in `src-tauri/src/proxy/iron_config.rs`, and reloads the sidecar. The placeholder is `mio-placeholder-<env_var>` (`placeholder()` in `src-tauri/src/net_policy.rs`) and reaches the sandbox as an environment variable at create.
- **The GitHub credential** is a single value at `<store_dir>/github/secret` (`src-tauri/src/credentials/mod.rs`). `reconcile_sandbox()` (`src-tauri/src/services/github_access.rs`) pipes it on stdin into `gh auth login --with-token` plus `gh auth setup-git` inside every running, opted-in sandbox, and `REMOVE_SCRIPT` deletes `hosts.yml` on removal or opt-out. `github_secret_env()` (`src-tauri/src/credentials/mod.rs`) also passes the real token to in-sandbox mise calls as a secret exec environment.
- **iron-proxy 0.56.0** (`IRON_PROXY_IMAGE_DEFAULT` in `src-tauri/src/proxy/iron.rs`) swaps in request headers, query, path and body. For an `Authorization: Basic` header the proxy decodes the payload, swaps, and re-encodes. Response transforms are no-ops in the `secrets` and `oauth` transforms.
- **Not every sandbox has a sidecar yet.** microsandbox swaps through msb's own `--secret`, with a mitmproxy sidecar for the traffic record only.

## Proposed Model

### An app-owned GitHub binding

The app renders the GitHub binding itself, beside the workspace bindings, in `render()` (`src-tauri/src/proxy/iron_config.rs`). The binding never lives in `.mio/net-policy.json`: code inside the sandbox can write the workspace, and a sandbox-written binding could point the GitHub credential at a host the attacker controls. The binding is fixed:

- **Source:** a `file` source holding the GitHub token, materialised beside the workspace bindings' files under the sidecar's `secrets/` directory.
- **Hosts:** `github.com`, `api.github.com` and `uploads.github.com`.
- **Match:** `match_headers: ["Authorization"]`, with query, path and body off.
- **Placeholder:** one constant for every sandbox (next section).

The binding is rendered only when a GitHub credential exists and the sandbox is opted in (the opt-out already lives in `<app_data_root>/github/<name>.json`).

### A fixed, GitHub-shaped placeholder

A constant such as `ghp_MioPlaceholder0000000000000000000000` (`ghp_` plus 36 characters), shared by every sandbox. The placeholder carries no secret, so being known is harmless: only the sidecar can turn it into the token, and only for the GitHub hosts. One constant means a re-login never touches the sandbox.

### Delivery into the sandbox

`reconcile_sandbox()` keeps `APPLY_SCRIPT` unchanged and feeds it the placeholder instead of the token for a sandbox behind a sidecar. `gh auth login --with-token` validates the placeholder online, and the sidecar swaps that validation request, so login succeeds and `hosts.yml` stores only the placeholder. `gh auth setup-git` makes git send the placeholder in Basic auth, which the sidecar also swaps. `github_secret_env()` passes the placeholder instead of the token to mise in the same sandboxes.

### Ordering and lifecycle

- **Apply:** the sidecar must have the GitHub binding loaded before the placeholder login runs, or gh's validation gets a `401`. The reconcile therefore re-applies the sidecar policy first, then runs `APPLY_SCRIPT`.
- **Re-login or rotation:** rewrite the token file and reload every sidecar. No exec into any sandbox is needed, and stopped sandboxes are covered because a sidecar re-reads its files at every start.
- **Removal or opt-out:** drop the binding, delete the token file and reload. Requests carrying the placeholder then fail with "Bad credentials". `REMOVE_SCRIPT` still runs so `gh auth status` reports logged out instead of failing.
- **Migration:** a sandbox already holding the plaintext token gets the placeholder on its next reconcile, because `APPLY_SCRIPT` clears `hosts.yml` first.

### Sandboxes without a sidecar

Until every sandbox has a sidecar, a sandbox without one keeps today's plaintext apply. The Manage card (`src/lib/workspace/GithubAccess.svelte`) shows which mode a sandbox is in, so the user can see where a plaintext copy exists.

## Technology Choices

| Decision | Chosen | Alternatives and why rejected |
|---|---|---|
| How the sidecar attaches the token | `replace` (placeholder swap) | `inject` mode, which sets the header on every matching request with nothing in the sandbox: gh refuses to run without a local login, `gh auth status` fails, and a forced header overrides any credential the user supplies. |
| Swap direction | Requests only | A response swap through iron-proxy's gRPC `TransformService` (`proto/transform/v1/transform.proto`): its protocol can replace a response only by returning `REJECT` with a modified response, it buffers whole bodies (large git fetches), and GitHub's API and git endpoints never return the token. |
| Where the binding lives | App-rendered, never in the workspace | A binding in `.mio/net-policy.json`: the workspace is writable from inside the sandbox, so the sandbox could rebind the credential to its own host. |
| Placeholder shape | GitHub-shaped constant | `placeholder()`'s `mio-placeholder-<env_var>` form: gh's acceptance of a non-token-shaped value is unverified (Open Question "Placeholder shape"). A per-sandbox random placeholder: adds state and a sandbox exec on every rotation for no gain, since a placeholder is useless outside the sidecar. |
| Where the value is read | The credential store, at every materialise | A copy in `<app_data_root>/proxy/secrets.json`: two plaintext stores to keep in sync on login, rotation and delete. |
| Engine | iron-proxy (Podman) first | msb's `--secret` swap: Basic-auth handling unknown, and the user's direction is iron-proxy. |

No new host-global state: the binding renders into existing per-sandbox sidecar state under `app_data_root()`, which honours `MIO_APP_DATA_DIR`.

## Validation

The spike on 2026-11-18 (`docs/features/github-cli-auth/spike/proxy-swap.md`, scripts in `docs/features/github-cli-auth/spike/proxy-swap/`) ran iron-proxy 0.56.0 and `localhost/sandbox-mio/base:10` with a real GitHub token:

- **Swap mechanics:** the sidecar swapped the placeholder in `token`, `Bearer` and Basic `Authorization` headers, and left other headers and the query string alone.
- **gh with only the placeholder:** `gh auth login --with-token` and `gh auth setup-git` exited 0, `gh api user` worked, and `hosts.yml` held only the placeholder.
- **git:** `git ls-remote` on a private repository and `git push --dry-run` both authenticated.
- **No leaks:** zero sidecar log lines and zero sandbox git-config lines contained the real token, and the audit annotated each swap.
- **Revocation:** with the binding removed, gh returned "Bad credentials" and git "Invalid username or token". Restoring the binding restored access without touching the sandbox.

## Open Questions

- **Placeholder shape.** Does gh accept `mio-placeholder-gh_token`, which would let the GitHub binding reuse `placeholder()`, or must the placeholder look like a GitHub token? Only the GitHub-shaped form was tested.
- **`require` on the GitHub binding.** Should the sidecar reject requests to the GitHub hosts that carry no placeholder? Rejecting blocks a credential the user brought themselves (a different `gh auth login`) and anonymous public clones. Allowing them keeps the user's own logins working.
- **Host list.** Do `gist.github.com` (gist clones) and `codeload.github.com` (private archive downloads) need the swap, or do their flows redirect through `api.github.com` with signed URLs?
- **microsandbox.** Does microsandbox stay on plaintext apply until every sandbox runs iron-proxy, or does the feature first test msb's `--secret` with Basic auth?
- **Traffic panel.** Should swapped GitHub requests show the secret as `github` in `TrafficPanel.svelte`, given the audit annotation names the token file's path?

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The Manage card shows a `Failed` outcome with a `401` right after a GitHub login on a sidecar sandbox | `APPLY_SCRIPT` ran `gh auth login` with the placeholder before the sidecar reloaded with the GitHub binding, so gh's validation request reached GitHub unswapped | Reload the sidecar policy (`apply_policy()` in `src-tauri/src/proxy/iron.rs`) before running `APPLY_SCRIPT` in `reconcile_sandbox()` |
| `gh api user` returns "Bad credentials" in a sandbox whose `hosts.yml` holds the placeholder | The sidecar config has no GitHub binding, or the token file under the sidecar's `secrets/` directory is missing (the audit row then annotates `secret_unavailable`) | Check the GitHub credential exists and the sandbox is opted in, then re-apply the sidecar policy |
| The real token reaches the sandbox in a response body | A host inside the GitHub binding's rules echoes request headers back, as `httpbin.org/headers` did in the spike | Keep the binding's hosts limited to GitHub's own hosts and `match_headers` limited to `Authorization` |
| A swapped request fails upstream although the token file exists | An iron-proxy `file` source sends the file's exact bytes, so a trailing newline in the token file becomes part of the token | Write token files without a trailing newline, as `materialise_secret_files()` does |
| A workspace binding with `secretId` naming the GitHub credential sends the token to an unexpected host | The binding came from `.mio/net-policy.json`, which code inside the sandbox can write | Never resolve the GitHub credential from a workspace binding; only the app-rendered binding reads the credential |
