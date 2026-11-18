---
type: ideas
title: "GitHub CLI Login, Shared With Every Sandbox"
feature: github-cli-auth
---
# GitHub CLI Login, Shared With Every Sandbox

This document scopes a GitHub login the user performs once in the app, through `gh auth login` in the hidden auth container, whose token the app then applies to every sandbox's `gh` and `git`, both at create and to existing sandboxes, unless the user turns it off for a sandbox. The flow mirrors the Claude Code login (`docs/features/shared-agent-auth/02_dev.md`, `docs/features/credentials-inline-auth/02_dev.md`). Nothing of this feature exists yet. The credential store already holds one GitHub token, pasted by hand and used only for in-sandbox `mise` calls (`docs/features/techdebt-sweep/02_dev.md`, Phase H); this feature merges the new login into that one credential. The base image has no `gh` today. Relevant code: the credential store (`src-tauri/src/credentials/mod.rs`, `src-tauri/src/commands/credentials.rs`), the auth runner (`start_auth()` and `cancel_auth()` in `src-tauri/src/agents/mod.rs`), the login card (`src/lib/agents/authSession.svelte.ts`, `src/lib/agents/AuthSessionCard.svelte`, `src/routes/credentials/+page.svelte`), sandbox create and start (`create()` in `src-tauri/src/services/sandbox.rs`, `start_sandbox` in `src-tauri/src/commands/sandbox.rs`) and the base image (`src-tauri/resources/Containerfile`, `BASE_IMAGE_VERSION` in `src-tauri/src/backends/mod.rs`).

## Current State

- **One GitHub credential exists, mise-only.** `set_github_token` (`src-tauri/src/commands/credentials.rs`) saves a pasted token as the single-value credential `github` (`GITHUB_SERVICE`, `save_secret()` in `src-tauri/src/credentials/mod.rs`): a `secret` file plus a `CredentialManifest` under `<store_dir>/github/`. `github_secret_env()` exports the token as `GITHUB_TOKEN` and `MISE_GITHUB_TOKEN` on in-sandbox `mise` calls only, through the secret exec environment (`ExecOpts.secret_env`, `src-tauri/src/provider/types.rs`), never in argv. The token never reaches a user shell, `gh` or `git` in a sandbox. The token card is the last block of `src/routes/credentials/+page.svelte`.
- **The Claude login is the model to follow.** The Credentials page creates `mio-auth-<service>` (`ensure_auth_container()`, pinned to Podman), runs the login command through `start_auth()`, streams output to `AuthSessionCard.svelte`, and on exit `handleExit()` (`src/lib/agents/authSession.svelte.ts`) captures the result and destroys the auth container. Both `handleExit()` and the page's service list key off the agent catalogue (`AGENTS` in `src/lib/agents/catalogue.ts`), so a non-agent login has no place in that flow today.
- **Claude injection is per sandbox and manual.** A saved Claude login reaches a sandbox only when Claude Code is installed (`install_agent`, `src-tauri/src/commands/agents.rs`) or the user clicks "Use saved login" (`useSavedLogin()` in `src/routes/projects/AgentManager.svelte`). Nothing applies a credential at create or to all sandboxes. The open question "Automatic injection at sandbox creation" in `docs/features/shared-agent-auth/01_ideas.md` was never resolved.
- **Exec plumbing is ready.** `MachineProvider::exec_piped` (`src-tauri/src/provider/types.rs`) feeds bytes on stdin to a command in a Podman container or microsandbox VM; `tar_inject()` in `src-tauri/src/credentials/mod.rs` uses it. It starts a stopped sandbox first, so callers must not use it to reach every stopped sandbox.
- **Start has a best-effort follow-up hook.** `start_sandbox` (`src-tauri/src/commands/sandbox.rs`) spawns `services::inference_reach::spawn_ensure_for_sandbox` and `services::panes::spawn_prewarm_agent_web` after `store.start`, which is the pattern a "reapply on start" step can copy.
- **Per-sandbox host state lives in app data.** `crate::paths::sandbox_key_dir` and `sandbox_pane_dir` (`src-tauri/src/paths.rs`) hold per-sandbox state outside the workspace, and `cleanup_host_state()` (`src-tauri/src/services/sandbox.rs`) removes it on destroy.

