---
type: dev-plan
title: "Parallel worktree e2e runs — build plan"
feature: parallel-worktree-e2e
---
# Parallel worktree e2e runs — build plan

This document is the build plan for running e2e tiers from several git worktrees of this repository
on one Windows machine. It continues and supersedes
`docs/features/parallel-worktree-e2e/01_ideas.md`, which surveyed the option space; where the two
disagree, this document wins. All seven phases below are implemented; two of the app-data
collisions `01_ideas.md` listed had already been fixed in the working tree by unrelated work, which
changed the shape of Phase C and is recorded under The concrete model.

`01_ideas.md` scoped the work to concurrent `light` runs and deferred everything about
`standard`/`heavy` to `04_improvements.md`. That scope is widened here, for the reason set out in
the next section: the development loop this feature exists to serve ends each feature with a
`standard` run, so a `standard` run must be safe alongside the `light` runs other worktrees are
still driving. Making `standard` runs *concurrent with each other* remains deferred.

## The development loop this serves

A feature is developed in its own worktree and driven almost entirely by `./build e2e light` —
seconds per iteration, no podman, no shared infrastructure beyond the host-global resources this
document removes. When the feature is behaviourally complete and its light specs are green, the
worktree adds whatever `standard` specs the feature needs (`e2e/AGENTS.md`'s decision rules keep
`standard` as the source of regression truth) and runs `./build e2e` once, at the end.

Three requirements fall out of that loop, and they set the scope of every phase below:

**Many concurrent light runs, one per worktree.** The steady state. This is `01_ideas.md`'s original
scope and Phases A–F cover it.

**A standard run must be safe while other worktrees run light — in both directions.** A light run
must not damage a concurrent standard run: today `onComplete` strips `~/.ssh/sandbox-mio/config`
unconditionally, and `killStaleDevProcesses()` kills every `target\debug` podman process host-wide.
A standard run must equally not damage a concurrent light run, through the same process killer and
the shared workspace and app-data roots. This cross-tier direction is **in scope**, where
`01_ideas.md` treated anything touching `standard` as deferred.

**At most one standard run per host.** `mio-tests` and `mio-tests-remote`
(`e2e/lib/podman-connection.ts`) are singular host-global machines whose whole lifecycle `onPrepare`
provisions and `onComplete` stops or removes, and the deploy flow writes one shared `~/.ssh`
alias file. Two standard runs cannot be made safe by keying paths off a slug — they have to be
serialised. Phase A therefore takes a **host-scoped** lock for a non-light run, so the second one
fails immediately with a clear message instead of two runs tearing down each other's machines.
Making standard runs genuinely concurrent stays deferred in `04_improvements.md`.

One further consequence: because the light tier is the loop that runs hundreds of times, the ~180 MB
of asset copying `onPrepare` performs on **every** run stops being a rounding error. Phase F treats
it as part of the feature rather than a nicety.

## The concrete model

One module, `e2e/lib/run-identity.ts`, computes a **run slug** from the absolute worktree root and
exports every per-run value derived from it: the slug itself, the WebDriver port pair, the workspace
root, and the app-data root. `e2e/wdio.conf.ts` imports those values instead of computing them
inline, so each of the host-global resources is keyed off one identity rather than fixed
independently.

The slug is a short hash of `path.resolve(__dirname, "..")` (the worktree root as seen from `e2e/`)
joined to the root's directory basename — for example `mi-sandbox-3f9c1a`. Path-derived rather than
branch-derived, so the slug is stable across runs in one worktree: `onPrepare`'s destructive cleanup
then only ever deletes directories belonging to the invoking worktree, and no unbounded set of
orphaned directories accumulates.

Four facts about the current tree shape the plan.

**`e2e/wdio.conf.ts` is evaluated once per process, not once per run.** `@wdio/runner` constructs its
own `ConfigParser(configFile, args)` in each worker, so the config module's top level executes in the
launcher *and* again in every worker process — with `maxInstances: 1` that is one worker per spec
file, sequentially. Anything with a side effect at module scope therefore happens N+1 times. Two
phases depend on getting this right: the port pair is allocated once in the launcher and published
through the environment (Phase E) rather than probed at module scope, and the run lock is released
only by the launcher (Phase A). Workers inherit `process.env` as it stood when they were forked,
which is after `onPrepare` returned, and carry `WDIO_WORKER_ID` — set by `@wdio/local-runner` and
absent in the launcher — which is the discriminator both phases use.

