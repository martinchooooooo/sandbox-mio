# Shared Agent Auth Ideas

This document covers the ideation for a host-side credential store that decouples agent authentication from individual sandboxes. Today a user must run the full OAuth login (`claude auth login`) inside every sandbox that has Claude Code installed, and the credentials die with the container on destroy. The proposed feature lets the user authenticate against a service (e.g. Anthropic) once, stores the resulting credential files on the host under the Tauri app data directory, and injects (or re-injects) those files into any sandbox that needs them. Nothing of the feature is implemented; the per-sandbox auth flow it generalises is fully implemented in `src-tauri/src/agents/mod.rs` and `src/routes/projects/AgentManager.svelte` (see `docs/features/agent-setup/`).

## Current State

Per-sandbox authentication is complete and working:

- `src-tauri/src/agents/mod.rs` — `start_auth` spawns the agent's login command inside the named container via `podman exec` with piped stdin/stdout, streams output as `agent-auth-output-<session_id>` events, and `send_auth_input` delivers the one-shot PKCE code. `get_auth_status` reads the credential file back out of the container with `exec_in_container` and extracts `expiresAtMs` and `accountEmail`.
- `src/lib/agents/types.ts` — `AgentDescriptor.auth` (`ExecInteractiveAuth`) declares the login command and the container-side credential file paths: `credentialFile` (`/root/.claude/.credentials.json` for Claude Code) and `accountSourceFile` (`/root/.claude.json`). These path declarations are exactly the file set the host store must capture and inject.
- `src/lib/agents/catalogue.ts` — the static `AGENTS` array; `claude-code` is the only entry with an `auth` strategy today (OpenCode and pi are config-provisioned, no credentials).
- `src/routes/projects/AgentManager.svelte` — per-sandbox UI that runs install and auth; the seam where "inject stored credentials" replaces "log in again".
- Credentials live in the container overlay filesystem, so they survive stop/start but are destroyed by `destroy_sandbox` — documented as a UX gotcha in `docs/features/agent-setup/01_ideas.md`.

Host-side persistence seams the feature builds on:

- `src-tauri/src/settings/mod.rs` — the pattern for app-global persisted state (`app_config_dir()/settings.json`); the credential store follows the same module shape (`mod.rs` + `types.rs`, registered in `src-tauri/src/lib.rs`).
- `app_data_dir()` is already the home for large host-side artefacts: LLM models (`src-tauri/src/inference/mod.rs`) and the Mutagen data dir (`src-tauri/src/sync/mod.rs`).
- `RuntimeManager::exec_in_container` in `src-tauri/src/runtime/types.rs` — the generic exec seam for reading files out of and writing files into a container.
- `src/lib/components/GlobalNav.svelte` and the top-level route layout in `src/routes/+layout.svelte` — the navigation seam for a global (non-per-sandbox) credentials page.

## Proposed Model

### Credential Store (new module `src-tauri/src/credentials/`)

A new Rust module following the `settings` module shape: `types.rs` with a `CredentialError` enum and store types, `mod.rs` with load/save/list functions, Tauri commands registered in `src-tauri/src/lib.rs`. Credential bundles are stored per **service**, not per agent, at `app_data_dir()/credentials/<service_id>/` — one directory per service (e.g. `anthropic`), containing a verbatim copy of each credential file the service's agents need, plus a small `manifest.json` recording capture time, account email, and expiry. Keying by service rather than agent means a future second Anthropic-backed agent reuses the same bundle without a second login.

### Catalogue Extension: Credential Spec (`src/lib/agents/types.ts`)

`AgentDescriptor` grows a `credentialService` field naming the service id an agent draws from (`"anthropic"` for `claude-code`). The file list to capture/inject is derived from the existing `auth` fields (`credentialFile`, `accountSourceFile`) rather than duplicated, so the catalogue stays the single source of truth for container-side paths. Agents without `auth` (OpenCode, pi) have no `credentialService` and are unaffected.

### Capture: Global Login in a Dedicated Auth Container