## Proposed Model

### One GitHub credential, two ways to set it

The `github` single-value credential stays the only GitHub secret. The Credentials page's GitHub card gains a "Log in with GitHub" button beside the existing paste field. Both paths end in `save_secret(GITHUB_SERVICE, …)`, so `mise` calls keep working from either source and nothing else stores a second token. A pasted token is applied to sandboxes exactly like a login token. `CredentialManifest` (`src-tauri/src/credentials/types.rs`) gains the GitHub login name (an account label, since `account_email` is the wrong shape), so the card can show "Logged in as `<login>`". No migration is needed for a token saved before this feature: nobody uses the paste card yet (user decision, 2026-11-18).

### Login: `gh auth login` in the auth container

- The login reuses `ensure_auth_container()` (name `mio-auth-github`) and `start_auth()` with the command `gh auth login --hostname github.com --git-protocol https --web`. gh comes from the base image, so no install step runs, unlike Claude's `install_agent` call in `logIn()` (`src/routes/credentials/+page.svelte`).
- Without a TTY, gh goes straight to the device flow and prints a one-time code and `https://github.com/login/device` (spike, Risk 1). `start()` in `src/lib/agents/authSession.svelte.ts` already detects the URL; the store also parses the code (`one-time code: XXXX-XXXX`) and `AuthSessionCard.svelte` shows it with a copy button. The device flow needs no paste-back, so the card never enters `awaiting-code`.
- On exit 0, a new host command (working name `capture_github_login`, in `src-tauri/src/commands/credentials.rs`) execs `gh auth token` and `gh api user --jq .login` in the auth container, keeps stdout host-side, and calls `save_secret`. Only the manifest crosses IPC. `handleExit()` branches on the GitHub service instead of looking the service up in `AGENTS`.
- Cancel destroys the auth container. Closing stdin, which is all `cancel_auth()` does, leaves gh polling until the device code expires (spike, Risk 2).

### gh in the base image

The Containerfile installs a pinned, checksum-verified gh release binary to `/usr/local/bin/gh`, the same way it installs Herdr, and `BASE_IMAGE_VERSION` (`src-tauri/src/backends/mod.rs`) is bumped. The fixed path matters: `gh auth setup-git` writes the absolute path of gh into `/root/.gitconfig` (spike, Risk 4), so a version-specific mise install path would break git after a gh upgrade.

### Applying the token to a sandbox

A new module (working name `src-tauri/src/credentials/github.rs`) owns apply and remove for one sandbox:

- **Apply:** `exec_piped` runs `gh auth login --with-token --hostname github.com --git-protocol https` with the token on stdin, then `gh auth setup-git`. gh writes `/root/.config/gh/hosts.yml` itself, and git over HTTPS then authenticates through gh's credential helper (spike, Risk 4). gh validates the token against `api.github.com` before saving it (spike, Risk 5).
- **Remove:** `gh auth logout --hostname github.com`, which needs no confirmation without a TTY (spike, Risk 5). The git helper section stays and fails closed.
- **Older images:** a sandbox whose image has no `/usr/local/bin/gh` gets the git-only fallback (open question "Fallback for sandboxes without gh"), and the Manage tab says gh needs a recreate.
- The token never appears in an argv, a log line, or the workspace, per the invariants in `docs/architecture/agents.md` and `docs/architecture/overview.md`.

### When the token is applied

