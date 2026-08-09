# Podman Exec Transport: Improvements

## Zombie `sshd -i` processes accumulate under the container's `/bin/sh` PID 1

Phase A's validation spike found that every terminal/agent-web/exposed-port connection-open leaves
a `[sshd] <defunct>` zombie process inside the container once the session closes gracefully,
because PID 1 (`/bin/sh`, unchanged by this feature) never `wait()`s on reparented children. A
zombie costs one process-table slot and no CPU/memory, and a dev sandbox's practical
connection-open count over its lifetime is unlikely to approach the kernel's per-PID-namespace
limit, so this is not being mitigated now. A real fix would give the container a reaping PID 1
(e.g. `tini`/`dumb-init` ahead of `/bin/sh` in the Containerfile's `CMD`) — deferred because it
changes container startup behavior for every sandbox, not just exec-capable ones, and so needs its
own scoped change rather than folding into this feature's `create`/`deploy` argv rewrite.

Phase F's re-verification (spiked 2026-08-10, five real Mutagen `forward create`/`forward
terminate` cycles) measured the defunct count staying bounded at ~1 rather than growing one per
cycle, so the accumulation Phase A described is milder on the shipped teardown path than a
worst-case reading of Phase A suggests. The reaping-PID-1 fix stays a nice-to-have, not a
correctness fix.

## An abrupt (non-graceful) client death orphans a bare `-N` forwarding `sshd` indefinitely

Phase F's spike (2026-08-10) found that the exec-transport in-container `sshd -i` tears down cleanly
**only when the SSH connection closes gracefully** — which the app's normal path always does, since
`close_forward`/`close_sandbox` call `mutagen forward terminate`, and Mutagen closes its transport
gracefully (measured: zero running `sshd` after every terminate). But if the outer `ssh` client is
killed abruptly (`kill -9`, an app crash, the host sleeping) with no protocol disconnect, a bare
`-N`-style forwarding session's in-container `sshd` is **not** reaped: `podman exec` does not
propagate the client's death to the exec'd process, and `sshd_config.container` sets no
`ClientAliveInterval`/`ClientAliveCountMax`, so the orphaned `sshd` has no way to notice the broken
transport and lives until the container stops. (A session running a remote command — Phase A's
SIGKILL test — self-cleans when the command exits, which is why Phase A concluded SIGKILL was clean;
a forwarding session has no such command.) The cheap mitigation is to add
`ClientAliveInterval 15` / `ClientAliveCountMax 3` to the baked `sshd_config.container` so an
orphaned session self-terminates within ~45s of its transport going silent; deferred because it only
affects abnormal-exit cleanup, not correctness of the running feature, and touches the image
(a `BASE_IMAGE_VERSION` bump) rather than app code.

## No fixture for a pre-`EXEC_TRANSPORT_MIN_IMAGE_VERSION` sandbox in the `standard` e2e tier

`expose-port.spec.ts`'s coverage for the `!exec_capable` (publish+relay) path currently relies on
manually rolling back `EXEC_TRANSPORT_MIN_IMAGE_VERSION` or hand-creating a sandbox from an older
pinned base-image tag — there is no automated fixture that pins an old image version and exercises
both reachability models side by side in one `standard` run. Worth adding if the publish+relay path
regresses silently in the future; skipped here since nothing today forces a version mismatch to
exist during normal development.

## Per-sandbox SSH keys and `Proxied` aliases are never cleaned up

`destroy`/`remove_remote` (`src-tauri/src/backends/mod.rs`) do not remove
`app_data_root(app)/ssh-keys/<name>/` or call `crate::ssh::remove_alias(name)` when a sandbox is
destroyed. This mirrors the pre-existing gap for `Direct` aliases (`~/.ssh/sandbox-mio/config`
already accumulates stale blocks for destroyed connections, with no existing cleanup precedent) and
is small, worktree-isolated, and harmless to leave behind. Add cleanup to `destroy`/`remove_remote`
if this becomes a practical problem — `remove_alias` already exists but has no caller yet.
