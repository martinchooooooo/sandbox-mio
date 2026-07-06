# Shared Agent Auth — Dev Plan

This document is the build plan for the host-side credential store described in
`docs/features/shared-agent-auth/01_ideas.md`. It continues `01_ideas.md`'s Proposed Model and
supersedes its Open Questions with concrete resolutions, informed by the 2026-07-02 spike that
completed a real OAuth login in the `gg` sandbox and validated portability, refresh-token
rotation, and `.claude.json` merge strategy (recorded in `01_ideas.md`'s Validation section).
Status: nothing in this document is implemented yet; per-sandbox auth (`src-tauri/src/agents/mod.rs`,
`src/routes/projects/AgentManager.svelte`) is the only piece that exists today and every phase
below reuses it rather than replacing it.

## The Concrete Model

### New Rust module: `src-tauri/src/credentials/`

Two files, following the shape of `src-tauri/src/settings/`:

- `src-tauri/src/credentials/types.rs` — `CredentialError` (thiserror enum: `Io`, `Parse`,
  `ExecFailed`, `NotFound`), `CredentialManifest` (serde struct: `captured_at_ms: u64`,
  `account_email: Option<String>`, `expires_at_ms: Option<u64>`), and `AUTH_CONTAINER_PREFIX: &str
  = "mio-auth-"` (the reserved name prefix that marks a container as an ephemeral auth container
  rather than a user sandbox).
- `src-tauri/src/credentials/mod.rs` — the store and transport functions (Phase A/C/D below).

Bundles live at `app_data_dir()/credentials/<service_id>/`, mirroring `models_root`/`model_dir` in
`src-tauri/src/inference/mod.rs`. Each service directory holds three files, none of which are ever
printed to logs or returned to the frontend as raw content — only `CredentialManifest` fields
cross the Tauri IPC boundary:

- `home.tar.gz` — a `tar czf` archive of the credential file's parent directory (e.g. `~/.claude`,
  covering `.credentials.json` and everything alongside it: `settings.json`, onboarding caches,
  etc.) plus the whole `auth.accountSourceFile` (e.g. `~/.claude.json`), captured and restored
  verbatim — see Resolved Decisions on why whole-directory replacement superseded the original
  merge-only design.
- `manifest.json` — the serialized `CredentialManifest`.

### Catalogue extension: `src/lib/agents/types.ts`, `src/lib/agents/catalogue.ts`

`AgentDescriptor` (`src/lib/agents/types.ts`) grows `credentialService?: string`. In
`src/lib/agents/catalogue.ts`, the `claude-code` entry sets `credentialService: "anthropic"`.
`opencode` and `pi` are untouched (no `auth`, so no `credentialService`). (`ExecInteractiveAuth`
briefly grew an `accountMergeKey?: string` field for the merge-only strategy; removed 2026-07-03
when that strategy was superseded — see Resolved Decisions.)

### Capture: dedicated ephemeral auth container, reusing `SandboxBackend`

The auth container is a normal sandbox container created through the existing
`SandboxBackend::create`/`SandboxBackend::destroy` (`src-tauri/src/backends/mod.rs`), named
`mio-auth-<service_id>` (the `AUTH_CONTAINER_PREFIX` constant). Reusing `SandboxBackend::create`
means `ensure_base_image`, the `/root/work` bind mount, and the mise toolchain setup all work
unmodified, so `agents::install_packages`, `agents::run_postinstall_npm`, and `agents::start_auth`
(`src-tauri/src/agents/mod.rs`) run against it exactly as they do for a user sandbox — only the
container name differs. `src-tauri/src/lib.rs`'s `list_sandboxes` command filters out any
`Sandbox` whose name starts with `AUTH_CONTAINER_PREFIX` before returning to the frontend, so the
auth container never appears in `src/routes/projects/`.

Capture flow, driven by a new Tauri command `capture_agent_credentials` in `src-tauri/src/lib.rs`
that calls `credentials::capture` (`src-tauri/src/credentials/mod.rs`):

1. `SandboxBackend::create("mio-auth-<service_id>")` if the container does not already exist
   (`SandboxBackend::list` first; recreate-on-missing, not reuse-forever — see Resolved Decisions).
2. `agents::install_packages` + `agents::run_postinstall_npm` with the agent's `misePackages` /
   `postInstallSteps` (frontend passes these from the catalogue, same shape `install_agent` already
   takes).