**App-data isolation is half-built, by a different mechanism.** `settings_path()` in
`src-tauri/src/settings/mod.rs` already honours `MIO_SETTINGS_DIR`, and `store_dir()` in
`src-tauri/src/credentials/mod.rs` already honours `MIO_CREDENTIALS_DIR`, both through
`env_dir()` in `src-tauri/src/env_ext.rs`; `e2e/wdio.conf.ts` sets both to fixed subdirectories of
`~/.sandbox-mio-e2e-appdata`. `01_ideas.md`'s premise that a light run wipes the developer's
installed-app settings is therefore already false. What remains true is that the two directories are
**fixed, not slug-keyed**, so two worktrees still collide on them, and that three call sites are
still unredirected: `models_root()` in `src-tauri/src/inference/mod.rs`, `data_dir()` in
`src-tauri/src/sync/mod.rs`, and `init_logging()` in `src-tauri/src/lib.rs`. Phase C therefore adds
the `MIO_APP_DATA_DIR` root as the *fallback* layer beneath the two existing subsystem variables
rather than replacing them: the specific variable wins when set, the root applies otherwise, and the
compiled-in Tauri path applies last. Keeping the existing variables costs one extra branch in
`src-tauri/src/paths.rs` and preserves two documented behaviours the light tier already depends on.

**Worktrees exist both inside and outside the primary worktree.** `git worktree list` on 2026-08-02
reports `C:/Users/krist/prog/mi-sandbox/.claude/worktrees/tool-ssh` (nested) and
`C:/Users/krist/emdash/worktrees/sandbox-mio/emdash/check-dependencies-iu9t4` (external). The
process filter must therefore match the invoking worktree's full absolute `src-tauri\target\debug\`
path, not a prefix of its root — a root prefix still matches the nested worktree's processes when
the run is invoked from the primary worktree.

**A second host-wide process killer exists.** `kill_stale_dev_processes()` in `build.rs` runs on the
`build dev` arm with the same `*\target\debug\*` filter as `killStaleDevProcesses()` in
`e2e/wdio.conf.ts`. A `build dev` in one worktree kills another worktree's running e2e podman
processes, so Phase B narrows both.

### Verified clear

Two resources that look like collisions and are not, recorded so the analysis is not repeated:

- **The mock control plane.** `src-tauri/src/mockctl/` is reached over Tauri IPC from inside the
  webview (`invokeCmd()` in `e2e/lib/mock.ts`) and holds its director in process state. The light
  tier's newest subsystem opens no host-global port or pipe.
- **The WebView2 user-data folder.** No `dataDirectory` is set in `src-tauri/tauri.conf.json` and
  nothing in `src-tauri/src/` sets `data_directory`, so wry passes an empty user-data folder and
  WebView2 falls back to a directory beside the executable — already per-worktree, since each
  worktree has its own `src-tauri/target/debug`.

## Resolved Decisions

### From `01_ideas.md`'s Open Questions

1. **Which staged assets does a light build actually require?** Answered by the spike
   `01_ideas.md`'s Validation section records: all of them. Removing the hard-linked
   `binaries/mutagen-x86_64-pc-windows-msvc.exe` fails `pnpm tauri build --debug --no-bundle` in
   `tauri-build`'s own build script — `bundle.externalBin` is validated even under `--no-bundle` —
   before any Rust compilation runs. Phase F stages `tools/msedgedriver.exe` plus the whole of
   `src-tauri/binaries/` by hard link accordingly, which costs no disk regardless. What the same phase
   does have to answer is the separate per-run *copy* into `target/debug`, which is where the cost
   actually lands.
2. **Does `onPrepare` still delete and rebuild the app binary on every light run?** Yes, unchanged.
   The stale-exe failure mode the comment in `e2e/wdio.conf.ts` documents is real, and per-worktree
   `src-tauri/target` directories mean two concurrent builds contend for CPU but never for a cargo
   lock. A skip-the-rebuild escape hatch is deferred to `04_improvements.md`.
3. **Should the port band be configurable?** Yes. The band defaults to 45000–45999 and is overridden
   by `MIO_PORT_BASE`; a window with no free pair fails with an explicit message naming the window
   and the variable, rather than surfacing as a WebDriver connection refusal.
4. **Does `IS_LIGHT_RUN` need to become worktree-aware?** No — the tier is a property of the run, not
   of the worktree. Its destructive reach shrinks instead: after Phases C and D the workspace root,
   settings, credentials, models, and logs are all slug-scoped, so a wrong answer damages only the
   invoking worktree. Its *importance* rises, though: `IS_LIGHT_RUN` now gates the `~/.ssh` cleanup
   (Phase D) and the host-scoped standard lock (Phase A), so a wrong answer either strands the lock
   or lets two standard runs start. The gotcha about `argValues()` flag spellings in `e2e/AGENTS.md`
   is the thing to keep true.
5. **How many concurrent runs must be supported?** Two different answers per tier, and Phase A takes
   two locks accordingly. **Light: one per worktree** — a lock in the slug-keyed app-data root, so a
   second light run in the same worktree exits naming the holding process id rather than silently
   sharing ports and directories. **Standard and heavy: one per host** — a lock at a fixed
   host-global path, because the podman machines and `~/.ssh` cannot be slug-keyed. A light run never
   takes the host lock; that is the whole point of the split.
