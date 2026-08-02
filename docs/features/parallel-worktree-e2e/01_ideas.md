# Parallel worktree e2e runs — ideation

This document scopes running the e2e `light` tier concurrently from several git worktrees of this
repository on one Windows machine, so each worktree gets its own fast feedback loop without
corrupting another worktree's run or the developer's installed app. The target platform is Windows
only; no cross-platform or headless work is in scope (the headless surface investigated first is
deferred in `docs/features/parallel-worktree-e2e/04_improvements.md`). Every collision lives in
`e2e/wdio.conf.ts` (the launcher: ports, workspace root, app-data cleanup, process killing, teardown)
except app-data location, which is decided by five `AppHandle::path()` call sites in `src-tauri/src/`.
Asset staging lives in `build.rs`.

The scope is the `light` tier specifically. Concurrent `standard`/`heavy` runs need podman connection
and Mutagen daemon isolation as well, which is a materially larger problem and is deferred in
`04_improvements.md`.

## Current State

A `light` run is `./build e2e light` → `pnpm run test:light` in `e2e/` → `wdio run wdio.conf.ts
--suite light`. `onPrepare` in `e2e/wdio.conf.ts` skips podman provisioning for a light run
(`IS_LIGHT_RUN`, derived by `argValues()` from the CLI flags) but still runs
`pnpm tauri build --debug --no-bundle`, then `tauri-driver` launches
`src-tauri/target/debug/sandbox-mio.exe` with `MIO_MOCK_BACKEND=1` set in `beforeSession`.

Nothing about the tier is worktree-aware. Seven host-global resources are shared, verified by reading
`e2e/wdio.conf.ts` and the Rust call sites on 2026-07-26:

- **WebDriver ports.** `config.port` is the literal `4444`, and `tauri-driver` defaults to the same
  port for itself plus `4445` for the native driver (`tauri-driver --help` lists `--port` and
  `--native-port`). A second concurrent run fails to bind.
- **Process killing.** `killStaleDevProcesses()` kills every `podman`, `gvproxy`, `win-sshproxy`,
  `sandbox-mio`, and `mutagen` process whose `Path` matches `*\target\debug\*`, host-wide, and runs in
  `onPrepare` before the build. A light run in worktree A therefore kills worktree B's running app
  and any podman/gvproxy process B staged into its own `target/debug`.
- **Workspace root.** `WORKSPACE_ROOT` is the fixed `~/.sandbox-mio-e2e`, exported as
  `SANDBOX_WORKSPACE_ROOT`, and deleted with `fs.rmSync(..., { recursive: true })` in both
  `onPrepare` and `onComplete` — so a starting run destroys a running run's sandbox directories.
- **Persisted settings.** `onPrepare` deletes `settings.json` under
  `%APPDATA%/com.tincho.sandbox-mio`, which every worktree and the developer's installed app share.
  The light tier genuinely writes the file: `e2e/test/specs/light/local-llm-ui.spec.ts` round-trips
  `set_settings`/`get_settings` to prove engine persistence.
- **Downloaded models.** `onComplete` deletes the test model's subdirectory under
  `%APPDATA%/com.tincho.sandbox-mio/models`, shared the same way.
- **`~/.ssh`.** `onComplete` removes `~/.ssh/sandbox-mio/config` and strips the
  `Include sandbox-mio/config` line from `~/.ssh/config` unconditionally, including on a light run
  that never wrote either — so a finishing light run deletes a concurrent standard-tier run's SSH
  aliases.
- **Application log.** `init_logging()` in `src-tauri/src/lib.rs` writes a daily-rolling `mio.log`
  under `app_log_dir()`, shared across worktrees, so two runs interleave into one file.

Two further facts shape the design:

- **Worktrees are nested inside the primary worktree.** `git worktree list` reports
  `C:/Users/krist/prog/mi-sandbox/.claude/worktrees/tool-ssh` — a child path of the primary worktree
  root. Any fix that scopes a filter to "paths under the current worktree root" therefore still
  matches the nested worktree's paths when run from the primary worktree.