3. `agents::start_auth` / `agents::send_auth_input` — unchanged, driven by the same
   `/agent-auth` `WebviewWindow` flow `src/routes/projects/AgentManager.svelte`'s `openAgentAuth`
   uses today, just pointed at the auth container's name instead of a sandbox name.
4. On `AgentAuthEvent::Exit { code: 0 }`, `credentials::capture` reads
   `auth.credentialFile` and `auth.accountSourceFile` out of the auth container via
   `RuntimeManager::exec_in_container` (`cat`), extracts the `accountMergeKey` subtree from the
   parsed `accountSourceFile` JSON with `serde_json`, and writes all three files under
   `app_data_dir()/credentials/<service_id>/` (Phase A's `credentials::store_dir`).
5. `SandboxBackend::destroy("mio-auth-<service_id>")` — the auth container does not persist
   between logins (see Resolved Decisions on lifecycle).

The secondary capture path ("save credentials from this sandbox") calls the same
`credentials::capture` function with an existing sandbox's name instead of the auth container's,
skipping steps 1–3.

### Injection: exec + stdin, never `podman cp`

A new function `credentials::inject` (`src-tauri/src/credentials/mod.rs`), exposed as the Tauri
command `inject_agent_credentials`, writes the stored bundle into a target container using the
transport validated in the spike: `podman exec -i <name> sh -c 'mkdir -p <dir> && cat ><path> &&
chmod 600 <path>'` with the file bytes piped to stdin, run directly via `tokio::process::Command`
(same pattern as `agents::start_auth`, not through `RuntimeManager::exec_in_container`, because
that method has no stdin support — see `src-tauri/src/runtime/mod.rs:270`). `podman cp` is
rejected outright (confirmed in the spike: it lands files at mode `0666`).

Injection performs two writes:

1. `credential-file.json` → `auth.credentialFile`, mkdir-then-write-then-chmod as above.
2. `account-merge.json` merged into `auth.accountSourceFile`: `cat` the target's existing file (or
   treat a `cat` failure as `{}` for a fresh container), parse with `serde_json`, set
   `target[accountMergeKey] = stored_subtree`, serialize, and write back with the same
   transport. This is the merge-only strategy validated by spike 3 in `01_ideas.md` — the whole
   captured `accountSourceFile` is never used for injection, only for capture.

Injection is called from two places, per `01_ideas.md`'s Proposed Model:

1. `install_agent` (`src-tauri/src/lib.rs`) — after `agents::install_packages` and
   `agents::run_postinstall_npm` succeed, check whether `credentials::list` has a bundle for the
   agent's `credentialService`; if so, call `credentials::inject` automatically before returning
   `AgentInstallEvent::Done`.
2. An explicit "use saved login" / "refresh credentials" action in
   `src/routes/projects/AgentManager.svelte`, calling `inject_agent_credentials` directly.

### UI: `src/routes/credentials/+page.svelte`