6. **Does the shared application log need splitting?** Yes when `MIO_APP_DATA_DIR` is set — Phase C
   routes `init_logging()` through it, so each worktree's run writes its own `mio.log`. When the
   variable is **not** set the log directory must stay exactly where it is; see Resolved Decision 9.
   The cost is that a developer debugging a light failure looks under the slug-keyed root; Phase A
   prints that root once at run start to close the discoverability gap.

### Resolved by review, before implementation

7. **Where is the port pair allocated, given the config module runs once per process?** In the
   launcher, inside `onPrepare`, published to workers through `process.env`. Allocating at module
   scope would re-probe independently in every worker, making the pair the launcher logs advisory
   rather than authoritative — precisely the wrong property when diagnosing a concurrency failure.
   The launcher-allocated pair also removes the need for top-level `await` in the config module.
8. **Does the run lock survive a worker exiting?** Only if it is gated. The `onShutdown` handler is
   registered at module scope and therefore runs in every worker; an ungated `releaseRunLock()` would
   delete the launcher's lock as soon as the first spec file's worker exited, leaving the rest of the
   run unprotected. Both the acquire and the release are gated on `!process.env.WDIO_WORKER_ID`.
9. **Does routing `init_logging()` through the app-data root move the shipped app's logs?** Yes, if
   done naively, and it must not. `app_log_dir()` resolves to `dirs::data_local_dir()/<identifier>/logs`
   — **Local** AppData — while `app_data_dir()` and `app_config_dir()` resolve to Roaming. Folding
   the log directory into `app_data_root()/logs` unconditionally would relocate every user's log as a
   side effect of an e2e change. The override branch alone changes: `MIO_APP_DATA_DIR` set →
   `<root>/logs`; unset → `app_log_dir()`, untouched.
10. **Are the slug-derived port windows genuinely disjoint?** No, and the plan must not claim they
    are. A digest reduced into a fixed number of windows collides at a birthday rate — with a handful
    of worktrees, a shared window is unlikely but not rare. Disjoint windows are a strong default,
    not a guarantee, so Phase E logs the window index and treats an immediate `tauri-driver` exit as
    a recoverable bind failure rather than assuming it cannot happen.
11. **Can hard-linked assets drift between worktrees?** Only through a writer that writes *through*
    the link, and the current `build.rs` has two: `fetch_mutagen` finishes with `fs::copy` into an
    existing destination, which on Windows writes into the existing file rather than replacing it,
    and `fetch_llama` extracts straight into `src-tauri/binaries/llama`, overwriting in place. Phase F
    removes the destination before writing in both `setup_worktree()` and the `fetch_*` fallbacks,
    which is what makes "hard links cannot drift" true rather than merely likely.
12. **What does a fresh worktree need before `build dev --setup-worktree` can run?** More than the
    assets: `build.exe` is `.gitignore`d and produced by `rustc build.rs` (`README.md`), and neither
    `node_modules` tree is checked in. Phase F therefore gives the arm an optional target-path
    argument so it can be invoked from an already-staged worktree, and has it run `pnpm install`
    (one install at the root covers `e2e/` through `pnpm-workspace.yaml`) alongside the asset
    staging.

## Implemented: Phase A — run identity and run locks

New file `e2e/lib/run-identity.ts`, exporting from a single module-scope computation:

- `WORKTREE_ROOT` — `path.resolve(__dirname, "..")`, the absolute worktree root.
- `RUN_SLUG` — the directory basename joined to a short hex digest of `WORKTREE_ROOT`.
- `APP_DATA_ROOT` — `~/.sandbox-mio-e2e-appdata-<slug>`, replacing the fixed
  `~/.sandbox-mio-e2e-appdata` currently built inline in `e2e/wdio.conf.ts`.