- **A fresh worktree cannot run the light tier at all today.** `.gitignore` excludes
  `tools/msedgedriver.exe`, `tools/mutagen.exe`, `tools/mutagen-agents.tar.gz`, and all of
  `/src-tauri/binaries/`. The existing `tool-ssh` worktree has `node_modules` installed but no
  `tools/` directory, so `tauri-driver` has no native driver to launch, and `onPrepare`'s
  `mutagen-agents.tar.gz` copy has no source file.

Cargo target directories are already per-worktree (`src-tauri/target`), so two builds do not contend
on a cargo lock. `e2e/failures.log` and `e2e/.diag` (`e2e/lib/diag.ts`) are worktree-relative and need
no change.

## Proposed model: a per-worktree run identity

Introduce one module, `e2e/lib/run-identity.ts`, that computes a stable **run slug** once and exports
every per-run value derived from it. Every collision above is then keyed off the same slug rather than
fixed six ways in `e2e/wdio.conf.ts`.

The slug is derived from the absolute worktree root path (`path.resolve(__dirname, "..")` from `e2e/`)
as a short hash, combined with the directory basename for readability — for example
`mi-sandbox-3f9c1a`. Deriving from the path rather than the branch name matters: the slug must be
stable across runs in the same worktree so that `onPrepare`'s destructive cleanup only ever deletes
directories belonging to the same worktree, and a branch rename must not orphan a directory.

`e2e/lib/run-identity.ts` exports the slug, the port pair, the workspace root, and the app-data root.
`e2e/wdio.conf.ts` imports the values instead of computing them inline, so a future collision has one
place to be fixed.

## Proposed model: port pair allocation

Each run needs two free ports: one for `tauri-driver` (which `config.port` in `e2e/wdio.conf.ts` must
match) and one for the native driver (`tauri-driver --native-port`, passed alongside the existing
`--native-driver` argument in `beforeSession`).

Allocation is a **disjoint per-worktree base range plus a liveness probe inside that range**. The slug
hash maps to a base port in a reserved band (for example 45000–46000, stepped by 16 so each worktree
owns a 16-port window), and the launcher probes upward within its own window for the first free pair.
Two worktrees never probe the same numbers, so the classic probe-then-bind race cannot happen between
worktrees; probing within the window still handles a re-run started while the previous run's ports
linger in `TIME_WAIT`.

`e2e/wdio.conf.ts` is an ESM module loaded by `tsx`, so the probe can run at module scope with
top-level `await` and have its result assigned to `config.port` before WebdriverIO reads the config.

## Proposed model: worktree-scoped process killing