A new top-level route, linked from `src/lib/components/GlobalNav.svelte` (which today only holds
the theme toggle and locale switcher). Lists every distinct `credentialService` present in
`src/lib/agents/catalogue.ts`'s `AGENTS`, with per-service status read from
`list_credential_bundles` (wraps `credentials::list`, returning `Vec<(String, CredentialManifest)>`).
"Log in" invokes `capture_agent_credentials` and opens the same `/agent-auth` `WebviewWindow` UI
`AgentManager.svelte` already uses. "Log out" invokes `delete_credential_bundle` (wraps
`credentials::delete`, which removes `app_data_dir()/credentials/<service_id>/` — container copies
are untouched, and the page states this explicitly per `01_ideas.md`'s gotcha table).
`src/routes/projects/AgentManager.svelte`'s `openAgentAuth` changes: when
`list_credential_bundles` already has an entry for the agent's `credentialService`, the auth step
becomes a single "use saved login" button calling `inject_agent_credentials` instead of opening the
`WebviewWindow` login.

## Resolved Decisions

Restates every Open Question from `01_ideas.md`:

- **Refresh-token rotation**: RESOLVED by spike 2 — Anthropic invalidates the previous refresh
  token the moment any copy refreshes. Consequence for this build: injected bundles are **single
  active copy** in practice. `credentials::inject` does not attempt to prevent double-injection
  (no lock), but `src/routes/credentials/+page.svelte` and `AgentManager.svelte`'s "use saved
  login" / "refresh credentials" copy must tell the user that injecting into a second sandbox may
  silently log the first one out. No automatic detection of "which copy is still valid" is built
  in v1; a sandbox that starts failing with `Not logged in` after another sandbox's agent ran is
  expected to need a manual "refresh credentials" re-injection or a fresh host login.
- **`.claude.json` merge strategy**: SUPERSEDED 2026-07-03 — spike 3's merge-only strategy
  (merging only `accountMergeKey`'s subtree into the target's existing `accountSourceFile`) shipped
  first, but field use surfaced a gap it didn't cover: a target sandbox that never ran the agent
  interactively has none of the CLI's own onboarding-completion state in its `accountSourceFile`
  (e.g. Claude Code's first-run wizard), so even with a valid injected token the interactive UI
  still re-runs first-run setup, including its own login-method prompt — confusing since the token
  underneath is actually fine (confirmed non-interactive invocation succeeds with no prompt).
  RESOLVED now with whole-directory replacement instead: `capture`/`inject` tar the credential
  file's parent directory (e.g. `~/.claude`) plus the whole `accountSourceFile` (e.g.
  `~/.claude.json`) and extract verbatim over the target, so the target inherits the source's
  completed onboarding state along with the token. `machineID`/`userID`/per-container identity are
  no longer preserved on inject — accepted, since the source (an ephemeral auth container that just
  finished a real interactive login) already has a fully-formed, valid version of that state and
  the common target (a freshly agent-installed sandbox) had none to lose. `accountMergeKey` is
  removed from the catalogue schema.
- **Encryption at rest**: RESOLVED for v1 — plaintext files under `app_data_dir()/credentials/`,
  matching the container-side plaintext the agents themselves use. No DPAPI wrapping in this
  phase; if added later it only changes the read/write bodies of `credentials::capture` and
  `credentials::inject`, not their signatures or the store's directory layout.
- **Injection transport for remote deploys**: RESOLVED — `podman exec` piping a `tar -czf -`
  archive through host stdout/stdin to `tar -xzpf -` on the other side, never `podman cp` (confirmed
  landing files at mode `0666` regardless of source) and never `exec_in_container` (its
  `String::from_utf8_lossy` output would corrupt binary gzip data). The transport takes a
  `--connection` argument identically for local and remote targets (`RuntimeManager::local_connection`
  / the connection `reproduce_remote` uses in `src-tauri/src/runtime/types.rs`), so no
  remote-specific code path is needed. Whether remote sandboxes receive credentials **by default**
  is deferred to Phase E UI copy, not resolved here — the mechanism works either way.
- **Auth container lifecycle**: RESOLVED with a provisional, revisitable choice — ephemeral,
  recreated on every login rather than kept running. Named `mio-auth-<service_id>`
  (`credentials::types::AUTH_CONTAINER_PREFIX`), created via `SandboxBackend::create`, destroyed
  via `SandboxBackend::destroy` after capture succeeds or fails. This pays the mise install cost on
  every login in exchange for zero standing container to manage or leak; if login latency proves
  too high in practice, switching to reuse-if-present is a one-line change to step 1 of the Capture
  flow above, not a redesign.
- **Multiple accounts per service**: RESOLVED for v1 — one bundle per service
  (`credentials/<service_id>/`), no per-account picker. `credentials::capture` overwrites any
  existing bundle for the same `service_id` on a fresh login. A future multi-account layout
  (`credentials/<service_id>/<account>/`) is out of scope for this phase.
- **Automatic injection at sandbox creation**: RESOLVED for v1 — injection happens only at
  `install_agent` time (Phase D above), not at `SandboxBackend::create`. A freshly created sandbox
  with no agents installed has nothing to inject into; tying injection to install keeps the two
  concerns (container existing vs. agent present) independent, matching how `install_agent`
  already owns the only path that puts an agent's files in a container.

## Implemented: Phase A — Credential Store Module

Files: `src-tauri/src/credentials/types.rs`, `src-tauri/src/credentials/mod.rs`. Functions:
`store_dir(app) -> PathBuf` (`app_data_dir()/credentials`, created if absent, mirrors
`settings::settings_path` / `inference::models_root`), `bundle_dir(app, service_id) -> PathBuf`,
`load_manifest(app, service_id) -> Result<CredentialManifest, CredentialError>`, `save_manifest`,
`list(app) -> Result<Vec<(String, CredentialManifest)>, CredentialError>` (scans `store_dir`
subdirectories), `delete(app, service_id) -> Result<(), CredentialError>`. Registered `mod
credentials;` in `src-tauri/src/lib.rs` next to `mod agents;` / `mod settings;`. Added
`CredentialError` to the `AppError` enum in `src-tauri/src/error.rs` (`#[error(transparent)]
Credential(#[from] CredentialError)`, `kind()` arm `"Credential"`), following the existing
`SettingsError` arm, and the matching `"Credential"` member in `AppErrorKind`
(`src/lib/errors.ts`). `store_dir`/`bundle_dir`/`load_manifest`/etc. are unused until Phase C/D
wire them into commands — `cargo check` reports them as dead code until then, which is expected.
The per-bundle payload file (`home.tar.gz`, see Phase C/D below) is written in Phase C.

## Implemented: Phase B — Catalogue Extension

Edited `src/lib/agents/types.ts`: added `credentialService?: string` to `AgentDescriptor`. Edited
`src/lib/agents/catalogue.ts`: set `credentialService: "anthropic"` on the `claude-code` entry. No
backend changes in this phase — the field is read by the frontend when building Phase C/D command
arguments. (`ExecInteractiveAuth.accountMergeKey` was added here originally for the merge-only
strategy and removed 2026-07-03 — see Resolved Decisions.)

## Implemented: Phase C — Capture

Functions added in `src-tauri/src/credentials/mod.rs`: `ensure_auth_container(name, backend: &Arc<dyn
SandboxBackend>) -> Result<(), CredentialError>` (lists first, creates only if missing — no
`connection` parameter: `SandboxBackend`/`RuntimeManager` are already pinned to one connection
app-wide, matching every other agent command in `lib.rs`), `capture(app, name, service_id,
credential_path, expires_at_json_key, account_source_path, account_email_json_key, runtime: &Arc<dyn
RuntimeManager>) -> Result<CredentialManifest, CredentialError>`. `capture` reads `credential_path`
and (if given) `account_source_path` via `exec_in_container` `cat` purely to compute the
`CredentialManifest`'s `account_email`/`expires_at_ms` (small JSON, safe for the lossy
`String::from_utf8_lossy` `exec_in_container` uses), then separately tars the credential file's
parent directory plus `account_source_path` via `tar_capture` (2026-07-03, see Phase D) and stores
the archive as `home.tar.gz`. Tauri commands added in `src-tauri/src/lib.rs`: `ensure_auth_container`
(wraps the Rust function so the frontend can create the auth container idempotently before driving
`install_agent`/`start_agent_auth` against it), `capture_agent_credentials` (parameter shape mirrors
`get_agent_auth_status`'s `credential_path` / `expires_at_json_key` / `account_source_path` /
`account_email_json_key` quartet plus `service_id`), `list_credential_bundles`,
`delete_credential_bundle`. `list_sandboxes` gains the `AUTH_CONTAINER_PREFIX` filter.
`agents::json_path_get` was changed from private to `pub(crate)` so `credentials::capture` reuses
it instead of duplicating the dot-path JSON walk. The auth-container *login* orchestration itself
(create → install → interactive auth → capture → destroy) is Phase E's job, driven from the
frontend by chaining this phase's `ensure_auth_container` / `capture_agent_credentials` with the
pre-existing `install_agent` / `start_agent_auth` / `send_agent_auth_input` / `destroy_sandbox`
commands — no new Rust orchestration function wraps that whole sequence, since the interactive PKCE
step can only be driven from the frontend's `WebviewWindow` already used for per-sandbox auth.