- `WORKSPACE_ROOT` — `~/.sandbox-mio-e2e-<slug>` (Phase D consumes it).
- `DEBUG_DIR` — `<WORKTREE_ROOT>\src-tauri\target\debug\`, the process-filter anchor (Phase B).
- `IS_LAUNCHER` — `!process.env.WDIO_WORKER_ID`. Every side effect in this module and in the
  `onShutdown` handler is gated on it; the config module runs in each worker too (The concrete
  model), and an ungated lock release ends the run's protection at the first worker exit.
- `acquireWorktreeLock()` / `releaseWorktreeLock()` — create and remove `<APP_DATA_ROOT>/run.lock`.
  Acquisition against a live holder aborts with a message naming that process id.
- `acquireHostLock()` / `releaseHostLock()` — the same pair against a fixed
  `~/.sandbox-mio-e2e-host.lock`, taken only when the run is not a light run, so two standard or
  heavy runs cannot both own `mio-tests` and `~/.ssh`. The abort message names the holding process
  id and the worktree that holds it, since the other run is in a different directory.

Both locks record the process id **and** the process start time, and treat a holder as live only
when both match: Windows reuses process ids, so a pid-only liveness check can mistake an unrelated
process for the holder and refuse to start forever. A lock whose holder is genuinely gone is stale
and reclaimed.

`e2e/wdio.conf.ts` imports the module at the top and logs the slug, port pair, port window index,
workspace root, and app-data root in one line at the start of `onPrepare`, so every subsequent
phase's slug-keyed paths are discoverable from the run output.

## Implemented: Phase B — worktree-scoped process killing

Two functions, one filter change each, both to the invoking worktree's absolute
`src-tauri\target\debug\` path:

- `killStaleDevProcesses()` in `e2e/wdio.conf.ts` — takes `DEBUG_DIR` from
  `e2e/lib/run-identity.ts`.
- `kill_stale_dev_processes()` in `build.rs` — the same narrowing, with the path built from
  `root_dir()` rather than imported. The `build dev` arm is not part of an e2e run but shares the
  hazard.

Both filters must be built from the full `src-tauri\target\debug\` path rather than the worktree
root, so a run invoked from the primary worktree leaves the nested `.claude/worktrees/tool-ssh`
worktree's processes alone.

Both must also stop using PowerShell's `-like`. The path is interpolated into the script, and `-like`
gives `[`, `]`, `*`, and `?` wildcard meaning while a `'` in a path breaks the surrounding quoting —
a filter that silently matches too much is the exact failure this phase exists to prevent. Pass the
path in through an environment variable and compare with `StartsWith` under
`[StringComparison]::OrdinalIgnoreCase`: no escaping, no wildcard semantics, and the intent is
readable.

## Implemented: Phase C — app-data isolation in Rust

New file `src-tauri/src/paths.rs`, registered with `mod paths;` in `src-tauri/src/lib.rs`, exposing
two functions read at call time and never cached, matching the Environment Variables convention in
`AGENTS.md`. Both read `MIO_APP_DATA_DIR` through the existing `env_dir()` in
`src-tauri/src/env_ext.rs` rather than a fresh `std::env::var` call — that helper already carries the
read-at-call-time contract in its doc comment. `env_dir()` counts an empty value as set, so
`paths.rs` treats an empty `MIO_APP_DATA_DIR` as unset rather than resolving a relative empty path.

- `app_data_root(app) -> PathBuf` — the override when set, otherwise `AppHandle::path()`'s
  `app_data_dir()`.
- `app_config_root(app) -> PathBuf` — the override when set, otherwise `app_config_dir()`. The two
  collapse to one directory under the override, which matches Windows, where `app_data_dir()` and
  `app_config_dir()` already resolve to the same roaming path.

Five call sites route through the new module. Two keep their existing subsystem override as the
higher-precedence branch, so the behaviour the light tier relies on today is unchanged:

- `settings_path()` in `src-tauri/src/settings/mod.rs` — `MIO_SETTINGS_DIR`, else
  `app_config_root()`.
- `store_dir()` in `src-tauri/src/credentials/mod.rs` — `MIO_CREDENTIALS_DIR`, else
  `app_data_root()/credentials`.
- `models_root()` in `src-tauri/src/inference/mod.rs` — `app_data_root()/models`.
- `data_dir()` in `src-tauri/src/sync/mod.rs` — `app_data_root()/mutagen`, still exported to the
  sidecar as `MUTAGEN_DATA_DIRECTORY`.
- `init_logging()` in `src-tauri/src/lib.rs` — `<override>/logs` when `MIO_APP_DATA_DIR` is set, and
  otherwise `app_log_dir()` exactly as today, keeping the existing `std::env::temp_dir()` fallback
  when path resolution fails. The asymmetry is deliberate and load-bearing: `app_log_dir()` is
  Local AppData while `app_data_dir()` is Roaming (Resolved Decision 9), so folding the unset case
  into `app_data_root()/logs` would relocate the shipped app's logs for every user.

  This phase also fixes a pre-existing bug this feature's spike surfaced (`03_e2e.md`, De-risking
  spike results): `init_logging()` currently does `Box::leak(Box::new(guard))`, permanently leaking
  the `tracing_appender::non_blocking` `WorkerGuard`. That guard's `Drop` is the only code path that
  signals the worker thread to shut down and blocks briefly for it to drain its queue; leaking it
  means whatever is enqueued but not yet written when the process is killed is silently lost, for
  every run, not only e2e. `init_logging()` moves to returning the guard (or storing it in
  `app.manage()` / a field the caller keeps alive) instead of leaking it, so it drops — and flushes —
  when the app actually shuts down. This is a correctness fix independent of the app-data
  redirection above, worth landing in the same phase since both touch `init_logging()`.

Redirecting all five keeps the invariant "no app-data write escapes `MIO_APP_DATA_DIR` when the
variable is set" statable and checkable, and hands the deferred standard-tier work its Mutagen data
directory. The invariant is deliberately scoped to app data: the sandbox workspace root
(`SANDBOX_WORKSPACE_ROOT`) and `~/.ssh` are outside it by design, isolated by Phase D instead.