The user logs in once from a new global surface (not tied to any sandbox). The login runs inside a short-lived **auth container** created from the same base image sandboxes use (`src-tauri/resources/Containerfile`), with the agent's CLI installed via the existing `install_packages` / `run_postinstall_npm` path in `src-tauri/src/agents/mod.rs`. The existing `start_auth` / `send_auth_input` machinery drives the interactive PKCE flow unchanged — only the target container name differs. On successful exit the credential files are read out of the auth container (`exec_in_container` with `cat`, or `podman cp`) into `app_data_dir()/credentials/<service_id>/`, and the auth container is removed.

A secondary capture path — "save credentials from this sandbox to the host store" — is offered in `AgentManager.svelte` for users who already authenticated a sandbox the old way; the same file-export step runs against the user's sandbox instead of an auth container.

### Injection: Copy-In Per Sandbox

Injection writes the stored credential files into a target container's filesystem at the paths declared in the catalogue descriptor. Injection is triggered:

1. Automatically at the end of agent install (`install_agent` flow in `src-tauri/src/lib.rs`), when the store holds a bundle for the agent's `credentialService` — the user never sees a login step.
2. On demand from `AgentManager.svelte` ("use saved login" / "refresh credentials") for existing sandboxes or after a re-auth.

Each sandbox receives an independent **copy**; the store is not live-shared (see the bind-mount rejection below). The injection command is a new `RuntimeManager`-level or `agents`-module function that pipes file content to `podman exec` (`tee` to the target path) or uses `podman cp`, choosing whichever survives the remote-connection case (see Open Questions).

### Status and Staleness (host `manifest.json` + existing `get_auth_status`)

The global credentials page shows per-service status from the host manifest (account email, `expiresAtMs`) without needing any container. Per-sandbox status keeps using `get_auth_status` in `src-tauri/src/agents/mod.rs` against the container copy. The host bundle is treated as a **bootstrap snapshot**: agents inside sandboxes refresh their own tokens independently after injection, and the host copy is only replaced by an explicit re-login or an explicit "save from sandbox" export. No automatic two-way sync (see tradeoffs).

### UI: Global Credentials Page

A new top-level route (e.g. `src/routes/credentials/`) linked from `src/lib/components/GlobalNav.svelte`, listing every service referenced by the catalogue with status (not connected / connected as `<email>` / expires `<date>`), a "Log in" button that runs the auth-container flow, and a "Log out" button that deletes the host bundle (container copies are untouched — stated in the UI). `AgentManager.svelte` changes behaviour when a bundle exists: the auth step collapses to a one-click inject instead of the WebviewWindow login.

## Technology Choices with Tradeoffs

### Storage: plaintext files under `app_data_dir()/credentials/` (chosen)

Chosen: store the credential files verbatim as files in the app data directory, mirroring how they exist in the container.

Alternatives evaluated:

- **OS keychain via the `keyring` crate (Windows Credential Manager)** — rejected because Claude Code's credential material is a multi-kilobyte nested JSON document plus a second config file, not a single secret string; Windows Credential Manager caps blobs at roughly 2.5 KB; and the files must be materialised as plain files inside the container at injection time anyway, so keychain storage adds an encode/decode layer without removing the plaintext-at-rest window.
- **`tauri-plugin-stronghold` encrypted vault** — rejected for v1 because it requires a user-supplied password on every app start, which contradicts the login-once convenience goal. Encryption at rest (e.g. Windows DPAPI, which needs no user password) is kept as an open question rather than rejected outright.
- **Reusing `settings.json` (`src-tauri/src/settings/mod.rs`)** — rejected because settings are readable app configuration; mixing OAuth refresh tokens into the settings file makes every settings read/write a credential-handling operation and bloats a file that other subsystems load routinely.

### Sharing mechanism: copy-in per sandbox (chosen) vs shared bind-mount vs environment variables

Chosen: inject an independent copy of the credential files into each container.

Alternatives evaluated:

