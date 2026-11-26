---
type: dev-plan
title: "Codebase Restructure — Build Plan"
feature: codebase-restructure
---
# Codebase Restructure — Build Plan

Build plan for a structural refactor of Sandbox-mio with no behaviour change: the Rust backend's module dependency graph (`src-tauri/src/`), the frontend component layout (`src/lib/`, `src/routes/`), a typed IPC layer, the oversized backend files, and the root build tool (`build.rs`, `docs_check.rs`). Status: Phases 1 to 4 are implemented; Phase 5 remains; Phase 6 waits on a user decision. Resume at Phase 5.

## How Each Phase Is Run

- One phase per agent pair, run sequentially in one worktree: an implementer agent makes the change, then a separate reviewer agent reviews the uncommitted diff, fixes every real problem, re-runs verification and proposes a commit message. The phase is committed only after the review passes.
- Every phase is a pure refactor: no change to behaviour, IPC shapes, error `kind` discriminants, error Display text, `settings.json` shape or visuals.
- Verification per phase, from the repository root unless noted: `cargo check --all-targets` and `cargo test` in `src-tauri/` (any Rust change); `pnpm check` (any `src/` or `e2e/` TypeScript change); `build.exe docs`; `build.exe e2e light`. On Windows `./build` resolves to the gitignored `build/` directory, so call `build.exe`.
- Follow the Code Comments rules in the root `AGENTS.md`; moved comments travel with their code, and a comment a move makes false is updated in the same phase.
- `docs/features/**` plans of other features are history and are not rewritten for moved paths; `docs/improvement-backlog.md`, `docs/architecture/*.md` and the `AGENTS.md` files are live and are updated.

## Resolved Decisions

1. **Foundation module named `domain`.** App-wide concepts every layer may use (the mock-mode switch, sandbox name validation, workspace paths, `Sandbox`, `ProviderId`, `BackendError`) live in `src-tauri/src/domain/`. A module named `core` was rejected because it collides with Rust's `core` crate.
2. **No compatibility re-exports.** A moved item is imported from its new home at every call site; a re-export would hide the dependency edge the move removes.
3. **Base image belongs to the provider.** `BASE_IMAGE`, `BASE_IMAGE_VERSION` and `base_image()` live in `src-tauri/src/provider/image.rs`, beside the embedded Containerfile build, so `provider` does not depend on `backends`.
4. **Subsystem dependency rule.** Subsystems depend only on `domain`, `process`, `paths`, `env_ext`, `mio_files`, and on `provider` where they drive the engine; the remaining documented exceptions are listed in `src-tauri/AGENTS.md`. A conversion only a service needs is a `map_err` in that service, not a `From` impl in a subsystem.
5. **Frontend placement rule.** A component lives in the `src/lib/` folder of what it manages, not beside whoever mounts it; a component mounted only by one route stays beside that route's `+page.svelte`. Nothing under `src/lib/` imports from `src/routes/`.

## Implemented: Phase 1 — Break Backend Import Cycles

Commit `8849b28`. Removed the `backends <-> provider`, `backends <-> process`, `process <-> sync` and `settings <-> inference` cycles.

- New `src-tauri/src/domain/` (`mod.rs`, `types.rs`) holds `mock_backend_enabled()`, `validate_name()`, `validate_new_name()`, `workspace_root()`, `workspace_path()`, `PROXY_CONTAINER_SUFFIX`, `Sandbox`, `ProviderId` and `BackendError`.
- `src-tauri/src/process/podman.rs` and `src-tauri/src/process/mutagen.rs` hold binary location (`podman_command()`, `connection_name()`, `DEFAULT_MACHINE_NAME`, the Mutagen sidecar command and `data_dir()`); `process` now depends only on `paths`.
- `src-tauri/src/ssh/identity.rs` holds the sandbox SSH identity, alias, proxy-command and known-hosts code; `backends::seed_sandbox_host_keys()` keeps only the container exec.
- `src-tauri/src/backends/hosts.rs` holds remote-host management; `src-tauri/src/backends/image.rs` holds the dismissed-image-version marker.
- Pane teardown moved from `src-tauri/src/workspace/mod.rs` to `src-tauri/src/services/panes.rs`; `workspace` depends on nothing.
- `ensure_auth_container` moved to `src-tauri/src/services/auth_container.rs`; `EngineKind` moved to `src-tauri/src/settings/types.rs`; `json_path_get` moved to `src-tauri/src/mio_files.rs`.
- The dependency rule and its exception list are documented in `src-tauri/AGENTS.md`.

