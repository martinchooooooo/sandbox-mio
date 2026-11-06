---
type: backlog
title: "Technical Debt"
---
# Technical Debt

This document records deliberate workarounds that are known to be suboptimal and are kept only until a better solution is built. Each entry states what the workaround is, why it is a workaround, the concrete risk it carries, and what would replace it.

## Zellij web-terminal token written to the synced workspace

**Where:** `RuntimeManager::open_terminal` (`src-tauri/src/runtime/mod.rs`), feature `docs/features/web-terminal/`.

**What:** When `open_terminal` starts the Zellij web client it runs `zellij web --create-token`, reads the one-time token from the command's stdout, and writes it (with the terminal URL) to the file `/root/work/.mio/zellij.json` inside the sandbox workspace. The plaintext token is the value the browser/webview presents to log in; Zellij itself only persists a hash (`/root/.local/share/zellij/tokens.db`) and never re-emits the plaintext, so the app captures it at creation time. The `/root/work/.mio/` directory is the app's own scratch location inside the workspace bind-mount.

**Why it is a workaround:** `/root/work` is the bind-mounted workspace. It is mirrored to remote hosts by the Mutagen sync session (see `docs/features/remote-sync/`) and is the directory the user edits and may commit to version control. Storing a live login token there means a secret travels to every remote the sandbox deploys to and can be accidentally committed. A token kept only in process memory, or in an OS keychain, would not have this exposure. The file-on-disk approach was chosen now because it is the simplest way to make the token readable across the app's components without yet building secret storage.

**Risk:** the token grants terminal (shell) access to the sandbox. Anyone who can read `/root/work/.mio/zellij.json` — on the laptop, on a synced remote, or in a repository the workspace was committed to — can authenticate to the running web terminal. The token is revocable (`zellij web --revoke-all-tokens`) but the file is not automatically cleaned up.

**Mitigations while the workaround stands:** add `.mio/` to the workspace's ignore rules so it is not committed; treat the file as a secret; revoke and re-mint if exposure is suspected.

**Replacement:** hold the plaintext token only transiently (frontend memory for the app session) and, if cross-restart persistence is needed, store it in an OS keychain rather than a file. A `WebviewWindow`'s shared cookie store may make persistence unnecessary because the first login establishes a session cookie. Tracked as the deferred "auto-login / token storage" follow-up in `docs/features/web-terminal/02_dev.md`.