- **Bind-mount a shared host credentials directory into every sandbox** — rejected on isolation grounds: any process in any sandbox could read or overwrite the credentials used by all other sandboxes, and a token refresh performed by one sandbox would race with concurrent refreshes in others. Bind-mounts also do not exist for remote deploys (`reproduce_remote` in `src-tauri/src/runtime/types.rs` seeds files by copying, not mounting), so a mount-based design would need a copy fallback anyway.
- **Inject `ANTHROPIC_API_KEY`-style environment variables** — rejected because the goal is subscription OAuth login, not API keys; Claude Code's OAuth session is file-based (`/root/.claude/.credentials.json`) with rotating refresh tokens, and environment variables cannot be updated in a running container.

### Capture context: dedicated ephemeral auth container (chosen) vs host-native CLI vs existing sandbox only

Chosen: run the global login inside a short-lived container built from the sandbox base image, reusing the existing `start_auth` plumbing in `src-tauri/src/agents/mod.rs`.

Alternatives evaluated:

- **Run the agent CLI natively on the Windows host** — rejected because Node.js plus the agent CLI would have to be installed and version-managed on the host (nothing in the app currently touches the host toolchain), the credential paths differ (`%USERPROFILE%` vs `/root`), and a host-native install contradicts the app's premise of keeping tooling inside sandboxes.
- **Capture only from an existing authenticated sandbox (no global login)** — kept as a secondary path but rejected as the primary flow, because the primary goal is authenticating *before or independent of* any sandbox; requiring a sandbox with the agent installed re-couples auth to sandbox lifecycle and fails for a fresh install with zero sandboxes.

### Staleness policy: bootstrap snapshot with explicit refresh (chosen) vs two-way sync

Chosen: the host bundle is written only at login or explicit export; injected copies then diverge as each sandbox refreshes its own tokens.