## Implemented: Phase 2 — Move Dashboard Components Out of `src/routes/projects/`

Commit `60d6b0f`. `src/lib/workspace/` no longer imports the Manage dashboard's panels from `src/routes/projects/`.

- `AgentManager.svelte` moved to `src/lib/agents/`; `PackageManager.svelte` and `packages.svelte.ts` to new `src/lib/packages/`; `PortsManager.svelte` and `SshAccessCard.svelte` to `src/lib/network/`; `ProxyPanel.svelte` and `TrafficPanel.svelte` to `src/lib/proxy/`; `NetPolicyEditor.svelte`, `NetPolicyProposalDialog.svelte` and `net-policy.ts` to new `src/lib/net-policy/`; `RemoteManager.svelte` to new `src/lib/remote/`; `StatusRow.svelte` to `src/lib/sandbox/`.
- `SandboxItem.svelte` and `CreateSandboxItem.svelte` stay in `src/routes/projects/`, because only the Home page mounts them.
- `src/lib/registry.svelte.ts` (the mise registry cache) stays at the `src/lib/` root because the `AGENTS.md` files cite it as the reference store pattern; moving it into `src/lib/packages/` is optional later work.
- The root `AGENTS.md` lost its stale `onboarding-story/` row, and its mirrored-values comment example now cites `DEFAULT_LLAMA_CTX_SIZE` (`src-tauri/src/inference/llama.rs` ↔ `src/lib/agents/pi.ts`).

## Implemented: Phase 3 — Typed IPC Layer `src/lib/api/`

Replaces every `invoke()` call in `src/` with a typed wrapper. 96 of the 104 registered commands are wrapped, in 17 modules, one per command domain file in `src-tauri/src/commands/`; 154 call sites in 47 files were migrated.

- `src/lib/api/` holds `sandbox`, `deploy`, `panes`, `agent-chat`, `terminal`, `inference`, `machine`, `agents`, `image`, `credentials`, `packages`, `hosts`, `ssh`, `app`, `network`, `proxy` and `net-policy`. Each wrapper passes the command's argument names through unchanged and only rethrows; `parseError()` stays at the call sites that branch on `kind` or show `message`.
- Not wrapped because the frontend never calls them: the three `mockctl` commands (the e2e harness reaches them through its own browser-side `invoke`), `add_sandbox_host`, `prepare_sandbox_host`, `provision_sandbox_host`, `switch_app` and `terminal_detach`. A new caller adds its wrapper then.
- Result and argument types that already lived in a feature's `types.ts` or store are imported by the wrapper module, not copied. Types that were declared inside a component or page moved into the wrapper module: `CredentialManifest` and `GithubSandboxStatus` (with its outcome type) to `api/credentials.ts`, `TerminalEvent` to `api/terminal.ts`, `SshAccess` to `api/ssh.ts`. `WorkspaceState` is now exported from `src/lib/workspace/state.svelte.ts`.
- `AppSettings` is one full type in `api/app.ts`, mirroring the Rust struct; the three partial local copies (settings page, onboarding store, learning store) and the untyped `get_settings` reads are gone.
- Dynamic command names became functions: `AppDescriptor.command` (a command string) is now `open`, a wrapper from `api/panes.ts`; the terminal view's input and resize sends are queued as thunks; the sync toggle picks `enableSync` or `disableSync`.
- `src/lib/ssh.ts` and the thin `exposePort`/`unexposePort` and `providerCapabilities` pass-throughs were deleted in favour of `api/ssh.ts`, `api/network.ts` and `api/image.ts`. Where a wrapper's name collides with an existing store function (`startSandbox`, `closePane`, `downloadModel`, `installPackage`), the call site imports the wrapper under a `...Command` alias.
- Event listeners (`listen`) stay in their stores; only `listen_events` and the channel-carrying commands are wrapped, with `Channel<T>` parameters typed per command.
- Verify the done-when with `grep -rnE "invoke(<|\()" src --include=*.ts --include=*.svelte`, which must match only `src/lib/api/` and the comment in `src/lib/errors.ts`. The earlier form of the grep missed the generic `invoke<T>(` calls.
- Out of scope, still a user decision: generated bindings. tauri-specta (still a release candidate for Tauri 2) could later generate `src/lib/api/` from the Rust side and end the hand-mirroring and the `AppErrorKind` drift.
- `src/AGENTS.md` gained an IPC section, and the Error Handling section of the root `AGENTS.md` and the "Adding a Tauri Command" steps in `src-tauri/AGENTS.md` name the wrapper step.

