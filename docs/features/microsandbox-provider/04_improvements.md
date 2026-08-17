# Microsandbox Provider — Deferred Improvements

## Retire `MockRuntime` and `MockNetworkManager` onto the scripted-runner seam

`02_dev.md`'s Phase B/C route the real `MiseRuntime` and `MutagenNetworkManager` through
`MachineProvider`, but keep their hand-written mocks (`src-tauri/src/runtime/mock.rs`,
`src-tauri/src/network/mock.rs`) for the `light` tier. Once both subsystems reach `podman` only
through the provider, the provider could itself run over `ScriptedPodmanRunner` under
`MIO_MOCK_BACKEND=1`, letting `default_runtime` / `default_network_manager` drop their mock branches
entirely — the direction `docs/features/scriptable-mock-backend/02_dev.md` records as superseding
hand-written mocks.

Deferred because it is a much larger change than the extraction and would break every `light` spec
that relies on `MockRuntime`'s per-workspace package tracking and `MockNetworkManager`'s forward
bookkeeping: those semantics live above the container primitive (mise manifest state, forward
registry) and would have to be reproduced through scripted `podman.*` output plus the runner's
tracked state, spec by spec. Keeping the two mocks makes the Podman-only extraction verifiable by the
existing `standard` tier with zero light-spec churn; retiring them is its own reviewable follow-up.

## Intermittent `agent-web.spec.ts` failure: the create-time SSH alias block goes missing

`e2e/test/specs/standard/agent-web.spec.ts`'s first test asserts that `create` left a
`Host <sandbox>` block in `~/.ssh/sandbox-mio/config`. It was absent in 2 of 5 `standard` runs during
Phase J, and present in the other 3. A single baseline run (Phase J changes stashed) passed, which at
a ~40% observed failure rate is far too weak a sample to either blame Phase J or clear it.

What is established, and rules out the obvious explanations:

- The container exists and publishes no ports — the assertion *before* the failing one passes — so
  `create` reached `create_instance` and chose the exec-transport model.
- A `create` that returns `Ok` must have called `ensure_sandbox_alias`, so either it returned `Err`
  after making the container (and the row still appeared, which the frontend's create flow should not
  allow), or the block was written and then lost.
- The harness does not reset the file per session: `wdio.conf.ts` removes it only in `onComplete`.
- `useSandboxWorkspace` does not pre-create the sandbox, so there is no second `create` racing.
- Instrumenting `write_alias` with an immediate read-back over a full clean run showed `present=true`
  every time and the file growing monotonically (6617 → 9012 bytes across specs) — nothing truncates
  it mid-run. That run did not reproduce the failure, so the failing case remains unobserved.

Next step when it recurs: keep the `write_alias` read-back *and* log `create`'s `Err` path and the
frontend's create-row error state together, so the two remaining branches ("returned Err after the
container existed" vs. "written then clobbered") can be told apart from one run. Suspect worth
checking first: two app processes overlapping at a spec boundary, since `write_alias`'s
read-modify-write is unguarded across processes (see the next section).

## `ssh::write_alias` does a read-modify-write with no lock