| Event | Action | Seam |
|---|---|---|
| Sandbox created | Apply, best-effort, after `runtime.init` | `create()` in `src-tauri/src/services/sandbox.rs` |
| Login or paste saved | Apply to every running, opted-in sandbox | the save path in `src-tauri/src/commands/credentials.rs` |
| Sandbox started | Apply if the sandbox's applied stamp is older than the stored credential; remove if the credential is gone | spawned follow-up in `start_sandbox` (`src-tauri/src/commands/sandbox.rs`) |
| Credential deleted | Remove from every running sandbox | `delete_credential_bundle` |
| Opt-out toggled | Remove now, or apply now | new per-sandbox command |

Stopped sandboxes are never started just to apply or remove the token. Every apply failure is warn-logged and never fails create or start, matching `github_secret_env()`'s rule that a missing token never fails an install. Apply runs only on local sandboxes (Podman and microsandbox). A deployed sandbox (`docs/architecture/remote.md`) is skipped: nobody uses remote deploy yet (user decision, 2026-11-18), and `04_improvements.md` records the extension.

### GitHub hosts in the starter allowlist

When the create dialog seeds an allowlist policy, the GitHub hosts gh and git need join the starter hosts, beside `allAgentApiHosts()` (`src/lib/agents/catalogue.ts`), which `CreateSandboxItem.svelte` (`src/routes/projects/`) passes as `starterHosts` (user decision, 2026-11-18). Like the agents' hosts, they are seeded whether or not a GitHub credential is saved. The minimum set is `github.com` and `api.github.com`, which `gh auth login --with-token` and git over HTTPS need; `02_dev.md` settles the full list (for example `codeload.github.com` and `objects.githubusercontent.com` for archives and release assets). A sandbox in open mode needs nothing, and an existing allowlist is not rewritten.

### Per-sandbox state and the opt-out

A small JSON file per sandbox in app data (working path `<app_data_root>/github/<sandbox>.json`, a sibling of `sandbox_pane_dir`) records `opted_out` and the `captured_at_ms` of the credential last applied. `cleanup_host_state()` deletes the file on destroy. The default is opted in. A switch on the sandbox's Manage tab, near `AgentManager.svelte` in `src/lib/workspace/DashboardOverview.svelte`, shows "GitHub: `<login>`", "Off for this sandbox" or "Not applied (needs recreate)", and toggles the opt-out.

## Technology Choices with Tradeoffs

### Token acquisition: gh device flow in the auth container (chosen)