Overriding `APPDATA` in the app's spawn environment is not an alternative and must not be
reintroduced: `dirs-6.0.0`'s `data_dir()`/`config_dir()` on Windows call
`known_folder_roaming_app_data()`, which queries the known-folder API and ignores the environment
variable.

In `e2e/wdio.conf.ts`: `MIO_APP_DATA_DIR` is set to `APP_DATA_ROOT`; `MIO_SETTINGS_DIR` and
`MIO_CREDENTIALS_DIR` become subdirectories of it instead of of the fixed
`~/.sandbox-mio-e2e-appdata`; and the `onComplete` model cleanup targets
`<APP_DATA_ROOT>/models/<test model id>` instead of `%APPDATA%/com.tincho.sandbox-mio/models`. The
`APP_DATA_DIR`/`APP_DATA_MODELS` constants built from `process.env.APPDATA` are deleted — nothing
outside the model cleanup reads them.

Deleting those constants orphans three comment blocks that justify the redirections *in terms of*
them, and one of the justifications stops being true: the model cleanup's "debug and release share
the app-data store" no longer holds once the store is e2e-owned and slug-keyed, and the credentials
and settings comments both reference `APP_DATA_DIR` by name. Rewriting those four comments is part
of this phase, not a follow-up — a stale comment is worse than none (`CLAUDE.md`). The reason the
model cleanup still removes only the test model's subdirectory changes from "the release build shares
this store" to "re-downloading a 100 MB GGUF per run is not worth the disk it saves".

## Implemented: Phase D — workspace root and tier-scoped teardown

In `e2e/wdio.conf.ts`:

- `WORKSPACE_ROOT` is imported from `e2e/lib/run-identity.ts` rather than built inline, becoming
  `~/.sandbox-mio-e2e-<slug>`. It stays under `$HOME`: the existing comment records that the podman
  machine only auto-mounts the user profile into its VM, which the standard tier's bind mount needs.
  The `SANDBOX_WORKSPACE_ROOT` export and both `fs.rmSync` calls (`onPrepare`, `onComplete`) follow
  it unchanged in shape — a stable slug keeps the delete self-healing, so a crashed run's leftovers
  are reclaimed by the next run in the same worktree.
- The `~/.ssh` cleanup block in `onComplete` — the `~/.ssh/sandbox-mio/config` removal and the
  `Include sandbox-mio/config` stripping — moves inside the existing `!IS_LIGHT_RUN` branch that
  currently guards only the remote-machine teardown. A light run writes neither file, so the block is
  pure collateral damage against a concurrent standard-tier run today. With the host lock from
  Phase A serialising standard runs, tier-gating here is sufficient: no other run that writes
  `~/.ssh` can be in flight.

## Implemented: Phase E — port pair allocation

Extends `e2e/lib/run-identity.ts` with port allocation and rewires two call sites. Allocation runs in
the launcher only, inside `onPrepare` (Resolved Decision 7).

- `PORT_WINDOW_BASE` — the slug digest reduced into a window index, times 16, added to the band base
  (`MIO_PORT_BASE` or 45000), so each worktree gets a 16-port window it probes first. Windows are
  disjoint for distinct digests but **not guaranteed** disjoint across worktrees, since the digest is
  reduced into a finite set (Resolved Decision 10) — treat them as a strong default that makes the
  probe-then-bind race rare, not as a proof it cannot happen.
- `allocatePortPair()` — probes upward inside the window for the first two consecutive free ports.
  Probing inside the window handles a re-run started while the previous run's ports sit in
  `TIME_WAIT`. Exhaustion throws with the window bounds and `MIO_PORT_BASE` named.
- `onPrepare` publishes the pair as `MIO_DRIVER_PORT` and `MIO_NATIVE_PORT` in `process.env`. Workers
  are forked after `onPrepare` returns and inherit the environment as it stood then, so every worker
  reads the same authoritative pair — and the pair the launcher logged is the pair that was used.

In `e2e/wdio.conf.ts`: `config.port` is read from `MIO_DRIVER_PORT` at module scope instead of the
literal `4444`, and the `tauri-driver` spawn in `beforeSession` gains `--port` and `--native-port`
alongside its existing `--native-driver` argument.

The existing `tauriDriver.on("exit")` handler currently treats any unexpected exit as fatal. Because
a window collision surfaces exactly there — as an immediate exit with a bind error — it gains one
retry: on an exit within the first seconds, re-probe for a fresh pair and respawn once before
failing, and name `MIO_PORT_BASE` in the failure message. Without that, the least likely failure mode
is also the most confusing one.

## Implemented: Phase F — worktree asset staging and per-run staging cost

Two problems in one phase, because both are about moving the same large files.

**A fresh worktree cannot run at all.** It has neither `tools/msedgedriver.exe` nor
`src-tauri/binaries/`, both `.gitignore`d, so `tauri-driver` has no native driver and `onPrepare`'s
agent-bundle copy has no source file. It also has no `build.exe` (`.gitignore`d, produced by
`rustc build.rs`) and no `node_modules`.

