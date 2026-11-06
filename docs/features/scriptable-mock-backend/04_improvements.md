---
type: improvements
title: "Scriptable Mock Backend — Deferred Improvements"
feature: scriptable-mock-backend
---
# Scriptable Mock Backend — Deferred Improvements

Improvements noticed while planning `docs/features/scriptable-mock-backend/02_dev.md` that are out of the feature's v1 scope. Each section states what the improvement is and why it was deferred.

**2026-07-21 note**: this document predates the process-level (`ProcessRunner`) rewrite of `02_dev.md`. Items below are updated to reflect that rewrite where it changes their shape; items unaffected by the rewrite are unchanged.

## `KNOWN_CALLS` coverage for RuntimeManager and NetworkManager

Previously: "Director coverage for RuntimeManager and NetworkManager" — extend the (then trait-level) `MockDirector` wiring to `MockRuntime`/`MockNetworkManager`, one constructor argument plus a `KNOWN_KEYS` group each.

Reframed under process-level addressing: `PodmanMiseRuntime` (`src-tauri/src/runtime/mod.rs`) and `MutagenNetworkManager` (`src-tauri/src/network/mod.rs`) both already shell out to `podman` through the shared `PodmanProcessRunner` seam once Phase C lands — `runtime`'s `podman()`/`podman_on()` helpers and `network`'s `podman()` helper both need to be pointed at the shared runner (network's currently bypasses `podman_command()` entirely and would need that fixed first, noted in `02_dev.md` Phase C). Once pointed at the shared runner, no *new* seam is needed — only new `KNOWN_CALLS` entries for the subcommands these two subsystems use that `backends`/`machine` don't: `podman.exec`, `podman.cp`, `podman.port`. `runtime` also shells to `mise` *inside* the container via `podman exec ... mise ...` — that's still a `podman.exec` call from the runner's perspective (the `mise` invocation is opaque to `ProcessRunner`, visible only in the scripted stdout), so no `mise`-specific runner is needed, only fixtures shaped like what `mise`'s stdout would look like relayed through `podman exec`. Deferred because no current light-spec scenario needs to script package-install, port-forward, or relay-socat failures — the seam existing makes adding these entries mechanical once one does, same rationale as before, just cheaper now (no mock struct to write, only fixture text).

## Host commands outside the `ProcessRunner` seam

New, surfaced while finishing Phase C: `ProcessRunner`'s scope limit (one-shot calls to `podman`/the
Mutagen sidecar) leaves the `wsl` invocations in `src-tauri/src/machine/mod.rs` running against the
developer's real host in the `light` tier, because the tier now runs the real
`PodmanMachineManager` instead of a mock that returned `Ok(())`. This was not theoretical:
`settings-debugging-ui.spec.ts` drives `reset_stuck_machine`, whose `wsl --unregister
podman-machine-default` deletes the machine's WSL distro and its disk image on every light run.

Fixed narrowly rather than by extending the seam: `run_host_wsl()` (`machine/mod.rs`) no-ops under
`MIO_MOCK_BACKEND=1`, covering the three host-mutating calls (`--unregister`, `--shutdown`,
`--install`). Read-only `wsl --status` still runs.

The general improvement remains open: a scriptable seam for non-`podman` host binaries (`wsl`,
`ssh-keygen`, `ssh`) would let a spec assert on these paths instead of only suppressing them, and
would remove the need to remember the gate when a new host command is added. Deferred because it is
a second seam with its own key namespace, and no current spec needs to assert on `wsl` behaviour —
only to be protected from it. Until then: **any new host-binary call reachable from a light spec
needs a `mock_backend_enabled()` gate**, and `ssh-keygen`/`ssh` in `backends/mod.rs` should be
audited the same way if a light spec ever reaches a deploy path.

## Backend event scripting through listen_events

Unchanged from before: `ProcessRunner` explicitly does not cover spawn/stream calls (`02_dev.md`, "Scope limit"), so `MockBackend::listen_events`'s inert channel is moot — there is no `MockBackend` anymore, and the real `PodmanCliBackend::listen_events` spawns and streams a `podman events` child that `ProcessRunner`'s one-shot `run()` cannot represent. Scripting synthetic events would need a separate seam (e.g. a scriptable stream source), not an extension of `ProcessRunner`. Deferred because the frontend `plugin:event|emit` bridge already covers every current synthetic-event need.

## Delay behaviour for latency and race simulation

Unchanged: a `Behaviour` variant that delays a scripted response, for loading-state and race assertions. Still deferred — no current spec needs it, and scripted delays interact with spec timeouts in ways that would make the light tier less deterministic. Applies identically at process-level addressing: the delay would sit in `ScriptedPodmanRunner`/`ScriptedMutagenRunner`'s `run()` before returning the canned `Output`.

## Compile-time gating of the control plane

Unchanged: move `mock_configure`/`mock_reset` behind `#[cfg(debug_assertions)]`. Still deferred per the same reasoning — conditional `generate_handler![]` entries complicate `src-tauri/src/lib.rs`'s state wiring, and the commands are already inert without `MIO_MOCK_BACKEND=1`.

## CI pipeline for the tier cadence

Unchanged: no CI configuration exists in the repository; introducing one is its own feature.

## `std::process::Output` vs. a shared local output type

New, surfaced during the process-level rewrite: `ProcessRunner::run` is specified in `02_dev.md` Phase D as returning `std::process::Output`, but `tauri_plugin_shell::process::Output` (what the real Mutagen sidecar call produces) is a different type with no documented public path to construct a `std::process::Output` from raw bytes on this toolchain. Phase D flags this as needing a short spike before committing to the exact trait return type — the fallback is a small crate-local `struct ProcRunOutput` shared by both `PodmanProcessRunner` and `MutagenProcessRunner`. Recorded here in case the spike takes longer than expected and needs to be pulled out of Phase D into its own preparatory step.

## `network/mod.rs`'s direct `Command::new("podman")` bypass

New, surfaced during the process-level rewrite: `network::podman()` (`src-tauri/src/network/mod.rs`) calls `Command::new("podman")` directly instead of going through `backends::podman_command()`, so it does not get `CONTAINERS_HELPER_BINARY_DIR`/the bundled `podman.exe` resource-dir resolution that every other subsystem's podman calls get. Pre-existing, not introduced by this feature. Worth fixing before wiring `network` onto the shared `PodmanProcessRunner` (the "`KNOWN_CALLS` coverage for RuntimeManager and NetworkManager" item above), since routing an inconsistent call path through a runner that assumes `podman_command()`-equivalent construction would paper over the bug rather than fix it.
