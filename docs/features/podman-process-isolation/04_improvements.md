# Podman Process Isolation — Deferred Improvements

Work deferred out of the scope defined by `docs/features/podman-process-isolation/02_dev.md`. Neither
item below blocks any phase of the build plan.

## Bound the `msb` and Mutagen process path

`run_capturing_via_files()` (`src-tauri/src/process/mod.rs`) captures stdout and stderr through temp
files but applies no timeout at all, unlike its podman counterpart `run_via_blocking_process()`, which
is bounded by `call_timeout()`. `SystemMsbRunner::run()` and `SystemMutagenRunner::run()`
(`src-tauri/src/process/mod.rs`) therefore hang a Tauri command indefinitely when the `msb` or Mutagen
binary stops responding, with no diagnostic beyond a spinner that never resolves.

Deferred as Resolved Decision 4 in `docs/features/podman-process-isolation/02_dev.md`. Bounding the
path requires characterising how long a legitimate `msb create` takes, in the way
`MACHINE_START_TIMEOUT` and `BUILD_TIMEOUT` (`src-tauri/src/process/mod.rs`) already characterise the
two slow podman calls. The measurement is independent of the console-handle defect the feature fixes,
and the `msb` path gains the Phase A sanitiser and the Phase B detached constructor regardless.

Sizing the bound is complicated by `msb create` spawning a detached VM supervisor that inherits
`msb.exe`'s standard handles, described in `run_capturing_via_files()`'s doc comment — a bound chosen
from the direct child's exit is correct only because the file-redirect workaround already decouples
the two.

## Restrict child handle inheritance with an explicit handle list

`PROC_THREAD_ATTRIBUTE_HANDLE_LIST` limits a child to an explicitly named set of inherited handles,
which is a stronger per-spawn guarantee than the Phase A sanitiser: the sanitiser clears the inherit
flag on handles present when `run()` (`src-tauri/src/lib.rs`) starts, and cannot govern an inheritable
handle a dependency creates later in the process lifetime.

Deferred because `CommandExt::raw_attribute` is behind the unstable
`windows_process_extensions_raw_attribute` feature on rustc 1.96.0 stable, verified by
`std-handle-list-check.rs` (`docs/features/podman-process-isolation/spike/`). Adopting the attribute
today means calling `CreateProcessW` directly or depending on the `windows` crate, and
re-implementing the temp-file capture, exit-status plumbing and child termination that
`src-tauri/src/process/mod.rs` already provides.

The deferred threat is narrow: the diagnosed failure came from handles the app *inherited* at
startup, which the sanitiser covers completely. Re-run `std-handle-list-check.rs` after a toolchain
upgrade to check whether the attribute has stabilised.

## Carry the unresponsive condition onto the runtime path

`From<ProviderError> for RuntimeError` (`src-tauri/src/runtime/types.rs`) maps
`ProviderError::Unresponsive` to `RuntimeError::CommandFailed`, so a package install or terminal
call that stalls on an unresponsive podman reaches the frontend as a generic backend error rather
than the `PodmanUnresponsive` kind Phase E added.

Deferred because nothing on the runtime path branches on the kind today — the two surfaces that
render the condition distinctly are both in the welcome flow — and the message still names the
condition, so the user-visible text is correct even where the discriminant is not. Adding a
`RuntimeError` variant is only worth doing alongside a runtime-side surface that acts on it.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| An `msb create` or Mutagen call hangs forever with no error and no log entry | `run_capturing_via_files()` (`src-tauri/src/process/mod.rs`) applies no timeout, unlike `run_via_blocking_process()` which is bounded by `call_timeout()` | Terminate the stuck `msb` or `mutagen` process manually; implement the bound described in the "Bound the `msb` and Mutagen process path" section above |
| A handle created after app startup is inherited by a spawned child | The Phase A sanitiser in `src-tauri/src/process/mod.rs` clears inherit flags once, at the start of `run()` (`src-tauri/src/lib.rs`), and does not govern handles created later | Accept the gap while `CommandExt::raw_attribute` is unstable; re-run `std-handle-list-check.rs` (`docs/features/podman-process-isolation/spike/`) after a toolchain upgrade |
| A runtime-path failure caused by an unresponsive podman arrives as `kind: "Backend"` | `From<ProviderError> for RuntimeError` (`src-tauri/src/runtime/types.rs`) collapses `ProviderError::Unresponsive` into `RuntimeError::CommandFailed` | Read the message text, which still names the condition; add a `RuntimeError` variant when a runtime-side surface needs to branch on it |