Alternative evaluated: **automatic pull-back of refreshed tokens from sandboxes into the host store** — rejected for v1 because refresh-token rotation makes concurrent refreshers race (two sandboxes refreshing from the same snapshot may invalidate each other, depending on Anthropic's rotation semantics — unverified, see Open Questions), and a background file-watcher across all containers adds machinery with no user-visible benefit as long as the host snapshot's `expiresAt` is checked before injection.

### Store keying: per-service (chosen) vs per-agent

Chosen: bundles keyed by service id (`anthropic`). Alternative of keying by agent id (`claude-code`) was rejected because credential files belong to the account/service, not the tool; a second Anthropic-backed agent (or a renamed agent id) would otherwise force a duplicate login. The cost is a mapping field (`credentialService`) on the descriptor, which is one line per catalogue entry.

## Validation

Partial spike run on 2026-07-02 against the `gg` sandbox container and a throwaway `spikeb` container (both `localhost/sandbox-mio/base:2` on `podman-machine-default`). The per-sandbox auth flow the feature reuses was validated on 2026-06-28 (recorded in `docs/features/agent-setup/01_ideas.md`), which established the credential file set (`/root/.claude/.credentials.json`, `/root/.claude.json`) and the PKCE flow shape.

Cleared risks (2026-07-02):

- **Injection transport over an SSH Podman connection** — `podman exec -i spikeb sh -c 'mkdir -p /root/.claude && cat > /root/.claude/.credentials.json && chmod 600 ...' < file` exited 0 and produced the file with mode 0600. `podman cp file spikeb:/root/.claude/...` also worked but landed the file with mode 0666. Chosen transport for `02_dev.md`: `exec` + stdin, because permissions are controllable and the command shape is identical for any `--connection`.
- **Fresh containers lack `/root/.claude`** — confirmed on `spikeb` (`ls: cannot access '/root/.claude'`); injection must `mkdir -p` first.
- **No `jq` in the base image** — `command -v jq` is empty in `sandbox-mio/base:2`, so the `.claude.json` merge cannot be done in-container with jq; the merge belongs host-side in Rust (`serde_json`), writing the merged document back via the injection transport.
- **`.claude.json` clobber risk is real** — `gg`'s `/root/.claude.json` contains per-container state (`machineID`, `userID`, `firstStartTime`, migration flags); injecting a captured file wholesale would overwrite the target container's `machineID`. Confirms the merge-only-`oauthAccount` direction pending spike 3.
- **Login flow start via FIFO stdin** — `claude auth login` launched detached in `gg` with a FIFO on stdin printed the OAuth URL and the paste prompt into the captured output file, reproducing the 2026-06-28 result on Claude Code 2.1.195.

Completed (2026-07-02, second pass): the user completed the OAuth login in `gg` (real PKCE flow, code pasted back interactively), unblocking spikes 1–3.

1. **Portability of copied credentials** — CONFIRMED. Copied `/root/.claude/.credentials.json` and `/root/.claude.json` verbatim (`podman exec cat | podman exec -i ... cat >`, `MSYS_NO_PATHCONV=1` on the Git Bash host) from `gg` into a fresh throwaway container `spikeb` (same base image, Claude Code installed via `mise use` + manual `run_postinstall_npm`-equivalent). A non-interactive `claude -p` prompt in `spikeb` succeeded immediately with no re-auth. Anthropic credentials are not bound to machine or container identity.
2. **Refresh-token rotation semantics** — RESOLVED, and the risk is real: rotation is single-use. After copying, `expiresAt` was force-set to `1` in `spikeb`'s credential file and a `claude -p` call there triggered a refresh (`expiresAt` advanced, new token issued). Immediately after, the *original*, unmodified copy in `gg` failed with `Not logged in · Please run /login` — the refresh in `spikeb` invalidated `gg`'s refresh token even though `gg`'s file was never touched. **Only one injected copy can be "active" at a time**; the moment any copy refreshes, all sibling copies (including the host-captured snapshot's origin) are invalidated. This closes the open question: two sandboxes cannot safely run from the same snapshot indefinitely. The design needs either (a) a "re-inject from host after re-login" recovery flow surfaced per-sandbox, or (b) treating injection as effectively single-use per login until a policy for multi-sandbox refresh is designed (e.g. a lock, or accepting that only the most-recently-refreshed sandbox stays authenticated).
3. **Minimal `.claude.json` injection** — CONFIRMED merge-only is correct and safe. A third fresh container `spikec` was given its own `.claude.json` (created on first unauthenticated `claude -p` call, containing a unique `machineID`/`userID`/`firstStartTime`). Merging in only the `oauthAccount` key from `gg`'s file (via a Node script run inside the container, never printed to any log) left `machineID` and `userID` unchanged (`preserved: true`) while adding a valid `oauthAccount` — confirms the merge-only-`oauthAccount` direction over whole-file replacement, which would have clobbered `spikec`'s identity.

Both throwaway containers (`spikeb`, `spikec`) were removed after the spike. `gg` is left logged out (`Not logged in · Please run /login`) as a direct consequence of spike 2 and needs a fresh login before further use as a dev sandbox.

## Open Questions

