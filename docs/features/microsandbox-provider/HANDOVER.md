# Microsandbox Provider — Handover

Written 2026-08-11 at the end of an implementation session, for whoever picks this up next (a new
chat session with no memory of this one). Read `01_ideas.md` and `02_dev.md` first — this document
is a pointer into them plus session-specific state that isn't captured there: what's actually
verified, what's uncommitted, and what to check against the real `msb` binary before trusting the
next phase's design.

## Where things stand

Branch `attempt-micro`. **Nothing in this feature is committed yet** — `docs/features/
microsandbox-provider/` is untracked and the six source files below are modified-but-unstaged. Run
`git status` / `git diff --stat` before doing anything destructive.

Phases A–D (the Podman-only `MachineProvider` extraction, unrelated to this session) were already
merged before this session started (commit `8a153de`). This session added the plan for Phases E–J
(the actual microsandbox provider) to `02_dev.md` and implemented E and F:

| Phase | Status | What |
|---|---|---|
| E | **Implemented, verified** | `ProviderCapabilities`, `ProviderId`, `MachineRef` types (`src-tauri/src/provider/types.rs`), `PodmanProvider::capabilities()` |
| F | **Implemented, verified** | `SystemMsbRunner`/`ScriptedMsbRunner` mock seam, `msb_command()`, `derive_msb_key`, `KNOWN_CALLS` (`src-tauri/src/mockctl/`), `MSB_HOME`/`MSB_PATH` e2e isolation (`e2e/lib/run-identity.ts`, `e2e/wdio.conf.ts`) |
| G | Implemented | `MsbProvider` driving the real `msb` CLI (lifecycle, exec, base image) |
| H | Implemented (standalone) | Disk-volume workspace + Mutagen sync for microsandbox |
| I | Implemented | Provider-neutral machine readiness + welcome-flow UI (additive, not a `MachineManager` narrowing — see `02_dev.md`) |
| J | Implemented | `MachineRef` threading, `default_provider` setting, frontend picker |

(This table was last accurate as of the session that wrote Phase F; see `02_dev.md` for the
authoritative, up-to-date phase status — G through J have since landed.)

Verified this session: `cargo check`, `cargo test --lib provider::` (4/4 passing), `./build.exe e2e
light` (11/11 specs passing) — run twice, once after Phase E and once after Phase F, both clean.

A `TaskList` exists with one task per phase (E–J); E and F are `completed`, G–J are `pending`. Pull
it up (`TaskList`) at the start of the next session rather than re-deriving the phase breakdown.

## Files touched this session

```
docs/features/microsandbox-provider/02_dev.md   — the actual build plan; read this in full
src-tauri/src/provider/types.rs                 — ProviderCapabilities, ProviderId, MachineRef (+ 4 unit tests)
src-tauri/src/provider/mod.rs                   — PodmanProvider::capabilities()
src-tauri/src/mockctl/types.rs                  — SystemMsbRunner, ScriptedMsbRunner, TrackedMsbInstance
src-tauri/src/mockctl/mod.rs                    — msb_command(), derive_msb_key, KNOWN_CALLS msb.* entries
e2e/lib/run-identity.ts                         — MSB_HOME, MSB_PATH constants
e2e/wdio.conf.ts                                — sets MSB_HOME/MSB_PATH env vars at module scope
```

