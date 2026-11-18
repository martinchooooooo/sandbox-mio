---
type: spike
title: "Spike: GitHub Token as an iron-proxy Placeholder Swap"
feature: github-cli-auth
---
# Spike: GitHub Token as an iron-proxy Placeholder Swap

Run on 2026-11-18 against `podman-machine-default` with `docker.io/ironsh/iron-proxy:0.56.0` (the pin in `src-tauri/src/proxy/iron.rs`) and `localhost/sandbox-mio/base:10` (gh 2.105.0 baked in). The question: can a sandbox hold only a fixed placeholder while the iron-proxy sidecar swaps in the real GitHub token, for both gh and git over HTTPS? Scripts live in `proxy-swap/` beside this file and copy the topology of `docs/features/podman-egress-boundary/spike/` (internal network, dual-homed sidecar, CA injected into the sandbox). Every resource was named `spike-ghs-*` and removed by `proxy-swap/teardown.sh`. The real token was copied from the host credential store into a scratchpad state directory for the run, never printed, and deleted afterwards. Nothing was written to GitHub.

## Setup

- **Placeholder:** `ghp_MioPlaceholder0000000000000000000000`, 40 characters, shaped like a classic token. gh accepted the placeholder without a format complaint.
- **Binding:** one `secrets` `replace` entry with a `file` source, `rules` limited to `github.com`, `api.github.com` and `uploads.github.com`, and `match_headers: ["Authorization"]` (`proxy-swap/render_config.sh`).
- **CA:** generated with `iron-proxy generate-ca -alg ed25519` in a one-shot container on the pinned image.
- **Echo binding:** a second entry on `httpbin.org` with a fake value, so swap mechanics are visible without the real token.

## Phase 1: Swap Mechanics (httpbin.org, fake value)

| # | Sandbox sends | Result |
|---|---|---|
| E1 | `Authorization: token <placeholder>` | Swapped |
| E2 | Basic `x-access-token:<placeholder>` (git's form) | Decoded, swapped, re-encoded (`replaceInHeader()` in iron-proxy `internal/transform/secrets/secrets.go`) |
| E3 | Basic header to `httpbin.org/basic-auth/…` expecting the real value | 200 |
| E6 | `Authorization: Bearer <placeholder>` | Swapped |
| E4 | `X-Other: <placeholder>` | Not swapped: outside `match_headers` |
| E5 | `?t=<placeholder>` | Not swapped: `match_query` is off |

E1 also shows that an upstream which echoes request headers returns the real value to the sandbox, so the binding's host rules are the only guard against an echoing host.

## Phase 2: Real GitHub (`proxy-swap/run_github.sh`)

| # | Check | Result |
|---|---|---|
| G1 | `gh auth login --with-token` fed the placeholder, then `gh auth setup-git` | Both exit 0: gh's online validation passes because the sidecar swaps the token on the validation request |
| G2 | Sandbox storage | `hosts.yml` holds the placeholder; zero lines contain the real token; `gh auth token` prints the placeholder |
| G3 | `gh auth status`, `gh api user` | Logged in; login returned |
| G4 | `git ls-remote` on a private repository | HEAD returned |
| G5 | `git push --dry-run` to a new branch | Authenticated to receive-pack, exit 0; the branch does not exist afterwards |
| G6 | Leak scan | Zero sidecar log lines and zero sandbox git config lines contain the real token; audit rows on `github.com` and `api.github.com` annotate `swapped` with `header:Authorization` |
| G7 | Binding removed and sidecar reloaded | `gh api user`: "Bad credentials"; git: "Invalid username or token" — the sandbox holds no usable credential |
| G8 | Binding restored | `gh api user` works again without touching the sandbox |

## Response-Side Swap

Not tested and not supported: `TransformResponse` is a no-op in the `secrets` and `oauth` transforms at v0.56.0. Rewriting responses would need an external transform over iron-proxy's gRPC `TransformService` (`proto/transform/v1/transform.proto`), which can replace a response only by returning `REJECT` with a modified response and buffers the whole body. GitHub's API and git endpoints do not return the token, so the GitHub case needs no response swap.

## Consequences for the Design

- The two blockers recorded in `docs/features/github-cli-auth/04_improvements.md` ("Keep the token out of proxy sandboxes with secret swapping") are cleared for iron-proxy sandboxes: Basic auth is handled, and online validation succeeds through the swap.
- The apply script in `src-tauri/src/credentials/github.rs` can stay, fed the placeholder instead of the token, provided the binding is live in the sidecar before the apply runs.
- A token rotation or removal becomes a sidecar reload (`apply_policy()` in `src-tauri/src/proxy/iron.rs`), with no exec into the sandbox.
- microsandbox's `--secret` swap was not tested; Basic-auth handling there is unknown.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `gh api user` returns "Bad credentials" in a sandbox whose `hosts.yml` holds the placeholder | The sidecar config has no GitHub `secrets` entry, or the entry's `file` source is missing, so the placeholder reaches GitHub unswapped | Render the GitHub binding and reload the sidecar (`proxy-swap/render_config.sh github`, then the management `reload`); check the audit row for `secret_unavailable` |
| The real token reaches the sandbox in a response body | An upstream inside the binding's host rules echoes request headers back, as `httpbin.org/headers` does | Keep the GitHub binding's `rules` limited to GitHub's own hosts and `match_headers` limited to `Authorization` |
| The swapped request fails upstream although the value file exists | The `file` source sends the file's exact bytes, so a trailing newline in the token file becomes part of the token | Write the token file without a trailing newline (`tr -d '\r\n'`) |