## Implemented: Phase 4 — Split Oversized Backend Files

Splits the oversized backend files along the seams they already had. No file under `src-tauri/src/` other than tests exceeds 700 lines; the largest remaining files are `provider/mod.rs` (781 lines, 600 of production code, the rest its unit tests), `backends/mod.rs` (671), `workspace/mod.rs` (632 with its tests), `machine/mod.rs` (599) and `services/sandbox.rs` (588).

- `net_policy.rs` is now `src-tauri/src/net_policy/`: `mod.rs` (`NetPolicyError` and the shared test fixtures), `schema.rs` (the document types, file access, `starter()`, `placeholder()`, `to_egress()`), `validate.rs` (`validate()`, `stampable_vars()`), `diff.rs` (`diff()`, `PolicyChange`) and `applied.rs` (`AppliedPolicy` and the accepted-policy store). Tests moved with their code. The module no longer re-exports the `provider::types` vocabulary (`EgressMode` and the others): callers import it from `provider::types`.
- `process/mod.rs` keeps the detached-command constructors, the call-shape helpers (`is_monitor_call()`, `strip_connection()`) and `run_via_blocking_process()`. `process/windows.rs` holds the handle sanitiser and all Win32 FFI, including the process-handle duplication the timeout uses. `SystemPodmanRunner` with its timeouts, breaker and machine-list cache moved to `process/podman.rs`, and `SystemMutagenRunner` to `process/mutagen.rs`; their struct declarations left `process/types.rs`, which now holds only the `ProcessRunner` trait.
- `proxy/` is split into `store.rs`, `sidecar.rs` (create, remove, start, reload, sweep, status), `apply.rs` (`apply_policy()`, `apply_accepted()` and the locked local and remote variants), `secret_files.rs`, `remote_push.rs`, `github.rs` (`github_binding()`, `refresh_github_binding()`, `github_binding_live()`, `deploy_secret_count()`), `migrate.rs` and a slimmed `iron.rs` (image pin, CA, config writing). `proxy/mod.rs` keeps the naming helpers, `proxy_env()`, `inject_ca()` and `is_sidecar()`. The `pub use iron::ensure_sidecar_running` re-export is gone; every caller names `proxy::sidecar`.
- `ProxyStore { root }` (`proxy/store.rs`) owns the layout under `<app data root>/proxy/` and is registered in Tauri state in `bootstrap.rs`; the on-disk layout is unchanged and a unit test pins it. An unresolvable app data root is kept as an error inside the store and fails each call that needs it, as the per-call path derivation did before; it never aborts startup. Functions that only derived paths take `&ProxyStore`. `&AppHandle` stays in `ensure_proxy_image()` (activity registry), the flow poller (events), and the apply pipeline, GitHub binding functions and legacy migration, which read the accepted policy (`net_policy::applied`) or the credential store through it. The service layer takes `&ProxyStore` as an explicit parameter; `github_access` reads it from Tauri state, because it already derives every other handle that way.
- The accepted-policy store did not get the same pattern: its five functions are called from `backends`, `commands`, `proxy` and `services`, and a second state type would have grown every apply signature, so it did not stay small. It keeps `&AppHandle`.
- `services/resources.rs` is now `src-tauri/src/resources/` with `monitor.rs`, `rollup.rs` and `history.rs`. To avoid new `resources -> proxy` and `resources -> credentials` edges, `ResourceMonitor::start()` takes a `SandboxFilter`, which bootstrap supplies as `services::sandbox::is_user_sandbox()` (also used by `list_user()`), and `resources::history::history()` returns `BackendError` instead of `AppError`.
- `ProxyAttachment` moved to `provider/types.rs`, which removed the `backends -> proxy` edge and its entry in the exception list of `src-tauri/AGENTS.md`.
- The dependency-graph check shows no new subsystem edge; `resources` depends on `domain`, `paths`, `process` and `provider` only.
- Touched outside Rust: the Rust paths cited by comments in `src/lib/` (`net-policy.ts`, `NetPolicyEditor.svelte`, `health.ts`, `resources.svelte.ts`, `usage.ts`), `src-tauri/AGENTS.md`, `docs/architecture/network.md` and `docs/improvement-backlog.md`.

