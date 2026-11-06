---
type: spike
title: "Spike Artefacts — Proxy Toggle Management"
feature: proxy-toggle-management
---
# Spike Artefacts — Proxy Toggle Management

A live spike run on 2026-08-18, per `../01_ideas.md`'s Validation section: create a sandbox, open a
Terminal pane, recreate the container via the raw provider calls the recreate-in-place feature would
use, confirm the workspace file, the SSH alias, and a fresh Terminal-pane open all still work end to
end. Kept as the evidence trail behind `../01_ideas.md`'s Validation section and `../02_dev.md`'s
Phase B/C design.

## `recreate_spike.rs`

A `#[cfg(test)] mod recreate_spike` that drove the real backend/provider code this feature's design
reuses, not a mock: `PodmanProvider::create_instance`/`remove` (`src-tauri/src/provider/mod.rs`),
`CliBackend::ensure_key`, `ensure_sandbox_alias`, `forget_sandbox_host_key` (`src-tauri/src/backends/
mod.rs`), and `RuntimeManager::init`/`reproduce_remote`/`prepare_terminal` (`src-tauri/src/runtime/
mod.rs`) — all against a real, throwaway `mio-spike-toggle-test` sandbox on the developer's actual
running `podman-machine-default`. Never touched the developer's real sandboxes (`kok`, `jjj`, or any
other), confirmed present and unaffected both before and after the run.

Unlike the Windows-process-isolation spike probes elsewhere under `docs/features/*/spike/`, this one
is not a standalone `rustc`-compilable file: it depends on `sandbox_mio_lib`'s in-crate-private items
(`CliBackend::ensure_key`, `connection_name`, `base_image`, `forget_sandbox_host_key`,
`ensure_sandbox_alias`, `crate::process::detached_command`), none of which are reachable from outside
the crate (every top-level module in `src-tauri/src/lib.rs` is declared `mod`, not `pub mod`). It was
run by temporarily appending this exact module to the bottom of `src-tauri/src/backends/mod.rs` and
running `cargo test --lib backends::recreate_spike -- --ignored --nocapture` with
`podman-machine-default` running, then removing it again — `src-tauri/src/backends/mod.rs` carries no
trace of it today (confirmed via `git diff` showing no changes to that file after removal). Re-running
the spike means pasting this file's `mod recreate_spike { ... }` body back onto the end of that file.

Sequence exercised, matching `../02_dev.md` Phase B/C's design:

1. Build a real `PodmanProvider` pinned to `connection_name()` (the same connection resolution
   `CliBackend` uses) via `provider::default_provider`.
2. Create a throwaway host workspace directory, write a marker file into it, and seed
   `mise.toml`/`mise.lock` via `RuntimeManager::init` — the one step `services::sandbox::create` does
   that `CliBackend::create` itself does not, needed before `reproduce_remote` has anything to copy.
3. `create_instance` against that workspace (the real `podman create ... --volume <dir>:/root/work`),
   `start`, generate a per-sandbox ed25519 keypair via `CliBackend::ensure_key`, install the pubkey via
   the real `copy_in`, and write the real SSH alias via `ensure_sandbox_alias`.
4. Confirm SSH works (`ssh -F <managed config> mio-spike-toggle-test echo ...`) and open a Terminal
   pane equivalent (`RuntimeManager::prepare_terminal`, which starts `zellij web` and mints a token).
5. Recreate: `provider.remove(name)` (`podman rm --force`), confirm the marker file still exists on
   the host, `create_instance` again against the *same* workspace directory, `start`.
6. Confirm the marker file is visible **inside** the recreated container (not just present on the
   host — the bind mount genuinely re-attached), re-run `forget_sandbox_host_key` +
   `CliBackend::install_sandbox_key`-equivalent + `ensure_sandbox_alias`, and diff the alias block
   text before vs. after.
7. Confirm SSH works again and a fresh Terminal pane opens (`reproduce_remote` then
   `prepare_terminal` again).
8. Teardown (always runs, success or failure): `podman rm --force` the spike container,
   `ssh::remove_alias` to strip the alias block, `forget_sandbox_host_key` to drop the spike's
   `known_hosts` entry, and delete every spike-owned temp directory.

## Result

Passed clean on the first attempt after one harness fix (see below) — full run in `~57s`, dominated by
`zellij web --daemonize`/token minting and `mise install` on each side, not by the recreate itself.

- **Risk 1 (workspace bind mount survives `podman rm --force`)**: confirmed. The marker file written
  before `provider.remove` was present on the host afterward, and readable *inside* the recreated
  container via `podman exec ... cat` — the bind mount re-attached to live content, not just an empty
  directory that happened to still exist.
- **Risk 2 (SSH alias byte-identical across a same-name recreate)**: confirmed. The rendered block
  (`ProxyCommand podman --connection podman-machine-default exec -i mio-spike-toggle-test mio-sshd`,
  plus `User`/`IdentityFile`/`UserKnownHostsFile`/`IdentitiesOnly`/`StrictHostKeyChecking` lines) was
  identical before and after recreate — a plain string comparison, not just "close enough."
- **Risk 3 (fresh SSH / Terminal pane works against the recreated container)**: confirmed both ways —
  a fresh `ssh` call succeeded with no host-key prompt or mismatch (the base image bakes a fixed sshd
  host key, so an unchanged image across recreate means an unchanged host key, exactly as
  `../01_ideas.md`'s Current State predicted), and a fresh `RuntimeManager::prepare_terminal` call
  against the recreated container came back with a working `zellij web` token/port after
  `reproduce_remote` reinstalled the mise-locked toolchain.

**One harness bug, no design bug.** The first run failed at the `reproduce_remote` step with
`"mise.toml" could not be found on the host` — not a flaw in the recreate design, but a gap in the
spike itself: it initially never called `RuntimeManager::init` to seed `mise.toml`/`mise.lock` in the
workspace, a step `services::sandbox::create` performs but `CliBackend::create` does not. Adding one
`runtime.init(&workspace_source).await` call before the first `create_instance` fixed it; the second
run passed end to end with no other changes. This confirms `../02_dev.md`'s Phase C step 6 ordering is
correct as designed — `retoggle_proxy` never needs to call `runtime.init` itself, because the
manifest/lockfile already exist in a sandbox's workspace from its original `services::sandbox::create`
call, and `reproduce_remote` only ever needs to *find* them, not create them.

No divergence from `../01_ideas.md`'s Proposed Model or `../02_dev.md`'s Phase B/C design surfaced.
Cleanup confirmed complete: no `mio-spike-toggle-test` container, no alias block in
`~/.ssh/sandbox-mio/config`, no leftover temp directories; `kok` and `jjj` (the developer's real
sandboxes) were listed via `podman ps --all` before and after the run and untouched throughout.