`killStaleDevProcesses()` in `e2e/wdio.conf.ts` must match only the invoking worktree's own build
output. The filter changes from the substring `*\target\debug\*` to the current worktree's absolute
`src-tauri\target\debug\` path.

The match must be against that full absolute path, not a prefix of the worktree root: because
`.claude/worktrees/tool-ssh` sits inside the primary worktree, a root-prefix filter run from the
primary worktree still matches the nested worktree's processes. Matching on
`<worktree>\src-tauri\target\debug\` is exact enough to exclude the nested worktree, whose own path
contains `.claude\worktrees\tool-ssh\src-tauri\target\debug\`.

## Proposed model: app-data isolation via one env override

Tauri resolves `app_data_dir()` and `app_config_dir()` through `dirs::data_dir()` /
`dirs::config_dir()`, which on Windows call `SHGetKnownFolderPath(FOLDERID_RoamingAppData)` — the
known-folder API, **not** the `APPDATA` environment variable (confirmed in the vendored `dirs-6.0.0`
`src/win.rs`). Overriding `APPDATA` in the app's spawn environment therefore cannot relocate the
store, and the bundle identifier that forms the final path segment is compiled in from
`src-tauri/tauri.conf.json` via `generate_context!()`.

The proposal is a new `src-tauri/src/paths.rs` exposing `app_data_root(app)` and `app_config_root(app)`
that return the `MIO_APP_DATA_DIR` environment variable when set and fall back to the current
`AppHandle::path()` calls otherwise, read at call time and never cached, matching the env-steering
precedent documented in the Environment Variables section of `AGENTS.md`. Five call sites redirect
through the new module:

- `settings_path()` in `src-tauri/src/settings/mod.rs` (`app_config_dir` → `settings.json`)
- `models_root()` in `src-tauri/src/inference/mod.rs` (`app_data_dir/models`)
- `store_dir()` in `src-tauri/src/credentials/mod.rs` (`app_data_dir/credentials`)
- `data_dir()` in `src-tauri/src/sync/mod.rs` (`app_data_dir/mutagen`, exported to the sidecar as
  `MUTAGEN_DATA_DIRECTORY`)
- `init_logging()` in `src-tauri/src/lib.rs` (`app_log_dir`)

`e2e/wdio.conf.ts` then sets `MIO_APP_DATA_DIR` to a slug-keyed directory in the app's spawn
environment, and its `settings.json` and model cleanups target the same directory instead of
`%APPDATA%/com.tincho.sandbox-mio`. A side effect worth having: a light run stops deleting the
developer's real installed-app settings, which it does today.

Only `settings/` and `inference/` are on a light spec's path, but redirecting all five keeps the rule
"the app writes nothing outside `MIO_APP_DATA_DIR` when the variable is set" simple enough to state and
check, and gives the deferred standard-tier work (`04_improvements.md`) the Mutagen data directory it
will need.

## Proposed model: workspace root and tier-scoped teardown

`WORKSPACE_ROOT` in `e2e/wdio.conf.ts` becomes `~/.sandbox-mio-e2e-<slug>`. The directory stays under
`$HOME` because the existing comment records a hard constraint: the podman machine only auto-mounts
the user profile into its VM, which the standard tier's bind mount needs. A stable slug keeps the
`onPrepare` delete self-healing — the same worktree always reclaims its own leftovers from a crashed
run, and no unbounded set of directories accumulates.

The `~/.ssh` cleanup block in `onComplete` moves behind `!IS_LIGHT_RUN`, matching the remote-machine
teardown immediately below it. A light run writes neither file, so the block is pure collateral damage
today.

## Proposed model: worktree asset staging

A fresh worktree needs `tools/msedgedriver.exe` and `src-tauri/binaries/` before `pnpm tauri build`
and `tauri-driver` can run. The assets are version-pinned by `MUTAGEN_VERSION`, `LLAMA_VERSION`, and
`PODMAN_VERSION` constants in `build.rs`, so every worktree on the same commit wants byte-identical
files.

The proposal is a `build dev --setup-worktree` arm in `build.rs` that hard-links (falling back to
copy) the four `tools/` files and `src-tauri/binaries/` from a sibling worktree that already has them,
resolved via `git worktree list`, and only falls back to `fetch_mutagen`/`fetch_llama`/`fetch_podman`
when no sibling has the pinned version staged. Hard-linking costs no disk for the large `llama` and
`podman` payloads and cannot drift, because the pinned constants are identical across worktrees on the
same commit.

Which assets a light run strictly needs is an open question below — `binaries/llama` is optional
(`onPrepare` warns and continues) and the podman resource directory is skipped entirely under
`MIO_MOCK_BACKEND=1` (`src-tauri/src/lib.rs` guards `set_podman_resource_dir` on
`!mock_backend_enabled()`), but `bundle.externalBin` in `src-tauri/tauri.conf.json` lists
`binaries/mutagen`, which the Tauri CLI may validate even with `--no-bundle`.

## Technology choices with tradeoffs

### Run identity: worktree-path hash

**Chosen:** a stable slug hashed from the absolute worktree root path.

- **Rejected — process id or timestamp per run.** Guarantees uniqueness with no hashing, but makes
  every run's workspace root and app-data root a new directory, so `onPrepare` can no longer safely
  delete its predecessor and a crashed run leaks a directory forever. Cleanup would need its own
  age-based sweep.
- **Rejected — the git branch name.** Human-readable and already unique per worktree, but a branch
  rename or a checkout inside a worktree silently orphans the previous run's directories, and branch
  names contain `/` and other characters that need escaping before use in a path or port derivation.
- **Rejected — a developer-set environment variable per worktree** (for example `MIO_RUN_SLUG=a`).
  Zero magic and trivially debuggable, but it must be set correctly in every shell and every agent
  session that runs the suite, and forgetting it silently restores today's collisions rather than
  failing loudly.

### Ports: disjoint per-worktree band with in-band probing

**Chosen:** the slug hash selects a 16-port window in a reserved band; the launcher probes within its
own window.

- **Rejected — ephemeral ports (bind port 0).** Cannot collide at all, but neither `config.port` in
  `e2e/wdio.conf.ts` nor `tauri-driver --port` can accept a kernel-assigned port: both need the number
  up front, so the launcher would have to bind, read the port, close, and hand the number over —
  reintroducing exactly the probe-then-bind race that a disjoint band eliminates.
- **Rejected — a fixed offset from a shared base with no probing.** Simplest possible scheme, but a
  re-run started while the previous run's `tauri-driver` is still shutting down fails on `TIME_WAIT`
  rather than stepping to the next port.
- **Rejected — a lock file that serialises light runs across worktrees.** Directly contradicts the
  goal of concurrent runs.

### App data: `MIO_APP_DATA_DIR` read in Rust

**Chosen:** a new `src-tauri/src/paths.rs` env-override layer over the five `AppHandle::path()` call
sites.

- **Rejected — overriding `APPDATA` in the app's spawn environment.** The intuitive fix, and the one
  a previous draft of this design assumed. It does not work: `dirs-6.0.0`'s `data_dir()` on Windows is
  `known_folder_roaming_app_data()`, which queries the known-folder API and ignores the environment
  variable.
- **Rejected — a per-worktree bundle identifier.** Changing `identifier` in
  `src-tauri/tauri.conf.json` per worktree does relocate the store, since the identifier is the final
  path segment. Rejected because the identifier is compiled in by `generate_context!()`, so the change
  means either a dirty working tree in every worktree or a generated config file, and it silently
  changes the artefact's identity for updater and installer purposes.
- **Rejected — accepting the shared store and serialising the specs that touch it.** No Rust change at
  all, but `e2e/test/specs/light/local-llm-ui.spec.ts` writes settings through `set_settings`, so
  concurrent runs would need a cross-worktree lock around part of the suite, and light runs would keep
  deleting the developer's installed-app settings.

### Asset staging: hard-link from a sibling worktree

**Chosen:** a `build.rs` arm that links `tools/` and `src-tauri/binaries/` from a sibling worktree,
falling back to the existing `fetch_*` functions.

- **Rejected — re-download per worktree** (calling the existing `fetch_*` functions directly). Already
  implemented and correct, but re-downloads the pinned llama.cpp and podman payloads per worktree, and
  the `fetch_*` functions stage through fixed `env::temp_dir()` paths that `fs::remove_dir_all` first,
  so two concurrent setups corrupt each other's extraction.
- **Rejected — un-ignore the assets and commit them.** Removes the problem entirely, at the cost of
  large binaries in git history; the pinned-version constants in `build.rs` exist precisely to avoid
  that.
- **Rejected — one shared cache directory outside every worktree, symlinked in.** Cleaner than
  worktree-to-worktree linking, but symlink creation on Windows needs Developer Mode or elevation,
  whereas hard links to files on the same volume need neither.

## Validation

Every claim in Current State was confirmed by reading code and running read-only commands on
2026-07-26:

- `git worktree list` reports a worktree nested at
  `C:/Users/krist/prog/mi-sandbox/.claude/worktrees/tool-ssh`, whose `tools/` directory does not
  exist while its `node_modules` does.
- `tauri-driver --help` lists `--port`, `--native-port`, `--native-host`, and `--native-driver`.
- `dirs-6.0.0`'s `src/win.rs` maps both `data_dir()` and `config_dir()` to
  `dirs_sys::known_folder_roaming_app_data()`; `tauri-2.11.2`'s `src/path/desktop.rs` builds
  `app_data_dir()`/`app_config_dir()`/`app_log_dir()` from those functions plus the compiled-in
  identifier.
- The five app-data call sites are `settings/mod.rs:14`, `inference/mod.rs:30`,
  `credentials/mod.rs:24`, `sync/mod.rs:21`, and `lib.rs:1277`.
- `e2e/test/specs/light/local-llm-ui.spec.ts` calls `set_settings`, so the light tier writes the
  shared settings file.
- `MutagenCli::shutdown()` (`src-tauri/src/sync/mod.rs`) issues `daemon stop` through its
  `ProcessRunner`, so a light run's exit does **not** stop a real Mutagen daemon — one cross-tier
  hazard that does not exist.

A 3-step spike was run on 2026-08-02, in a fresh worktree plus a second worktree created for the
purpose (the `tool-ssh` worktree referenced above no longer exists), with hand-edited, uncommitted
overrides in `e2e/wdio.conf.ts` standing in for the not-yet-built `e2e/lib/run-identity.ts`:

1. Staged `tools/` and `src-tauri/binaries/` by hard link into a fresh worktree and ran
   `pnpm tauri build --debug --no-bundle`. **Succeeded** with only the hard-linked assets. **Confirmed
   surprise:** `bundle.externalBin`'s `binaries/mutagen` entry *is* validated even under `--no-bundle`
   — removing the hard-linked mutagen executable fails the build in `tauri-build`'s own build script,
   before any Rust compilation. This settles Open Question 1 below: a light build needs the same asset
   set `02_dev.md` Phase F stages, not a trimmed subset.
2. Ran `pnpm run test:light` (one fast spec) sequentially in both worktrees with the port pair,
   workspace root, and settings/credentials dirs overridden by hand, and `killStaleDevProcesses()`
   narrowed to each worktree's own `src-tauri\target\debug\` path. Both passed. The overrides were
   honoured for ports, workspace root, and settings/credentials — and, as `02_dev.md`'s "concrete
   model" already documents, **not** honoured for `models_root()` or `init_logging()`, which remain
   unredirected today and still write to the real shared `%APPDATA%` models directory and the real
   Local-AppData `mio.log`. That gap is exactly what `02_dev.md` Phase C exists to close, not a spike
   surprise.
3. Ran both worktrees' light suites concurrently with the same hand-set values. Genuine overlap was
   confirmed from timestamps, not accidental serialization. Both passed: no port collision, no
   cross-worktree process kill, no settings cross-contamination, and the `Blocking waiting for file
   lock on package cache` cargo contention appeared in both builds as expected. One open item surfaced
   rather than resolved: neither run's app produced a fresh write to the real `mio.log` within the
   ~5–9 second spec window, so whether `tracing_appender`'s buffered writer flushes in time for a
   post-start assertion (relevant once Phase C redirects the log under `MIO_APP_DATA_DIR`) is still
   unconfirmed — see `03_e2e.md`'s de-risking section.

No collision beyond the two already-known, already-scoped gaps (models, logs) turned up. The full
report lives in the session that ran it; the findings above are folded into `02_dev.md`'s Resolved
Decisions and `03_e2e.md`'s de-risking section rather than kept as a separate artefact.

## Open Questions

1. **Which staged assets does a light build actually require?** `binaries/llama` is optional by
   `onPrepare`'s own warning path and the podman resource directory is skipped under
   `MIO_MOCK_BACKEND=1`, but `bundle.externalBin`'s `binaries/mutagen` entry may be validated by the
   Tauri CLI even with `--no-bundle`. Spike step 1 answers it, and the answer decides whether
   worktree staging must move the large payloads at all.
2. **Does `onPrepare` still delete and rebuild the app binary on every light run?** The current delete
   exists because `tauri-build` emits no rerun-if-changed for frontend assets, so a stale exe can
   point at `devUrl`. Two concurrent full debug builds saturate the machine, so a light run may want to
   reuse an existing binary; skipping the rebuild reintroduces the stale-exe failure mode the comment
   in `e2e/wdio.conf.ts` documents.
3. **Should the port band be configurable?** A hardcoded band (45000–46000) can collide with an
   unrelated local service, which would surface as a confusing WebDriver connection failure rather
   than a clear message.
4. **Does `IS_LIGHT_RUN` need to become worktree-aware too?** `argValues()` in `e2e/wdio.conf.ts`
   already has a documented gotcha about flag spellings it does not match; the `~/.ssh` teardown gate
   and the podman-provisioning skip both depend on it, and a wrong answer is now destructive across
   worktrees rather than only locally.
5. **How many concurrent light runs must be supported?** A 16-port window per worktree and one
   app-data root per worktree assume one run at a time per worktree. Two simultaneous runs in the
   *same* worktree would share both, which the design does not currently prevent.
6. **Does the shared application log need splitting?** Redirecting `init_logging()` through
   `MIO_APP_DATA_DIR` gives each run its own `mio.log`, which helps diagnosis but means a developer
   debugging a light failure must look under the slug-keyed directory rather than the familiar
   `%APPDATA%` path.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A concurrent light run in a second worktree dies mid-suite with its app process gone | `killStaleDevProcesses()` in `e2e/wdio.conf.ts` kills every `sandbox-mio`/`podman`/`gvproxy`/`win-sshproxy`/`mutagen` process whose path matches `*\target\debug\*`, host-wide, and runs in `onPrepare` before the build | Narrow the PowerShell `Path -like` filter to the invoking worktree's absolute `src-tauri\target\debug\` path |
| Narrowing the process filter to the worktree root still kills the nested worktree's processes | Worktrees are created under `.claude/worktrees/` *inside* the primary worktree, so the nested worktree's paths are prefixed by the primary worktree's root | Match the full `<worktree>\src-tauri\target\debug\` path rather than a prefix of the worktree root |
| Setting `APPDATA` in the app's spawn environment does not relocate `settings.json` or the model store | `dirs-6.0.0`'s `data_dir()`/`config_dir()` on Windows call `known_folder_roaming_app_data()`, the known-folder API, which ignores the `APPDATA` environment variable | Route the five `AppHandle::path()` call sites through a `MIO_APP_DATA_DIR`-aware helper in `src-tauri/src/paths.rs` |
| A light run wipes the developer's installed-app settings | `onPrepare` in `e2e/wdio.conf.ts` deletes `settings.json` under `%APPDATA%/com.tincho.sandbox-mio`, which the debug build and the installed release share | Point both the app (`MIO_APP_DATA_DIR`) and the cleanup at a slug-keyed directory from `e2e/lib/run-identity.ts` |
| A standard-tier run in another worktree loses its `~/.ssh` aliases when an unrelated light run finishes | The `~/.ssh/sandbox-mio/config` removal and `Include` stripping in `onComplete` (`e2e/wdio.conf.ts`) is unconditional, unlike the remote-machine teardown directly below it | Gate the `~/.ssh` cleanup block on `!IS_LIGHT_RUN` |
| A running light run reports missing sandbox workspace directories | `WORKSPACE_ROOT` in `e2e/wdio.conf.ts` is the fixed `~/.sandbox-mio-e2e` and is deleted recursively in `onPrepare`, so a second run starting deletes the first run's directories | Suffix `WORKSPACE_ROOT` with the run slug, keeping the directory under `$HOME` as the existing comment's podman bind-mount constraint requires |
| Two concurrent runs fail with a port-in-use error or WebDriver connection refused | `config.port` in `e2e/wdio.conf.ts` is the literal `4444`, and `tauri-driver` defaults to `4444` for itself plus `4445` for the native driver | Allocate a slug-derived port window in `e2e/lib/run-identity.ts` and pass the pair to both `config.port` and `tauri-driver --port`/`--native-port` |
| `tauri-driver` fails to launch in a fresh worktree, or `onPrepare`'s `mutagen-agents.tar.gz` copy throws ENOENT | `.gitignore` excludes `tools/msedgedriver.exe`, `tools/mutagen*`, and all of `/src-tauri/binaries/`, so a newly created worktree has neither | Stage the assets into the new worktree before the first run (hard-link from a sibling worktree, or fall back to `build.rs`'s `fetch_mutagen`/`fetch_llama`/`fetch_podman`) |
| Two `build`-driven asset fetches run concurrently and corrupt each other's extraction | `fetch_mutagen`/`fetch_llama`/`fetch_podman` in `build.rs` extract through fixed `env::temp_dir()` paths that are `fs::remove_dir_all`'d first, and `%TEMP%` is shared across worktrees | Stage by linking from a sibling worktree instead of re-fetching, or make the temp extraction paths run-unique |