## Implemented: Phase D — Injection

Function added in `src-tauri/src/credentials/mod.rs`: `inject(app, name, service_id, runtime:
&Arc<dyn RuntimeManager>) -> Result<(), CredentialError>` (no `credential_path`/
`account_source_path` params — the target paths are baked into the stored archive's member paths,
not re-supplied by the caller). `inject` reads `home.tar.gz` and calls `tar_inject` to extract it.

2026-07-03 rework: `capture`/`inject` originally shipped writing/merging individual files via a
`write_file_via_stdin` helper (`mkdir -p && cat > && chmod 600` piped to stdin). That was replaced
with archive-based transport once the merge-only strategy was found to leave the interactive TUI
re-running onboarding on injected targets (see Resolved Decisions) — `write_file_via_stdin` is
gone, replaced by two helpers: `tar_capture(connection, name, paths) -> Result<Vec<u8>, ...>` runs
`podman exec <name> tar -C / -czf - <relative paths>` directly via `tokio::process::Command`
(never `RuntimeManager::exec_in_container`, whose `String::from_utf8_lossy` would corrupt binary
gzip output) and collects stdout as bytes; `tar_inject(connection, name, archive)` pipes those
bytes to `podman exec -i <name> tar -C / -xzpf -` (the `-p` flag preserves the credential file's
`0600` mode from the archive, same guarantee the old explicit `chmod 600` gave). Both paths are
made tar-relative (leading `/` stripped) so archive members restore to the same absolute location
they were captured from.