## Remaining: Phase 5 — Build Tool as an `xtask` Crate

- **Problem:** the repository-root build tool is two files compiled with plain `rustc` (`build.rs`, 1,312 lines; `docs_check.rs`, 1,257 lines), so it gets no `cargo test`, clippy or rust-analyzer support, and the root `build.rs` reads as a Cargo build script beside the gitignored `/build` Vite output directory.
- **Scope:** a std-only Cargo package (for example `xtask/`) with modules for bundled binaries (fetch and stage), `dev`, `e2e`, `ci`, worktree setup, `win_job` and the docs check. Keep the CLI surface of `build.exe` identical: every subcommand in the usage text printed by `main()` in `build.rs` (`dev`, `e2e`, `ci`, `package`, `docs` and their flags).
- **Bootstrap references to update:** `README.md` (the `rustc build.rs` lines), `.github/workflows/ci.yml` and `.github/workflows/release.yml` (`rustc build.rs -o build.exe`), the root `AGENTS.md`, the header comment of `docs_check.rs` (`rustc --test build.rs`), and `CITATION_ROOTS` in `docs_check.rs`, which lists `build.rs`.
- **Decision needed before starting:** how `build.exe` is produced after the move (a `cargo build` step that copies the binary to the root, or a Cargo alias such as `cargo xtask`). Ask the user.
- **Done when:** `build.exe ci` passes, the docs-check unit tests run under `cargo test`, and the CI workflow builds the tool the new way.

## Decision Needed: Phase 6 — Cargo Workspace for the Backend

- Separate crates are the only way to make the compiler reject subsystem dependency cycles. Candidates are the pieces without a Tauri dependency: `domain`, `process`, and the pure parts of `net_policy` (schema, validation, diff).
- Start only after Phases 1 and 4, and only if the user confirms; splitting is not worth it per subsystem.

## Known Leftovers

- `src-tauri/src/runtime/paseo_plugins.rs` carries an unused `use super::*` warning predating this feature.
- `build.exe docs` warns that `docs/features/tailscale-access/02_dev.md` cites a missing `03_e2e.md`; the warning predates this feature.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A backslash literal such as `'\\'` in a Rust file becomes `'\'` and fails to compile after an edit | Bash heredocs and Python-in-heredoc in this Windows environment collapse doubled backslashes | Edit Rust files with the Edit tool, not with heredocs |
| `./build docs` or `./build e2e light` fails with a "Is a directory" error | `./build` resolves to the gitignored Vite output directory `build/` on Windows Git Bash | Call `build.exe docs` or `build.exe e2e light` from the repository root |
| `build.exe e2e light --spec <file>` is rejected | `run_e2e_suite()` in `build.rs` takes only a tier | Rerun one spec from `e2e/` with `pnpm exec wdio run wdio.conf.ts --spec ./test/specs/light/<name>.spec.ts`, setting the environment `run_e2e_suite()` sets |
| `models-page-caching.spec.ts` fails one test on a 1-second warm-cache budget during a full light run | Twenty-five light specs run in parallel and slow the warm `/models` visit | Rerun `models-page-caching.spec.ts` alone; treat a failure only in the parallel run as a timing flake |
| Some light-run app instances stay on `about:blank` | Backlog task T7 in `docs/improvement-backlog.md`, unrelated to this feature | Rerun the failing spec alone before calling the failure a regression |
| A moved function's old path is still cited in a comment, an `AGENTS.md` or `e2e/lib/podman-machine.ts` | Comments and regex parsers cite file paths, which a move does not update | Grep the repository (excluding `node_modules` and history plans under `docs/features/`) for the old path after every move |
| A new dependency edge between subsystems appears after a split | A helper was placed in the subsystem that called it first | Re-run the dependency-graph check documented in `src-tauri/AGENTS.md` and compare it with the exception list there |