- ~~**Refresh-token rotation**~~ RESOLVED 2026-07-02: yes, Anthropic invalidates the previous refresh token on use (confirmed by spike 2 — `gg`'s original token died the moment `spikeb`'s copy refreshed). Design must pick between per-sandbox fresh logins after expiry and a "re-inject from host after re-login" recovery flow; a "one active copy" constraint is the practical consequence and should be called out explicitly in `02_dev.md` and in the credentials-page UI copy.
- ~~**`.claude.json` merge strategy**~~ RESOLVED 2026-07-02: merge only the `oauthAccount` subtree into the target's existing `/root/.claude.json`, leaving `machineID`/`userID`/`firstStartTime` untouched (confirmed by spike 3). Whole-file replacement is rejected.
- **Encryption at rest**: Leave bundles as plaintext files in `app_data_dir()/credentials/` or wrap them with Windows DPAPI (no user password needed) before write? Affects only the store module's read/write functions, so the decision can land in `02_dev.md` without design changes.
- **Injection transport for remote deploys**: `podman cp` vs `exec` + `tee` over a `--connection` remote; whichever is chosen must work through the Podman system connections used by `reproduce_remote` in `src-tauri/src/runtime/types.rs`. Also whether remote sandboxes should receive credentials at all by default.
- **Auth container lifecycle**: Name, reuse-vs-recreate policy (a persistent hidden auth container makes re-login faster but is one more container to manage; an ephemeral one pays the mise install of the agent CLI on every login), and cleanup on failure.
- **Multiple accounts per service**: Whether the store supports more than one bundle per service (work + personal Anthropic accounts) with a per-sandbox account picker, or one bundle per service in v1. Affects the store directory layout (`credentials/<service_id>/` vs `credentials/<service_id>/<account>/`).
- **Automatic injection at sandbox creation**: Should freshly created sandboxes with a catalogued agent get credentials injected automatically at `create` time, or only when the agent is installed? Depends on whether agent install remains the only path that puts the agent in the container.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Injected credentials work, then all sandboxes but one report auth errors after some hours | Refresh-token rotation: multiple containers refreshing from the same copied `refreshToken` may invalidate each other (unverified — Open Questions) | Run spike 2 before building; if rotation is single-use, surface a "re-inject from host" action in `AgentManager.svelte` and re-login on the credentials page when the host snapshot's `expiresAt` passes |
| Sandbox loses onboarding state or project history after credential injection | Injecting the whole captured `/root/.claude.json` overwrites per-sandbox non-credential state stored in the same file | Merge only the `oauthAccount` subtree into the container's existing `/root/.claude.json` (strategy pending spike 3) |
| Credentials on the host are readable by any local process | Bundles are plaintext files under `app_data_dir()/credentials/` | Accepted for v1 (matches the container-side plaintext the agents themselves use); DPAPI wrapping is an open question in `docs/features/shared-agent-auth/01_ideas.md` |
| "Log out" on the credentials page does not de-authenticate existing sandboxes | Injection copies files; deleting the host bundle leaves every container copy in place | State the behaviour in the credentials page UI; per-sandbox removal means deleting `/root/.claude/.credentials.json` in each container via `exec_in_container` |
| Global login fails with "native binary not installed" in the auth container | The auth container installs Claude Code via mise, which skips the npm postinstall (`--ignore-scripts=true`) | Reuse `run_postinstall_npm` in `src-tauri/src/agents/mod.rs` in the auth-container provisioning, exactly as the per-sandbox install does |
| Injected file has wrong ownership or missing parent directory | Fresh containers have no `/root/.claude/` directory (confirmed 2026-07-02), and `podman cp` writes files with mode 0666 | Injection must `mkdir -p` the parent directory first; prefer `podman exec -i` + `cat > file` + `chmod 600` over `podman cp` (validated 2026-07-02) |
| Podman exec path becomes `C:/Program Files/Git/root/work` and exec fails with exit 125 | Git Bash (MSYS) rewrites arguments that look like absolute POSIX paths before podman sees them | Prefix the command with `MSYS_NO_PATHCONV=1` when invoking `podman exec --workdir /root/...` from Git Bash; irrelevant for the Rust backend, which spawns podman directly |
| Two sandboxes injected from the same host snapshot: the second one to refresh works, the first now says "Not logged in" | Refresh-token rotation is single-use (confirmed 2026-07-02, spike 2) — refreshing in one container invalidates the refresh token everywhere else it was copied | No fix in v1; surface a "re-inject from host after re-login" action per-sandbox and document the "one active copy" constraint in the credentials-page UI |
| `claude --version` or first `claude` invocation fails with "native binary not installed" even outside the auth container | `mise use`/`mise install` alone (without the postinstall step) is not sufficient to make `claude` runnable at all, not just for login — confirmed while provisioning throwaway spike containers | Always pair `mise use npm:@anthropic-ai/claude-code@<version>` with the `run_postinstall_npm` step before any `claude` invocation, not only before `auth login` |