Tauri command `inject_agent_credentials` added in `src-tauri/src/lib.rs` (now just `name`,
`service_id`). `install_agent` gains one new optional parameter (`credential_service`, supplied by
`AgentManager.svelte`'s `installAgent` from the catalogue descriptor — `credential_path` and
`account_source_path` were dropped 2026-07-03, since `inject` no longer needs them) and, after
postinstall succeeds, calls `credentials::inject` directly when `credentials::load_manifest`
confirms a bundle exists for `credential_service` — no separate call through
`inject_agent_credentials` for the auto-inject path.

## Implemented: Phase E — UI

New route `src/routes/credentials/+page.svelte`, listing every distinct `credentialService` in
`AGENTS` with `data-service`/`data-status` attributes (matching `03_e2e.md`'s page-object
contract) and "Log in"/"Log out" buttons. "Log in" drives the full auth-container flow end to end
from the frontend: `ensure_auth_container` → `install_agent` (packages from the owning agent's
descriptor) → `start_agent_auth`, opening the same `/agent-auth` `WebviewWindow` used for
per-sandbox auth; the page separately subscribes to the `agent-auth-output-<session>` event (the
same app-wide Tauri event the `/agent-auth` window listens to) so it can react to `Exit` itself —
on `code === 0` it calls `capture_agent_credentials`, and in a `finally` block it always calls
`destroy_sandbox({ force: true })` on the auth container, satisfying the "auth container cleanup
on failure" cross-cutting concern from the frontend side (Rust has no single function spanning the
whole interactive flow, since the PKCE step can only be driven by the `WebviewWindow`). "Log out"
calls `delete_credential_bundle`. Edited `src/lib/components/GlobalNav.svelte`: added a
`<Button href="/credentials">` link (renders as `<a href="/credentials">`, matching the e2e
`navLink` selector).

Edited `src/routes/projects/AgentManager.svelte`: added `credentialBundles` state loaded via
`list_credential_bundles` in `onMount`; the per-agent auth row now branches three ways — already
authenticated (existing "Re-authenticate" button, unchanged), unauthenticated with a bundle for
`agent.credentialService` (new "Use saved login" button calling `inject_agent_credentials`
directly, no `WebviewWindow`), or unauthenticated with no bundle (existing "Authenticate" button,
unchanged, opens `/agent-auth`). `installAgent` also now passes `credentialService` /
`credentialPath` / `accountSourcePath` from the descriptor to `install_agent`, wiring Phase D's
auto-inject-at-install-time path. New Tauri commands consumed: `list_credential_bundles`,
`delete_credential_bundle`, `capture_agent_credentials`, `inject_agent_credentials`,
`ensure_auth_container`.

## Lifecycle / Cross-Cutting Concerns

- **Auth container cleanup on failure**: if `agents::start_auth` exits non-zero, or the frontend
  cancels via `agents::cancel_auth`, `credentials::capture` must still call
  `SandboxBackend::destroy` on the auth container in a `finally`-equivalent path (Rust: wrap the
  capture body so destroy runs on both `Ok` and `Err`), otherwise a failed login leaks a
  `mio-auth-<service_id>` container that `list_sandboxes`'s filter hides from the UI but that
  still consumes Podman resources.
- **Single active copy**: no code in this plan enforces it (see Resolved Decisions above); it is a
  documentation and UI-copy obligation on Phase E, not a Phase A–D mechanism. A future phase could
  add a lock file in the bundle directory recording which container last refreshed, but that is
  explicitly out of scope here.