New arm `build dev --setup-worktree [<target>]` in `build.rs`, with a new `setup_worktree()` function
and a `link_or_copy(src, dst)` helper:

- The optional `<target>` argument is the worktree to stage *into*, defaulting to the invoking one.
  It exists because the worktree that needs staging is exactly the one with no `build.exe` to run
  (Resolved Decision 12), so the usual invocation is from an already-staged worktree.
- Run `pnpm install` in the target root — `pnpm-workspace.yaml` lists `e2e`, so one install covers
  both trees.
- Resolve sibling worktrees by parsing `git worktree list --porcelain`, skipping the target.
- Hard-link `tools/msedgedriver.exe` from the first sibling that has it, falling back to a byte copy
  when the link fails (different volume). When no sibling has it, run `msedgedriver-tool.exe` (which
  `e2e --setup` installs into `~/.cargo/bin` globally) in the target's `tools/` directory rather than
  only printing a hint, mirroring how the binaries groups below fall back to their own `fetch_*`.
- Stage `src-tauri/binaries/` **per asset group** — mutagen, llama, podman — each with its own
  sentinel file, so a sibling that has mutagen staged but not llama does not silently produce a
  partially staged target whose `fetch_*` fallback never runs. Hard links cost no disk for the large
  `llama` (89 MB), `podman` (59 MB), and agent-bundle (93 MB) payloads.
- Link a group from a sibling **only when the sibling's `.staged-versions.json` manifest matches this
  checkout's pinned version** for that group; each `fetch_*` writes its entry after staging. Version
  constants alone do not make the link safe: parallel feature branches are exactly the case where two
  worktrees sit on different commits, and linking on mere existence hands this checkout a binary its
  own `MUTAGEN_VERSION`/`LLAMA_VERSION`/`PODMAN_VERSION` does not pin, which no later `fetch_*`
  corrects because the file already exists.
- Remove an existing destination before linking or copying, and do the same in the `fetch_*`
  functions before they write. Hard links make the staged assets shared mutable state:
  `fs::copy` on Windows writes into an existing destination rather than replacing it, and
  `fetch_llama` extracts in place, so without an unlink first a re-fetch in one worktree silently
  rewrites a sibling's staged asset (Resolved Decision 11).
- Fall back to `fetch_mutagen()`, `fetch_llama()`, and `fetch_podman()` when no sibling has the
  asset. These are not safe to run concurrently — each extracts through a fixed `env::temp_dir()`
  path it `fs::remove_dir_all`s first — so `setup_worktree()` runs them only on the no-sibling path
  and prints a warning naming the hazard when it does.
- Add the arm's guard **above** the general `Some("dev")` arm in `main()`, beside the existing
  `dev --setup` guard, since the general arm matches any second argument. It hangs off `dev` rather
  than `e2e` because staging is the first thing a fresh worktree does — before `dev`, before any e2e
  tier — not an e2e-specific step.
- Extend the usage text printed by the `_` arm of `main()` with the new command, listed first.

**Every run copies ~180 MB it did not need to.** `onPrepare` unconditionally copies
`mutagen-agents.tar.gz` (93 MB) and the whole `src-tauri/binaries/llama` directory (89 MB) into
`src-tauri/target/debug`. That was invisible when one run happened at a time; under the development
loop above it is paid on every iteration of the fast loop, in each worktree, on one disk. Both copies
become `link_or_copy` against the same helper `setup_worktree()` uses, skipped when the destination
already exists with a matching size — the assets are version-pinned, so an existing destination of
the right size is the right file.

One consolidation belongs here rather than in a later cleanup: `onPrepare` copies the Mutagen agent
bundle from `tools/mutagen-agents.tar.gz`, while commit 97e5e87 added the same file at
`src-tauri/binaries/mutagen-agents.tar.gz`, where `stage_mutagen_agents()` in `build.rs` reads it.
Repointing the `onPrepare` copy at the `src-tauri/binaries/` location reduces the staged surface to
`src-tauri/binaries/` plus one driver executable, which is what `setup_worktree()` then has to move.

## Implemented: Phase G — documentation

- `AGENTS.md`, Environment Variables section: add `MIO_APP_DATA_DIR` (app-data and log root; default
  the Tauri `app_data_dir()`, and for logs `app_log_dir()`), `MIO_PORT_BASE` (e2e WebDriver port band
  base; default 45000), and the two already-shipped variables `MIO_SETTINGS_DIR` and
  `MIO_CREDENTIALS_DIR`, which the table does not yet list. `MIO_DRIVER_PORT`/`MIO_NATIVE_PORT` are
  listed too, marked harness-set rather than as knobs: they are not read by the Rust side, but both
  `e2e/wdio.conf.ts` and `light/run-identity.spec.ts` read them, so a reader who meets them in the
  code needs an entry to land on.
