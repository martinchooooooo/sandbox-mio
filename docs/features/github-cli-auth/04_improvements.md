---
type: improvements
title: "GitHub CLI Login: Deferred Improvements"
feature: github-cli-auth
---
# GitHub CLI Login: Deferred Improvements

Ideas raised while scoping `github-cli-auth` (`docs/features/github-cli-auth/01_ideas.md`) that are worthwhile but outside the first build.

## Keep the token out of proxy sandboxes with secret swapping

A proxy-enabled sandbox could hold a placeholder instead of the real token, with the sidecar swapping it in only for `api.github.com` and `github.com` (`docs/architecture/network.md`, secret swapping). First deferred over two assumed blockers: git's Basic `Authorization` header base64-encodes the placeholder, and `gh auth login --with-token` validates the placeholder online. The 2026-11-18 spike (`spike/proxy-swap.md`) cleared both for iron-proxy 0.56.0 sandboxes: iron-proxy decodes and re-encodes Basic auth, and gh's validation request is swapped on its way out. The remaining work is product scope: every sandbox behind a sidecar, a host-sourced binding value, and binding-before-apply ordering. Now planned in `docs/features/proxy-credential-secrets/01_ideas.md`.

## A Sandbox-mio GitHub OAuth app

The app could run the GitHub device flow itself against its own registered OAuth app, with no auth container. Deferred because it needs a maintainer-owned OAuth app and client-id management (`01_ideas.md`, "Token acquisition").

## Narrower tokens per sandbox

gh's login always grants `repo`, `read:org` and `gist`, which reach every repository the user can access. A per-sandbox fine-grained token, limited to the repositories that sandbox works on, would shrink the blast radius of an untrusted agent. Deferred because GitHub offers no API to mint fine-grained personal access tokens on the user's behalf, so each token would be created by hand.

## Import an existing host gh login

When the host has gh installed and logged in, the Credentials page could offer to import `gh auth token` instead of a new device-flow login. Deferred because gh is uncommon on the Windows hosts the app targets.

## Deployed sandboxes

The first build applies the token only to local sandboxes. A sandbox deployed to a remote Podman host (`docs/architecture/remote.md`) could receive it through the same exec seam over the remote connection. Deferred because nobody uses remote deploy yet (user decision, 2026-11-18), and because the token would then sit on a machine the user may not fully control, which deserves its own opt-in.

## Multiple GitHub accounts

The store holds one `github` credential. Work and personal accounts with a per-sandbox picker would change the store layout (`credentials/<service>/<account>/`); the same question is open for Claude in `docs/features/shared-agent-auth/01_ideas.md`.

## Reconcile running sandboxes at app startup

`reconcile_sandbox()` (`src-tauri/src/services/github_access.rs`, planned in `02_dev.md` Phase D) runs on create, `start_sandbox`, save, delete and the Manage switch. A sandbox started outside the app (`podman start`, or a provider start inside an exec helper) keeps a stale or removed token until one of those events. A best-effort sweep over running sandboxes at startup would close the gap. Deferred because the Podman machine may not be ready at startup, and the Manage card already shows "out of date" with a retry (`02_dev.md`, Resolved Decision 6).

## Activity entries for GitHub apply

Applying the token to every running sandbox after a login runs silently in the background, and only the Manage card shows the outcome. A `TaskRegistry` entry (`src-tauri/src/activity/`) per apply-all would show progress and failures in the activity center. Deferred because each apply takes about a second and failures are already visible per sandbox.

## Capture secret stdout without a temp file

The Podman process runner feeds a call's stdin through a pipe, but still captures stdout and stderr to plaintext temporary files (`src-tauri/src/process/mod.rs`), so the GitHub token briefly lands on host disk during the `gh auth token` capture, as does the Claude credential archive during credential capture. Stdout cannot simply become a pipe: a grandchild holding the pipe open keeps `wait_with_output()` from ever seeing end-of-file. Candidate fixes are a delete-on-close temporary capture file read back through the app's own handle, or pipe reader threads with a bounded drain after the direct child exits. Deferred because the temporary files are removed after each call.