**Done.** `write_alias`, `ensure_include`, and `remove_alias` (`src-tauri/src/ssh/mod.rs`) now each
run their whole read-strip-append-write sequence inside `with_config_lock`, which holds both the
existing in-process `CONFIG_LOCK` `Mutex` and a new cross-process advisory lock (`std::fs::File::lock`
— `flock`/`LockFileEx` underneath — on a stable sibling file, `sandbox-mio/.config.lock`, never the
config file itself since `atomic_write`'s `rename` would strand a lock on the pre-rename inode). This
is the "real file lock" this section originally called for: two app processes racing at an e2e session
boundary now serialize on the OS-level lock the same way two threads in one process serialize on the
`Mutex`. `write_alias`'s internal call into `ensure_include`'s logic goes through a `_locked` variant
that does not re-acquire the lock, avoiding a self-deadlock. `std::fs::File::lock`/`unlock` are used
directly (stabilized in the Rust toolchain this project builds with) rather than pulling in a locking
crate.

Not independently reproduced against the `agent-web.spec.ts` flake described above — no failing run
was captured to confirm this was in fact the mechanism, only that it was the most plausible one and
the only one left unruled-out. If the flake recurs, that is strong evidence this fix did not address
the actual cause and the "next step" instrumentation above should still be pursued.

Original text, superseded by the above: `write_alias` (`src-tauri/src/ssh/mod.rs`) read the managed
config, stripped its own block, appended the new one, and atomically wrote the result. `CONFIG_LOCK`
guarded only `ensure_include`, not this sequence, so two concurrent writers — two threads, or two app
processes overlapping at an e2e spec boundary — could each read the same base and the later write
would silently drop the earlier one's block. The atomic rename prevented a torn file, not a lost
update. It was deferred because no in-process concurrent caller existed (`maxInstances: 1` in the e2e
harness, and the app's own alias writes are sequential within a command), and a cross-process fix
needed a real file lock rather than the existing `Mutex`.

## Rename `PodmanCliBackend`, which is no longer Podman-only

**Done**, as a standalone rename: `PodmanCliBackend` → `CliBackend` (`src-tauri/src/backends/types.rs`),
updated across every source call site and doc comment that named it (`backends/mod.rs`,
`machine/types.rs`, `mockctl/mod.rs`, `mockctl/types.rs`, `process/types.rs`, `provider/msb.rs`,
`provider/mod.rs`, the two `e2e/test/specs/light/` comments) plus the `src-tauri/AGENTS.md` subsystem
table row and Common Gotchas entries, and `e2e/AGENTS.md`'s two references. Historical feature docs
that cite the old name in the context of decisions made under it were left as written, since they
record what was true at the time.

Phase J made `SandboxBackend`'s instance lifecycle dispatch per sandbox through `ProviderRegistry`,
so the struct now creates, lists, starts, stops and destroys microsandbox sandboxes too. Only the
orchestration around it — deploy, the host registry, the event stream — is still genuinely
Podman-specific, which is why the new name keeps `Cli` (still drives the `podman` CLI directly for
that orchestration) but drops `Podman` (no longer an accurate blanket claim).

## Cache or narrow the `list()` call behind per-sandbox provider resolution

`ProviderRegistry::provider_of` and `SandboxBackend::list` ask every provider for its full instance
list to answer "which provider owns this name". For microsandbox that costs one `msb inspect` per
instance (the N+1 `01_ideas.md`'s Technology Choices already accepted), and `lib.rs` command handlers
call it on nearly every action — `sandbox_facts` collapsed two such passes into one, but several
handlers still do a `list()` per invoke.

Deferred because it is a performance concern with no correctness component at today's sandbox counts,
and the obvious fixes (a short-lived cache, or a provider-tag label read from a cheaper call) each
need their own invalidation story. Revisit if the projects page feels slow with many sandboxes, or
once a `standard`-tier microsandbox spec makes the `msb inspect` cost measurable.

## Give `SyncEngine` a first-class notion of a workspace session

A microsandbox sandbox's workspace sync is distinguished from a deploy purely by session naming
(`backends::msb_sync_session` → `workspace-<name>`, versus a deploy's bare `<name>`), because
`SessionInfo` has no field saying what a session is *for*. Every consumer therefore has to know the
naming convention: `sandbox_deployments` reports whatever is named for a sandbox, `destroy_sandbox`
terminates both shapes, and `sync_status`/`enable_sync`/`disable_sync` silently address only the
deploy one.

Deferred because adding a session kind to `SessionInfo` means parsing something Mutagen does not
give us (labels would have to be encoded into the session name anyway, or tracked app-side), which is
a larger design question than Phase J needed to settle. The naming convention is documented at both
ends; the risk is a future consumer that greps sessions by sandbox name without knowing it.

## Merge `PodmanMachineManager`'s `prepare()` with `provision_podman`/`start_podman_machine`

Phase I added `MachineReadinessProvider::prepare` (`src-tauri/src/machine/mod.rs`) as an independent
reimplementation of the WSL-install → machine-init → machine-start → socket-activate sequence that
`provision_podman`/`start_podman_machine` (`src-tauri/src/lib.rs`) already drive for the projects-page
recovery gate. The two now duplicate the same retry ladder (WSL shutdown-and-retry on a failed
machine start, `activate_podman_socket` before the final `poll_ready`).

Deferred deliberately, not by oversight: `provision_podman`/`start_podman_machine` back
`e2e/test/specs/standard/podman-startup-recovery.spec.ts`, the only spec that exercises them against
a real stopped/started Podman machine, and this phase had no way to re-run the `standard` tier to
prove a merged implementation is byte-identical (`docs/features/microsandbox-provider/02_dev.md`,
Phase I). Once `prepare()` has its own manual or `standard`-tier verification, the lib.rs commands
should be rewritten to translate `MachineSetupEvent`s into `activity::progress` calls and delegate to
`prepare()`, removing the duplication.

## Restore the devtools inspector alongside file logging

`tauri-plugin-devtools` and the application's own file logging are mutually exclusive as wired
today. The plugin's `Builder::init` calls `tracing_subscriber::registry().try_init()` internally and
returns only a `TauriPlugin`, exposing no layer to compose into another registry, so whichever
initialiser runs first owns the global subscriber. Before 2026-08-14 the plugin always won, and
`init_logging` (`src-tauri/src/lib.rs`) discarded its own failed `try_init`, so every `mio.log` in a
debug build was 0 bytes and no backend event survived an app run.

The 2026-08-14 fix made the choice explicit rather than removing it: file logging is the default and
the inspector is opt-in through `MIO_DEVTOOLS=1`, with `init_logging` writing an explanatory line
into the log file when it loses the race. Developers who want the inspector still give up the log
file for that run.

Deferred because having both needs either an upstream change to `tauri-plugin-devtools` that exposes
its `Layer` for composition, or a local reimplementation of the plugin's aggregator wiring against a
shared registry — a larger piece of work than the diagnostic session that surfaced the conflict could
justify. Revisit if developers start needing the inspector and the log file in the same run.
