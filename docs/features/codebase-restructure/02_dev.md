---
type: dev-plan
title: "Codebase Restructure — Build Plan"
feature: codebase-restructure
---
# Codebase Restructure — Build Plan

Build plan for a structural refactor of Sandbox-mio with no behaviour change: the Rust backend's module dependency graph (`src-tauri/src/`), the frontend component layout (`src/lib/`, `src/routes/`), a typed IPC layer, the oversized backend files, and the root build tool (`build.rs`, `docs_check.rs`). Status: Phases 1 and 2 are implemented; Phases 3 to 5 remain; Phase 6 waits on a user decision. Resume at Phase 3.

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

## Remaining: Phase 3 — Typed IPC Layer `src/lib/api/`

- **Problem:** the frontend calls 104 registered commands (`tauri::generate_handler!` in `src-tauri/src/commands/mod.rs`) by string through `invoke()` from about 47 files, 18 of them `.svelte` components. The same command is invoked from several places (`get_settings` from 8 files, `inference_status` from 7, `set_settings` and `destroy_sandbox` from 5). About 139 exported TypeScript types under `src/lib/` mirror Rust types by hand.
- **Scope:** add `src/lib/api/` with one module per command domain file in `src-tauri/src/commands/` (`sandbox`, `deploy`, `panes`, `agent_chat`, `inference`, `machine`, `agents`, `image`, `credentials`, `packages`, `hosts`, `ssh`, `app`, `network`, `proxy`, `net_policy`, `terminal`, `mockctl`). Each module exports typed async functions wrapping `invoke()`, with argument and result types next to them. Replace every `invoke()` call in `src/` with an import from `src/lib/api/`. Keep `parseError()` (`src/lib/errors.ts`) at the call sites that branch on `kind` or display `message`; the wrappers only rethrow.
- **Out of scope:** generated bindings. tauri-specta (still a release candidate for Tauri 2) could later generate `src/lib/api/` types from the Rust side and end the hand-mirroring and the `AppErrorKind` drift; adopting tauri-specta is a user decision, not part of Phase 3.
- **Update:** `src/AGENTS.md` (a new "IPC" section stating that components never call `invoke()` directly), the root `AGENTS.md` Error Handling section and the "Adding a Tauri Command" steps in `src-tauri/AGENTS.md` (add the wrapper to `src/lib/api/`).
- **Done when:** `grep -rn "invoke(" src --include=*.ts --include=*.svelte` matches only `src/lib/api/` and `src/lib/errors.ts`; `pnpm check` and `build.exe e2e light` pass.

## Remaining: Phase 4 — Split Oversized Backend Files

Split along seams the files already have; no behaviour change. Line counts as of commit `8849b28`.

- **`src-tauri/src/net_policy.rs` (1,078 lines)** becomes `src-tauri/src/net_policy/` with `schema.rs` (the `.mio/net-policy.json` types and loading), `validate.rs`, `diff.rs` (`diff()`, `PolicyChange`) and `applied.rs` (the accepted-policy store: `write_applied()`, `read_applied()`, `read_accepted()`, `remove_applied()`). Tests move with their code.
- **`src-tauri/src/process/mod.rs` (940 lines)** keeps the public surface and moves the Windows handle-inheritance code and FFI (`clear_inheritable_handles()`, `system_handle_snapshot()`) to `process/windows.rs`, and `SystemPodmanRunner` with its timeouts, half-open breaker and machine-list cache into `process/podman.rs` (created in Phase 1 for the command builders). `SystemMutagenRunner` joins `process/mutagen.rs`.
- **`src-tauri/src/proxy/iron.rs` (1,050 lines) and `src-tauri/src/proxy/mod.rs` (815 lines)** split into secret-file staging, remote push scripts, the apply pipeline (`apply_policy()`, `apply_accepted()` and the locked local/remote variants), the GitHub binding (`refresh_github_binding()`, `github_binding_live()`), the legacy-secret migration (`migrate_legacy_secrets()`), and sidecar lifecycle.
- **`ProxyStore`:** proxy functions take `&AppHandle` mainly to derive per-sandbox state paths (35 `&AppHandle` parameters in `proxy/mod.rs`, 12 in `proxy/iron.rs`). Introduce a `ProxyStore { root: PathBuf }` type in `src-tauri/src/proxy/` owning the state-directory layout (meta, secrets, CA, remote state) and pass it instead; register it in Tauri state in `src-tauri/src/bootstrap.rs`. Apply the same pattern to the accepted-policy store from `net_policy/applied.rs` (5 `&AppHandle` parameters) if it stays small.
- **`src-tauri/src/services/resources.rs` (686 lines)** is a background monitor with its own state, ticker and persistence, so it is a subsystem: move it to `src-tauri/src/resources/` with `monitor.rs` (`ResourceMonitor`), `rollup.rs` (`MinuteAcc`, pure) and `history.rs` (JSONL append, prune and read). Update `src-tauri/AGENTS.md`'s module inventory.
- **Optional:** move `ProxyAttachment` (`src-tauri/src/proxy/types.rs`) into `src-tauri/src/provider/types.rs` to drop the last `backends -> proxy` edge, then update the exception list in `src-tauri/AGENTS.md`.
- **Done when:** no file under `src-tauri/src/` other than tests exceeds about 700 lines; `cargo check --all-targets`, `cargo test` and `build.exe e2e light` pass; run `build.exe e2e` (`standard`) once, because the phase touches `proxy/` and `process/` (see the Common Gotchas in `docs/improvement-backlog.md`).

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
