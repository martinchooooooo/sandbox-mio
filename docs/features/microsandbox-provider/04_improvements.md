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

`write_alias` (`src-tauri/src/ssh/mod.rs`) reads the managed config, strips its own block, appends the
new one, and atomically writes the result. `CONFIG_LOCK` guards only `ensure_include`, not this
sequence, so two concurrent writers — two threads, or two app processes overlapping at an e2e spec
boundary — can each read the same base and the later write silently drops the earlier one's block.
The atomic rename prevents a torn file, not a lost update.

Deferred because no in-process concurrent caller exists today (`maxInstances: 1` in the e2e harness,
and the app's own alias writes are sequential within a command), so fixing it is not required for any
current flow. It is recorded here because it is the most plausible mechanism for the `agent-web`
failure above, and a cross-process fix needs a real file lock rather than the existing `Mutex`.

## Rename `PodmanCliBackend`, which is no longer Podman-only

Phase J made `SandboxBackend`'s instance lifecycle dispatch per sandbox through `ProviderRegistry`,
so `PodmanCliBackend` (`src-tauri/src/backends/types.rs`) now creates, lists, starts, stops and
destroys microsandbox sandboxes too. Only the orchestration around it — deploy, the host registry,
the event stream — is still genuinely Podman-specific. The name now misleads, as does the row for it
in `src-tauri/AGENTS.md`'s subsystem table.

Deferred because a rename is pure churn across `types.rs`, `mod.rs`, `AGENTS.md` and the feature docs
that cite it, with no behavioural content, and Phase J was already the widest change in this feature.
Worth doing on its own, together with the `AGENTS.md` table row.

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