- `AGENTS.md`, Tier cadence section: record the parallel-worktree loop — light per worktree
  concurrently, standard once per feature and one at a time host-wide.
- `e2e/AGENTS.md`: document the run slug, where a run's workspace root, app-data root, and `mio.log`
  now live, the two locks and which tier takes which, the `build dev --setup-worktree` prerequisite
  for a fresh worktree, and that two concurrent debug builds block on the global cargo package-cache
  lock (`Blocking waiting for file lock on package cache` is contention, not a hang).

## Lifecycle and cross-cutting concerns

**Ownership of the slug.** Every per-run path and port derives from `e2e/lib/run-identity.ts` and
nothing recomputes a slug locally. A future collision is fixed in that one module.

**Destructive-action scope.** After Phases C and D, every `fs.rmSync` in `e2e/wdio.conf.ts` targets a
path containing the slug, except the `~/.ssh` block, which is tier-gated instead because the files
are genuinely shared user state. That split is the invariant to preserve when adding a new cleanup:
slug-scope it, or gate it on the tier and say why.

**Cross-tier matrix.** The pairs that have to be safe, and what makes each safe:

- *Light beside light, different worktrees.* Ports (Phase E), workspace and app-data roots
  (Phases C, D), process killer (Phase B).
- *Light beside standard, different worktrees.* The same three, plus the `~/.ssh` gate (Phase D).
  The Mutagen daemon needs nothing: a light run's `MutagenCli` is scripted through
  `ScriptedMutagenRunner`, so it never starts or stops a real daemon.
- *Light beside light, same worktree.* Refused by the worktree lock (Phase A).
- *Standard beside standard, any worktrees.* Refused by the host lock (Phase A). Genuine concurrency
  here stays deferred in `04_improvements.md`.

**Lock lifetime.** `acquireWorktreeLock()` — and `acquireHostLock()` for a non-light run — run at the
top of `onPrepare`, before any deletion, so a second run aborts before it can destroy the first run's
directories. The releases run in `onComplete` and in the `onShutdown` handler that already calls
`closeTauriDriver()`, so a Ctrl-C does not strand either lock. Both acquire and release are gated on
`IS_LAUNCHER`: the config module and its `onShutdown` registration execute in every worker too, and
an ungated release ends the run's protection when the first spec file finishes (Resolved Decision 8).
A stale lock whose holder is gone — matched on process id *and* start time — is reclaimed rather than
fatal.

**Ordering.** Phase A is a prerequisite for B, C, D, and E, which are otherwise independent of each
other. The lettering above is the recommended landing order, and it is not the order of risk: B, C,
and D are the phases that stop damage happening *today* (a `build dev` killing another worktree's
processes, a light run deleting a standard run's SSH aliases, two worktrees sharing one app-data
root), and they make sequential cross-worktree work safe on their own. E is what makes runs actually
concurrent and is the riskiest phase, so it lands after the safety work rather than before it. F is
independent of all of them and is the one phase testable from a fresh worktree without the rest; a
newly created worktree needs it before it can run anything at all. G lands last, describing what
shipped.

Until A–F have all landed, worktrees should not be run concurrently: A–D make concurrent runs
*non-destructive*, but without E they still collide on port 4444 and fail.