Everything added is inert/unconstructed outside its own module (`#[allow(dead_code)]`, matching the
codebase's existing convention for a not-yet-wired variant like `SyncStrategy::Volume`) — Phase E/F
made **no behavioural change** to the running app. That's why `./build e2e light` staying green is
meaningful but not a strong signal on its own; the real test of this code starts at Phase G, once
something actually calls it.

## Before writing Phase G, verify against the real binary

`02_dev.md`'s Phase G section is written from three spike documents
(`spike/hands-on-cli.md` — executed against real `msb` 0.6.6; `spike/cli-surface.md` and
`spike/sdk-daemon-ssh.md` — desk research against docs/source, not executed). Where they
disagree, `01_ideas.md`'s own rule is "the executed result is treated as authoritative," and
`02_dev.md` already follows that rule (e.g. keeping `podman save`/`msb load` over bootstrap-and-
snapshot, despite `sdk-daemon-ssh.md` suggesting snapshots work — that suggestion was never tested).

That said, several Phase G specifics were never executed at all and are worth confirming against
`msb.exe` on this machine (`C:\Users\krist\.microsandbox\bin\msb.exe` per the spike) before
committing to exact argv in code:

- **`msb create`'s workspace-volume flags.** `01_ideas.md` says a `--kind disk` named volume; the
  exact create-time flag to attach it (and whether it's a flag on `create` itself or a separate
  `msb volume create` + reference) isn't nailed down in any spike.
- **`msb inspect --format json`'s label path.** `spike/cli-surface.md` line ~186 says labels live at
  `config.spec.labels`, but this was read from source, not printed and eyeballed.
- **The detached-exec shell wrapper's survival.** Resolved Decision 4 in `02_dev.md` picks
  backgrounding inside the guest shell (`sh -c '... & disown'`), but `spike/cli-surface.md` flags a
  2026-07-05 changelog entry — "`msb exec` now signals the full guest process group" — as a direct
  warning this may not survive the parent exec returning. This is the one Resolved Decision most
  likely to need revisiting once tested for real; the fallback (`msb create --script`) is noted in
  the gotchas table if the wrapper doesn't hold up.
- **`msb ssh serve <name> --stdio` as a `ProxyCommand`, and its per-sandbox host key.** Design is
  solid on paper (confirmed independently by both `spike/sdk-daemon-ssh.md`'s source read and
  `01_ideas.md`'s executed SSH test), but the exact `SshAlias::Proxied { command, known_hosts, ... }`
  values it needs were never assembled and run end-to-end.

None of these are blockers — they're the reason `02_dev.md`'s Phase G verify step calls for real
`msb`-backed light specs, not just mocked ones. Spend 10–15 minutes running the four items above by
hand against the installed binary before writing `src-tauri/src/provider/msb.rs`; it'll save a
rewrite if any argv guess is wrong.

**Update 2026-08-11 (second session): all four verified by hand against `msb.exe` 0.6.6.** Full
results in the new addendum at the bottom of `spike/hands-on-cli.md`. Summary:

- **Volume flags** — confirmed as written: `msb volume create <name> --kind disk --size 1G`, then
  `--mount-named <name>:/data:kind=disk,size=1G` at `create` time. Read and `stat` on the mount both
  exit 0.
- **Label path** — confirmed `active_config.labels` (and mirrored at `config.labels`), not
  `config.spec.labels`.
- **Detached-exec wrapper — the one real finding.** The wrapper as written in `02_dev.md` Resolved
  Decision 4 (`sh -c '<cmd> ... & disown'`) exits 127: this image's `/bin/sh` is `dash`, which has no
  `disown` builtin. The `&` still backgrounds the process before the failing `disown` runs — the
  target process itself survives (confirmed reparented to PID 1 via `ps -ef`) — but `msb exec` would
  report the call as failed. **Fixed already**: `02_dev.md` Resolved Decision 4 now specifies `bash
  -c` instead of `sh -c` (bash is present in the base image at `/usr/bin/bash`); retested with `bash
  -c` and it exits 0 cleanly. Phase G's `MsbProvider::exec` must build the wrapper with `bash -c`, not
  `sh -c`.
- **SSH ProxyCommand** — confirmed end-to-end, including per-sandbox `known_hosts` pinning via
  `StrictHostKeyChecking accept-new`. One minor argv note: `msb ssh authorize` has no per-sandbox
  scope — `--file <pubkey>` appends to the single host-wide `authorized_keys`, no `<SANDBOX>`
  positional accepted.

Host left clean (verification sandbox, volume, and SSH key all removed at the end of the session).
Phase G can now proceed straight to implementation — no remaining unknowns block writing
`src-tauri/src/provider/msb.rs`.

## Open product question not yet raised with the user

`02_dev.md` Resolved Decision 1 (ship user-visible in v1) and Decision 2 (accept the base image's
Podman dependency in v1) were made unilaterally this session as reasonable defaults consistent with
`01_ideas.md`'s own reasoning, not confirmed with the user. Worth a one-line check-in before Phase J
ships a picker — cheap to confirm, expensive to build a picker for a feature the user wanted gated.

## Suggested next steps, in order

1. Skim `02_dev.md`'s new "Phase E onward" section (search for that heading) — it's the full plan,
   Resolved Decisions, and gotchas table for what's left.
2. Spend the 10–15 minutes above confirming the four Phase G unknowns against the real `msb` binary.
3. Implement Phase G (`src-tauri/src/provider/msb.rs`), updating `KNOWN_CALLS`/`ScriptedMsbRunner`
   as real argv shapes solidify.
4. Re-run `cargo check`, `cargo test`, `./build e2e light` after Phase G — this is the first phase
   where a regression is actually possible, since Phase G is the first thing that constructs and
   calls the Phase F scaffolding.
