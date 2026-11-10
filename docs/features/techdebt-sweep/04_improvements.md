---
type: improvements
title: "Tech Debt Sweep — Deferred Improvements"
feature: techdebt-sweep
---
# Tech Debt Sweep — Deferred Improvements

Work noticed while building `docs/features/techdebt-sweep/02_dev.md` and left out of its scope. Each section states the improvement and why it was deferred.

## Light-tier creates write the real `~/.ssh` and run real `ssh-keygen`

Under `MIO_MOCK_BACKEND=1` only the Podman and Mutagen runners are scripted, so a `light` spec's `create` still calls `ensure_sandbox_alias()` and `ensure_sandbox_identity()` (`src-tauri/src/backends/mod.rs`) against the developer's real `~/.ssh/sandbox-mio/config`. Phase G's destroy cleanup now removes those blocks, so the leak no longer accumulates, and the `onComplete` comment in `e2e/wdio.conf.ts` now says so. Deferred because gating the SSH writes off under the mock switch changes what the `light` tier exercises and needs its own decision.

## A failure inside `store.create` leaves the container, alias and key behind

`CliBackend::create` (`src-tauri/src/backends/mod.rs`) writes the SSH identity and alias and installs the key after the container exists, with no rollback of its own. A failure there (for example in `install_sandbox_key`) returns an error from `store.create`, so `create()` in `src-tauri/src/services/sandbox.rs` never reaches its rollbacks and the container, alias and key directory survive. Deferred because the fix belongs inside the provider-dispatched create path, which Phase D was already rewriting.

## msb calls are logged twice

Phase B logs every msb call in the runner (`msb call ok`, `src-tauri/src/process/mod.rs`) while `MsbProvider::run_env` (`src-tauri/src/provider/msb.rs`) still logs its own `msb ok` line. Drop one of the two. Deferred to avoid a conflict with Phase D, which edited the provider file at the same time.

## Verify the microsandbox secret transport and bound it

`msb exec` takes only `KEY=value`, so Phase H passes exec secrets to a microsandbox guest on stdin through a shell wrapper (`secret_exec_command()` and `exec_streamed()` in `src-tauri/src/provider/msb.rs`). The wrapper is unit-tested only; it needs a hands-on check against a real sandbox (agent web UI login on a microsandbox sandbox). `exec_streamed()` spawns `msb` directly, so it is outside Phase B's bounds, and `exec_command()` only debug-asserts that no secret is passed. Deferred because no real microsandbox sandbox was available to the work units.

## GitHub token for the base-image build

Resolved Decision 3 passes the GitHub token to in-sandbox `mise` calls only. A cold base-image build (`src-tauri/resources/Containerfile`) can still hit GitHub's unauthenticated rate limit. Passing the token needs a build secret rather than an exec environment variable.

## Unread `ForwardInfo.connection`

After Phase D, `ForwardInfo.connection` (`src-tauri/src/network/types.rs`) always holds the sandbox's own alias and nothing reads it. Drop it together with its TypeScript mirror in `src/lib/network/state.svelte.ts`. Deferred because the field crosses IPC and its removal is unrelated to any phase's goal.

## One secret-environment mechanism

The microsandbox `StartEnvSource` and Phase H's `ToolchainSecrets` (`src-tauri/src/runtime/types.rs`) both turn stored secrets into a process environment. Merge them when `docs/features/proxy-credential-secrets/` sources proxy secrets from the credential store, which `load_secret()` (`src-tauri/src/credentials/mod.rs`) already supports.

## Some e2e app launches never load the UI

During Phase K on 2026-11-10, 4 of 14 `light` spec files failed in one full run and 2 in a second because their app instance never loaded the frontend. The standard tier's `podman-startup-recovery` failed the same way once. `sandbox-start-failure-ui` (run after `sandbox-list-ui`) and `workspace-dashboard-switch-ui` (run after `sync-status-ui`) failed in both full runs. Run as a batch without those predecessors, all four passed, which points at state a preceding instance leaves behind. The only window stayed on `about:blank` for the full 30 seconds the `before` hook in `e2e/wdio.conf.ts` waits. `mio.log` for each of those instances ends at "cleared inheritable handles at startup". A healthy instance follows that line within a second with frontend-driven calls (the updater check, `msb ls` polling). WebDriver still answered on the window, so the WebView2 browser process was alive while the page navigation never committed. The failing specs differed in kind (`local-llm-ui`, `microsandbox-provider-ui`, `sandbox-start-failure-ui`, `workspace-dashboard-switch-ui`); "Origin header is not a valid URL" was only the downstream symptom. The same symptom is recorded for `activity-center-ui` in `docs/features/manage-dashboard-redesign/04_improvements.md`, observed on 2026-11-08 on a clean checkout, so it predates this phase. One unconfirmed suspect: every instance in a run shares the WebView2 user-data folder beside `target/debug/sandbox-mio.exe`, which a force-killed predecessor's browser process may still hold. Deferred because finding a WebView2 or main-thread startup hang needs a reproduction outside this feature's scope.

## Smaller leftovers

- The `MachineReadinessProvider` doc comment (`src-tauri/src/machine/types.rs`) still describes the trait as independent of the recovery gate, which Phase C made only partly true.
- `socat` stays in the base image only as `e2e/test/specs/standard/expose-port.spec.ts`'s throwaway server; replacing that fixture would let the image drop it.
- The base-image update prompt does not say what version 7 changes (sshd keepalive, `tini`).
- `docs/architecture/providers.md` is near the 700-word architecture budget.