**Verification.** Two worktrees running `./build e2e light` concurrently, both passing, is the
headline acceptance condition, and the sequential-then-concurrent progression `01_ideas.md` proposed
as a spike becomes the manual verification of Phases A–E. Passing is necessary but not sufficient:
two runs that never happened to overlap on a shared resource also pass. The run must additionally be
shown to have been isolated — each worktree's own app-data root holding its own `mio.log` covering
only its own session, and the slug present in the settings, credentials, and workspace paths the app
actually used. The test plan is `03_e2e.md`.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A change to app-data isolation appears already done, then two worktrees still collide | `MIO_SETTINGS_DIR` and `MIO_CREDENTIALS_DIR` already exist in `src-tauri/src/{settings,credentials}/mod.rs`, but `e2e/wdio.conf.ts` points them at a fixed `~/.sandbox-mio-e2e-appdata` shared by every worktree | Keep the two variables and re-root them under the slug-keyed `APP_DATA_ROOT` from `e2e/lib/run-identity.ts` (Phase C) |
| The new `MIO_APP_DATA_DIR` root has no effect on settings or credentials | `settings_path()` and `store_dir()` check their own subsystem variable first, by design, and `e2e/wdio.conf.ts` sets both | Expect the specific variable to win; verify the root's effect through `models_root()`, `data_dir()`, or the log directory instead |
| The run lock disappears partway through a multi-spec run, and a second run starts on top of it | `e2e/wdio.conf.ts` is re-evaluated by every worker, so the module-scope `onShutdown` handler runs on each worker exit as well as the launcher's | Gate both acquire and release on `IS_LAUNCHER` (`!process.env.WDIO_WORKER_ID`) — Phase A, Resolved Decision 8 |
| The port pair `onPrepare` logged is not the pair the session used | A module-scope probe re-runs independently in every worker process | Allocate once in `onPrepare` and publish via `MIO_DRIVER_PORT`/`MIO_NATIVE_PORT`; workers inherit `process.env` as of fork, which is after `onPrepare` (Phase E) |
| A shipped release starts writing `mio.log` to Roaming AppData instead of Local | `app_log_dir()` is `data_local_dir()/<identifier>/logs`, not a subdirectory of `app_data_dir()`, so folding it into `app_data_root()/logs` moves it | Change only the override branch: `MIO_APP_DATA_DIR` set → `<root>/logs`, unset → `app_log_dir()` unchanged (Phase C) |
| The tail of a session's log lines is missing, in e2e and in the shipped app alike | `init_logging()` leaks its `tracing_appender` `WorkerGuard` via `Box::leak`, so `Drop` — the only path that waits for the worker thread to drain its queue — never runs; anything still queued when the process is killed is lost | Keep the guard alive for the app's lifetime instead of leaking it (Phase C) |
| A concurrent run in a second worktree dies mid-suite with its app process gone | `killStaleDevProcesses()` in `e2e/wdio.conf.ts` and `kill_stale_dev_processes()` in `build.rs` both match `*\target\debug\*` host-wide | Narrow both filters to the invoking worktree's absolute `src-tauri\target\debug\` path (Phase B) |
| Narrowing the process filter to the worktree root still kills a sibling worktree's processes | Worktrees exist under `.claude/worktrees/` *inside* the primary worktree, so the nested worktree's paths carry the primary root as a prefix | Match the full `<worktree>\src-tauri\target\debug\` path, never a prefix of the worktree root |
| The narrowed process filter matches nothing, or too much | PowerShell `-like` gives `[`, `]`, `*`, `?` wildcard meaning, and a quote in the path breaks the interpolated script | Pass the path via an environment variable and compare with `StartsWith` under `OrdinalIgnoreCase` (Phase B) |
| Setting `APPDATA` in the app's spawn environment does not relocate the model store or logs | `dirs-6.0.0`'s `data_dir()`/`config_dir()` on Windows call `known_folder_roaming_app_data()`, which ignores the environment variable | Route the call sites through `app_data_root()`/`app_config_root()` in `src-tauri/src/paths.rs` (Phase C) |
| A standard-tier run in another worktree loses its `~/.ssh` aliases when an unrelated light run finishes | The `~/.ssh` cleanup in `onComplete` is unconditional, unlike the remote-machine teardown directly below it | Move the block inside the existing `!IS_LIGHT_RUN` branch (Phase D) |
| Two standard runs in different worktrees tear down each other's podman machines | `mio-tests`/`mio-tests-remote` are singular and host-global; no slug can separate them | The host lock (Phase A) refuses the second run; genuine standard concurrency stays deferred in `04_improvements.md` |
| Two concurrent runs fail with a port-in-use error or a WebDriver connection refusal | `config.port` is the literal `4444`, and `tauri-driver` defaults to `4444` plus `4445` for the native driver | Allocate a slug-derived window and pass the pair to `config.port` and to `tauri-driver --port`/`--native-port` (Phase E) |
| Two worktrees collide on ports even though each has "its own" window | The slug digest is reduced into a finite set of windows, so distinct worktrees can share one | Read the window index from the run-start log line, move the band with `MIO_PORT_BASE`, and rely on the one respawn retry in `spawnTauriDriver()`, which re-probes and reassigns `config.port` inside `beforeSession` so the session follows the driver to its new port (Phase E) |
| `tauri-driver` fails to launch in a fresh worktree, or the agent-bundle copy throws ENOENT | `.gitignore` excludes `tools/msedgedriver.exe`, `tools/mutagen*`, and all of `/src-tauri/binaries/` | Run `build dev --setup-worktree` first (Phase F) |
| `build dev --setup-worktree` cannot be run in the worktree that needs it | `build.exe` is `.gitignore`d and built by `rustc build.rs`, and `node_modules` is absent too | Invoke it from an already-staged worktree with the new target-path argument, or run `rustc build.rs` in the fresh worktree first (Phase F) |
| A version bump re-fetched in one worktree silently changes another worktree's staged asset | The two are hard links to one inode, and `fs::copy` on Windows plus in-place `bsdtar` extraction both write through the link | Remove the destination before writing, in `setup_worktree()` and in every `fetch_*` (Phase F) |
| `pnpm tauri build` sits at "Blocking waiting for file lock on package cache" | `~/.cargo/.package-cache` is global; the other worktree's build holds it during registry access | Contention, not a hang — it clears on its own; per-worktree `target` directories mean the builds themselves never contend |
| A light failure cannot be diagnosed because `mio.log` is not where it used to be | Phase C moves the log under the slug-keyed app-data root whenever `MIO_APP_DATA_DIR` is set | Read the slug line `onPrepare` prints at run start (Phase A) |