- **State never crosses IPC as raw content**: every Tauri command in Phases C/D returns
  `CredentialManifest` or `()`, never the file contents. `credentials::capture` and
  `credentials::inject` are the only functions that see credential bytes, and both operate purely
  host-filesystem-to-container-stdin or container-stdout-to-host-filesystem, matching how the spike
  was run (`podman exec cat | podman exec -i ... cat >`, never through a shell that echoes content).

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `capture_agent_credentials` succeeds but a sandbox that later injects the same bundle reports "Not logged in" hours later | Refresh-token rotation is single-use (spike 2, `01_ideas.md`): the sandbox that refreshes first invalidates every other copy of the same bundle, including copies injected earlier | Re-run "refresh credentials" (`inject_agent_credentials`) in the affected sandbox after a fresh host login (`capture_agent_credentials`); do not treat the host bundle as eternally valid |
| `podman cp`-based injection lands files with wrong permissions | `podman cp` sets mode `0666` regardless of source mode (confirmed in the spike) | Use `credentials::tar_inject`'s `tar -xzpf -` transport exclusively; never call `podman cp` for credential files |
| Injected credentials work (`claude auth status` → `loggedIn: true`) but the terminal's interactive `claude` still opens the first-run onboarding wizard (theme/trust), which non-technical users read as "not logged in" | Whole-directory capture (2026-07-03) can only carry onboarding state that the *source* actually has — but the source is an ephemeral auth container that only ran `claude auth login`, which authenticates without ever running the interactive onboarding. So the captured (and every injected) `accountSourceFile` has empty onboarding state. Confirmed 2026-07-06: `hasCompletedOnboarding` was absent and `ccOnboardingFlags` was `{}` in an injected `.claude.json` even though auth was valid. | Fixed 2026-07-06 by `credentials::seed_onboarding`: after every injection (`install_agent`, `inject_agent_credentials`) and after a per-sandbox `auth login` (`seed_agent_onboarding`, called from `/agent-auth`), patch the target `accountSourceFile` to set the catalogue-declared onboarding flags. For Claude Code 2.1.195: top-level `hasCompletedOnboarding: true` gates the welcome/theme wizard; `projects["/root/work"].hasTrustDialogAccepted: true` gates the trust dialog. **`ccOnboardingFlags` is a red herring** — it stays `{}` even after a genuine onboarding, so do not gate on it. Flag names live in `src/lib/agents/catalogue.ts` (`auth.onboarding`) beside the version pin, so a version bump prompts re-verification. |
| `mio-auth-<service_id>` containers accumulate in `podman ps -a` | `credentials::capture` did not reach its `SandboxBackend::destroy` call on an error path (login failed, app crashed mid-capture) | Wrap the capture body so `SandboxBackend::destroy` runs regardless of `agents::start_auth`'s outcome; manually `podman rm -f mio-auth-<service_id>` to clean up an already-leaked container |
| Auth container's `claude --version` fails with "native binary not installed" even for non-login commands | `mise use`/`mise install` alone does not run the npm postinstall script; confirmed while provisioning throwaway spike containers, not specific to `auth login` | Always pair `agents::install_packages` with `agents::run_postinstall_npm` before any command that invokes the agent binary, matching `install_agent`'s existing ordering in `src-tauri/src/lib.rs` |
| `RuntimeManager::exec_in_container` cannot be used to implement capture/injection of the archive | It does `.output()` with `String::from_utf8_lossy` (`src-tauri/src/runtime/mod.rs:270`) — no piped stdin, and binary gzip bytes get corrupted by the lossy UTF-8 conversion | Implement `credentials::tar_capture`/`tar_inject` with a direct `tokio::process::Command` spawn, raw `Vec<u8>` stdout/piped stdin, the same pattern `agents::start_auth` already uses for the login process |
| Local shell testing of `tar -C / ...` against a container mangles the `/` argument | Git-Bash/MSYS automatically rewrites POSIX-looking arguments like `/` to a Windows path (`C:/Program Files/Git/`) before invoking a native `podman.exe` child, even though the argument is meant for the container, not the host | Only a Bash-tool/shell artifact — `tokio::process::Command` in the compiled Rust binary passes args through unmodified, so this does not affect the app; when testing manually from Git Bash, set `MSYS2_ARG_CONV_EXCL="*"` first |