- **Rust implements the OAuth device flow against a Sandbox-mio GitHub OAuth app.** Rejected for v1: the maintainer would have to register and own an OAuth app and ship its client id, and the app would add an HTTP client flow for one login. In its favour, it needs no container spin-up and the token would appear under the app's own name in the user's GitHub settings. Recorded in `04_improvements.md`.
- **Rust runs the device flow with gh's public OAuth client id.** Rejected: it presents the app as the GitHub CLI to GitHub.
- **Import the token from a host gh (`gh auth token` on Windows).** Rejected as the primary path: gh is rarely installed on the Windows hosts the app targets, and the app's premise is that tooling lives in sandboxes (`docs/features/shared-agent-auth/01_ideas.md`, "Capture context"). Kept as an improvement.
- **Paste a token only (today's card).** Kept as the second path, not the only one: the user asked for a login like Claude's, and creating a personal access token by hand is several pages of GitHub settings.

### What is stored: the token as a single value (chosen) vs a tarred `~/.config/gh/` bundle

- **Capture `/root/.config/gh/` with `tar_capture()` and inject it with `tar_inject()`, exactly like Claude.** Rejected: the pasted-token path has no directory to capture, so the two sources would need two storage shapes, and `mise` needs the bare token anyway. gh tokens do not rotate (spike, Risk 3), so the main reason Claude stores a directory, carrying onboarding state with a rotating credential, does not apply.

### Delivery into the sandbox: `gh auth login --with-token` + `gh auth setup-git` (chosen)

- **`GH_TOKEN`/`GITHUB_TOKEN` in the container environment at create.** Rejected: an existing sandbox would need a recreate, which Podman sandboxes do not support (`docs/architecture/providers.md`, Comparison), and a re-login or removal would also need one. It also puts the value in the container config, and `git` ignores `GH_TOKEN` without a helper.
- **`GH_TOKEN` exported from a file sourced by `/root/.bashrc`.** Rejected: only interactive bash sees it. Exec-launched processes such as the Paseo daemon and the native pi chat's supervisor would not, and the file would be a second plaintext copy beside gh's own store.
- **Write `hosts.yml` directly from the host.** Rejected: it couples the app to gh's config schema, which changed to a multi-account layout (spike, Risk 3, shows both layouts in one file). Its one advantage is skipping online validation, which would spare an allowlisted sandbox the `api.github.com` call (Common Gotchas); that does not outweigh the schema coupling.
- **Plain `git credential-store` only.** Rejected as the main path because `gh` itself would stay logged out; kept as the fallback for images without gh.

### gh binary: baked into the base image (chosen) vs installed by mise on apply

- **mise installs gh when the token is applied.** Rejected: either the gh entry lands in the project's `mise.toml`, which travels with the project and is not the user's choice, or in a global mise config that remote deploy does not reproduce. mise's install path also changes with the version, which breaks the absolute helper path `gh auth setup-git` writes. The cost of baking is a new base-image version and a binary of about 41 MB (spike, Risk 1, measured size). Sandboxes from older images get the git-only fallback.

### Apply policy: automatic with a per-sandbox opt-out (chosen)

User decision, 2026-11-18.

- **Every sandbox, no opt-out.** Rejected: a sandbox that runs untrusted code then always holds a token that can push to every repository the user can reach.
- **Per-sandbox opt-in, like Claude's "Use saved login".** Rejected: the user asked for login once and apply everywhere, including at create.

### Where the opt-out lives: app data (chosen) vs the workspace's `.mio/`

- **A flag in `.mio/`.** Rejected: the workspace is writable from inside the sandbox, so code in an opted-out sandbox could flip the flag and have the app push the token in. `.mio/` would also carry the choice to another machine, where the choice may not hold.
- **`settings.json` (`src-tauri/src/settings/mod.rs`).** Rejected: per-sandbox entries would outlive destroyed sandboxes unless `cleanup_host_state()` also edits settings. A per-sandbox file is deleted with the sandbox's other host state.

### Host-global state

The feature adds no host-global dependency: the store and per-sandbox files derive from the app-data root that `MIO_APP_DATA_DIR` and `MIO_CREDENTIALS_DIR` relocate per worktree. The light tier can script `gh` exec calls through the mock runner. Only a real device-flow login needs a human and GitHub, so no automated tier can run it end to end.

## Validation

A spike on 2026-11-18 ran gh 2.105.0 in two throwaway `sandbox-mio/base:9` containers on `podman-machine-default`, with a real device-flow login completed by the user. Record: [gh-device-flow.md](spike/gh-device-flow.md); commands: `docs/features/github-cli-auth/spike/probe.sh`. Results:

- `gh auth login` with no TTY prints the code and URL and completes after the browser step, so it fits `start_auth()` unchanged.
- Closing stdin does not end the login, so cancel needs a different mechanism (blocker for the current cancel path).
- The token is a non-expiring `gho_` OAuth token with `repo`, `read:org` and `gist` scopes. gh stores it in plain text in `hosts.yml`.
- Piping the token into `gh auth login --with-token` in a second container, then `gh auth setup-git`, authenticated both `gh api` and `git credential fill`.
- `--with-token` validates online (`HTTP 401: Bad credentials` for a bogus token). `gh auth logout` works without a TTY.

Not covered: the scope check `--with-token` applies to pasted classic or fine-grained personal access tokens, microsandbox, and deployed sandboxes.

## Open Questions

- ~~**Existing pasted token on upgrade.**~~ RESOLVED 2026-11-18 (user): no backwards compatibility. Nobody uses the paste card yet, so a pasted token is applied like a login token and the manifest needs no `source` field.
- **Pasted-token scope check.** `gh auth login --with-token` may reject a classic token that lacks `repo` or `read:org`, which a mise-only token usually lacks. Should save validate the token's suitability for sandboxes (and how, given the host has no gh), or should apply fail per sandbox and show the reason on the Manage tab?
- **Fallback for sandboxes without gh.** For images older than the gh bump: configure `git credential-store` with the token in `/root/.git-credentials`, skip the sandbox and show "needs recreate", or install gh into the sandbox through mise once? The fallback must also be removable when the credential is deleted.
- ~~**Deployed sandboxes.**~~ RESOLVED 2026-11-18 (user): out of scope. Nobody uses remote deploy yet, so deployed sandboxes are skipped (Proposed Model, "When the token is applied").
- ~~**Proxy sandboxes with an allowlist.**~~ RESOLVED 2026-11-18 (user): when a sandbox has an allowlist, the GitHub hosts join its starter hosts (Proposed Model, "GitHub hosts in the starter allowlist"). The exact host list is left to `02_dev.md`.
- **Re-login while sandboxes are stopped.** A stopped sandbox keeps the old token until its next start through `start_sandbox`. A sandbox started by another path (for example `provider.start` inside an exec helper) skips the start hook. Is a stale old token acceptable there, given gh tokens do not expire but may be revoked?

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The GitHub login card stays on "Signing in…" for about 15 minutes after the user clicks Cancel | `cancel_auth()` (`src-tauri/src/agents/mod.rs`) only drops the session's stdin handle, and `gh auth login` keeps polling GitHub with stdin closed (`docs/features/github-cli-auth/spike/gh-device-flow.md`, Risk 2) | Cancel a GitHub login by destroying `mio-auth-github` (`destroy_sandbox` with `force: true`), which ends the exec and emits the `Exit` event |
| The card shows the device URL but the user cannot find the code to enter | gh prints the one-time code only as an output line (`! First copy your one-time code: XXXX-XXXX`), which `AuthSessionCard.svelte` shows only under "Show details" | Parse the code in `start()` (`src/lib/agents/authSession.svelte.ts`) and render it prominently with a copy button |
| `git push` in a sandbox fails with "could not read Username" after gh was upgraded | `gh auth setup-git` writes gh's absolute path into `/root/.gitconfig`, and gh was installed somewhere other than the fixed `/usr/local/bin/gh` | Install gh only through the Containerfile at `/usr/local/bin/gh`; never through mise |
| Applying the token to a proxy-enabled sandbox fails with an `api.github.com` connection error | `gh auth login --with-token` validates the token online, and the sandbox's egress allowlist (`.mio/net-policy.json`) was seeded before the GitHub hosts joined the starter hosts, or the user removed them | Allow `api.github.com` and `github.com` in the sandbox's network policy, then re-apply from the Manage tab |
| A sandbox still pushes to GitHub after the user removed the GitHub credential | The sandbox was stopped when the credential was deleted, and removal runs only on running sandboxes and on the next start through `start_sandbox` | Start the sandbox through the app so the start hook runs `gh auth logout`; revoke the token at github.com under Settings → Applications if a copy may have leaked |
| Revoking the login in the app leaves the token valid on GitHub | `gh auth logout` and deleting the `github` credential only delete local copies; an OAuth token stays valid until revoked on GitHub | Revoke the GitHub CLI authorisation at github.com under Settings → Applications → Authorized OAuth Apps |
| A `.mio/` edit inside an opted-out sandbox re-enables GitHub access | Only true if the opt-out flag were stored in the workspace, which code inside the sandbox can write | Keep the opt-out in `<app_data_root>/github/<sandbox>.json`, never under `.mio/` |
