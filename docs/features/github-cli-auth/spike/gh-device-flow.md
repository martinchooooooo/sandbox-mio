---
type: spike
title: "Spike: gh Device-Flow Login Without a TTY, and Token Injection"
feature: github-cli-auth
---
# Spike: gh Device-Flow Login Without a TTY, and Token Injection

Run on 2026-11-18 against `podman-machine-default` (`podman --connection podman-machine-default`), using two throwaway containers from `localhost/sandbox-mio/base:9`: `spike-gh-auth` (stands in for the `mio-auth-<service>` auth container) and `spike-gh-target` (stands in for a user sandbox). gh was installed from the `cli/cli` release tarball, version 2.105.0 (released 2026-11-17). The commands are in `probe.sh` beside this file. Both containers were removed afterwards. Token values were redacted from all captured output.

## Risk 1: `gh auth login` without a TTY — CLEARED

The app's auth runner (`start_auth()` in `src-tauri/src/agents/mod.rs`) pipes stdin, stdout and stderr through `podman exec -i` with no PTY. The probe reproduced that shape.

- `gh auth login` with no flags and with `--hostname github.com --git-protocol https --web` behaved identically: no interactive prompt, no "Press Enter" step, straight into the OAuth device flow.
- Output, verbatim (stderr and stdout interleaved):
  - `! Failed to copy one-time code to clipboard`
  - `  No clipboard utilities available. Please install xsel, xclip, wl-clipboard or Termux:API add-on for termux-clipboard-get/set.`
  - `! First copy your one-time code: 7E61-9715`
  - `Open this URL to continue in your web browser: https://github.com/login/device`
- After the user entered the code in a host browser, gh printed `✓ Authentication complete.`, `! Authentication credentials saved in plain text`, `✓ Logged in as <login>` and exited 0.
- The existing URL regex in `start()` (`src/lib/agents/authSession.svelte.ts`) matches `https://github.com/login/device`. The one-time code is printed only in output, so the card must surface it.

## Risk 2: closing stdin does not end the login — BLOCKER FOR THE CURRENT CANCEL PATH

With stdin closed immediately (`echo "" |` and `</dev/null`), gh kept polling GitHub until the probe's `timeout` killed it (exit 124 after 20 seconds). `cancel_auth()` in `src-tauri/src/agents/mod.rs` cancels a login only by dropping the stdin handle. For a gh login, cancel must destroy the auth container (or kill the exec'd process); otherwise the session runs until the device code expires (about 15 minutes) and the card waits for an `Exit` event that does not arrive.

## Risk 3: where the token lands — CLEARED

- With no keyring in the container, gh writes the token to `/root/.config/gh/hosts.yml` (mode 0600, directory `/root/.config/gh/` mode 0711) in plain text, under both `github.com.oauth_token` and `github.com.users.<login>.oauth_token` (multi-account layout), with `user: <login>` and `git_protocol: https`. `config.yml` holds only preferences.
- The token is an OAuth app token: prefix `gho_`, 40 characters. `gh auth status` reports scopes `'gist', 'read:org', 'repo'`. gh-issued OAuth tokens carry no expiry and no refresh token, unlike Claude Code's rotating credential (`docs/features/shared-agent-auth/01_ideas.md`, spike 2), so identical copies in many sandboxes do not invalidate each other.
- A non-TTY login does not configure git: `/root/.gitconfig` did not exist after login.
- `gh auth token` prints the token on stdout, which is how the host can read it back from the auth container.

## Risk 4: injecting into another container over stdin — CLEARED

- `gh auth token` in `spike-gh-auth` piped into `gh auth login --with-token --hostname github.com --git-protocol https` in `spike-gh-target` (`podman exec -i`) exited 0 silently. The token travelled on stdin only, never in an argv.
- `gh auth status` in the target reported the same account and scopes. `gh api user --jq .login` returned the login.
- `gh auth setup-git` exited 0 and wrote `/root/.gitconfig` with `credential "https://github.com"` and `credential "https://gist.github.com"` sections, each holding an empty `helper =` reset and `helper = !/usr/local/bin/gh auth git-credential`. The helper path is absolute, so gh must live at a stable path.
- `git credential fill` for `host=github.com` then returned `username=<login>` and the token as the password: git over HTTPS is authenticated.

## Risk 5: validation and removal — CLEARED, with a network dependency

- `gh auth login --with-token` validates the token online before saving. A bogus token failed with `error validating token: HTTP 401: Bad credentials (https://api.github.com/)` and exit 1. Applying therefore needs the sandbox to reach `api.github.com`.
- `gh auth logout --hostname github.com` with stdin at `/dev/null` exited 0 with `✓ Logged out of github.com account <login>` and no confirmation prompt. `hosts.yml` became `{}`. After logout, `git credential fill` with `GIT_TERMINAL_PROMPT=0` failed (`terminal prompts disabled`), so the helper section left in `/root/.gitconfig` is harmless.
- Logout deletes the local copy only; the token stays valid on GitHub until revoked under the user's authorised OAuth apps.

## Not covered

- The minimum-scope check `gh auth login --with-token` applies to classic personal access tokens (believed to be `repo` and `read:org`), and how it treats a fine-grained token, was not probed: no such token was created during the spike.
- microsandbox and remote-deployed sandboxes were not probed; both are reached through the same provider exec seam.
