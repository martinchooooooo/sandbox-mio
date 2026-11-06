---
type: dev-plan
title: "Agent Web UI Dev"
feature: agent-web-ui
---
# Agent Web UI Dev

This is the build plan for embedding [Paseo](https://paseo.sh) as a third per-sandbox pane —
"Agents" — alongside the existing Terminal and Dashboard panes. It continues `01_ideas.md`'s
chosen model (one Paseo pane, one new `AppKind` unit variant, one new published port) and
supersedes none of it; the ideas document's "superseded material" (per-agent `opencode web` /
`jmfederico/pi-web` panes) stays rejected here too. Phases A–D are all implemented (see each phase
section for implementation-time adjustments).

## The concrete model

A fourth container-side process, Paseo, is installed **through mise into the workspace manifest**
(`mise use node@lts npm:@getpaseo/cli`, run idempotently on every "Agents" open) rather than baked
into the base image — workspace `mise.toml`/`mise.lock` entries are what `reproduce_remote`
reinstalls on a remote deploy, so an image-baked paseo would silently vanish there. Only `procps`
(the `ps` binary Paseo's own `daemon status`/`stop` shell out to) is baked, as an apt package.
Like Zellij, paseo is never started at container boot. The first time a sandbox's "Agents" tab is
opened, Rust idempotently starts
`paseo daemon start --foreground --listen 0.0.0.0:6767 --web-ui --hostnames true` inside the
container via a detached `podman exec -d`, publishes container port `6767` (a new
`PASEO_WEB_PORT` constant, sibling to `ZELLIJ_WEB_PORT = 8082`) through the same
`pasta:--host-lo-to-ns-lo` bridge and Mutagen-forward chain the Terminal pane already uses, and
embeds the forwarded URL as a new `AppKind::AgentWeb` content child webview.

Unlike Terminal and Dashboard (both embedded eagerly by `open_workspace` the moment a sandbox's
workspace opens), the Agents pane is embedded lazily: only when the switcher's "Agents" tab is
clicked for the first time for that sandbox. The switcher therefore needs to show an "Agents" tab
per open sandbox *before* any `AgentWeb` pane exists in the Rust-side registry — the tab's click
handler creates the pane on first use and switches to it on every use after.

Paseo requires a password (`PASEO_PASSWORD`) for non-Basic-Auth network exposure, set once at the
container's first `daemon start` and un-changeable for the life of that running daemon. The
`01_ideas.md` spike never found a password→token *exchange endpoint* because there isn't one to
find: the web client authenticates every HTTP/WebSocket request with `Authorization: Bearer
<password>` directly (confirmed via `paseo.sh/docs/security`), and remembers a connection across
reloads by writing it — password included — into a `localStorage` key the client itself owns,
`@paseo:daemon-registry` (reverse-engineered from the minified web bundle; see Phase D). Seeding
that key before the SPA boots reproduces exactly what a completed manual login leaves behind, so
v1 ships **with** scripted auto-login after all — Phase D below, not deferred.

## Resolved Decisions

Restating every Open Question from `01_ideas.md`:

- **The Paseo browser login flow is unscripted.** Resolution: **cleared** (Phase D). A live spike
  against a real running sandbox (Playwright/Chromium against the daemon's published port) found
  that the client's `HostRuntime.loadFromStorage()` reads a `@paseo:daemon-registry` localStorage
  entry (host profile + `directTcp` connection, password inline) on boot, before it ever shows a
  login screen. `prepare_agent_web_pane` (`src-tauri/src/lib.rs`) now seeds that key via
  `initialization_script`, confirmed end-to-end (screenshot-verified) to skip straight to the
  app's home screen with no login form. `AgentManager.svelte`'s manual password display was
  removed since it no longer has a purpose.
- **Whether Paseo's spawned agent subprocesses inherit `mise`-activated environment variables
  written after the daemon started.** Resolution: not re-tested further in this plan. Paseo and
  its node are `mise use`-installed into the workspace on every "Agents" open (implemented
  revision — nothing paseo-related is image-baked), so the concern reduces to OpenCode/Pi's
  per-*sandbox* local-LLM provisioning, which is written to the workspace too. Phase C's
  "Open Agents UI" action is a per-sandbox action already gated on the agent being installed
  (`installedAgents` in `AgentManager.svelte`), so by the time a user can click it for OpenCode/Pi,
  `provision_opencode`/`provision_pi` has already run — but the Paseo daemon itself may have been
  started (by an earlier "Agents" click, e.g. to reach Claude Code) *before* that provisioning
  wrote `mise.toml [env]`. Mitigation: `open_agent_web` restarts the Paseo daemon (`stop` then
  `start`, best-effort) whenever it is asked to open the pane for a sandbox where the daemon is
  already running, trading a few seconds of restart latency on every open for never serving a
  provider a stale environment. Revisit only if the restart proves unnecessary or too slow.
- **Fixed port literal and `AppKind` variant name.** Resolution: `PASEO_WEB_PORT = 6767` (Paseo's
  own default, confirmed reachable in the spike, no collision with `ZELLIJ_WEB_PORT = 8082`).
  Variant name: `AppKind::AgentWeb` — not `Paseo`, so the pane's Rust/TS-visible identity does not
  couple to one third-party tool's name if it is ever swapped.
- **Local-only or remote too?** Resolution: both, unchanged from Terminal's existing behavior. No
  restriction is added; `open_agent_web` accepts the same `connection` resolution
  `prepare_terminal_webview` already does (sync-session lookup, falling back to
  `runtime.local_connection()`).
- **Does "Open Agents UI" replace or sit alongside "Open Terminal"?** Resolution: sits alongside.
  Both access paths remain available per sandbox; the switcher grows a third tab
  (Terminal / Manage / Agents) rather than replacing either existing one.
- **`procps` and any other Paseo-expected CLI tools as new base-image dependencies.** Resolution:
  add `procps` to the `Containerfile` (confirmed needed). Other Paseo features (voice/speech model
  auto-download, worktree management, `gh`-dependent features) are not enabled by any flag this
  plan passes — `--web-ui --hostnames true` is the full flag set — so no further base-image
  dependency is pulled in by anything this plan does. If Paseo's speech-model auto-download still
  fires unconditionally on daemon boot (unconfirmed either way by the spike), that is a Phase A
  follow-up to check once the image is rebuilt, not a blocker for writing the phase.
- **Fallback commitment point.** Resolution: moot — the budgeted follow-up spike (Phase D) cleared
  the login-automation question on its first attempt, so the superseded per-agent plan was never
  needed.

## Implementation phases

### Implemented: Phase A — base image and port publishing

- `src-tauri/resources/Containerfile`: `procps` added to the `apt-get install` line — the only
  image-level change. Paseo itself is **not** baked (an earlier revision baked it plus a global
  node; reverted so remote deploys reproduce it from `mise.lock` like every other tool — see the
  `mise use` step in Phase B). `BASE_IMAGE_VERSION` bumped `2` → `3` — any sandbox created before
  this ships lacks `procps` and the published port and needs to be re-created (same caveat as
  `ZELLIJ_WEB_PORT`, already documented in Common Gotchas).
- `src-tauri/src/backends/mod.rs`: `const PASEO_WEB_PORT: u16 = 6767;` added next to
  `ZELLIJ_WEB_PORT`. Both `create()` (local) and `deploy()`'s remote container creation pass a
  second `--publish 127.0.0.1::{PASEO_WEB_PORT}` argument alongside the existing Zellij one —
  two `--publish` flags total, not a combined port range.
- `src-tauri/src/runtime/mod.rs`: the matching `const PASEO_WEB_PORT: u16 = 6767;` added (mirrors
  the existing doc-comment cross-reference between the two files' `ZELLIJ_WEB_PORT` constants).

### Implemented: Phase B — Rust: `AppKind::AgentWeb`, `prepare_agent_web`, `embed_agent_web`

Built as planned with three implementation-time adjustments, each noted below:

- `src-tauri/src/workspace/types.rs`: `AgentWeb` added as a third unit variant of `AppKind`
  (`Copy` intact; wire value `"agentweb"`). `AppPane` also gained a `workspace_path: String`
  field (the Phase C "extend `AppPane`" option — chosen over a switcher-side sandbox lookup),
  threaded through `embed_terminal`/`embed_dashboard`/`embed_agent_web` from the commands, which
  all already receive `workspace_path`.
- `src-tauri/src/runtime/types.rs`: `prepare_agent_web(&self, name, connection, password: &str)
  -> Result<u16, RuntimeError>`. **Adjustment 1 (password direction):** the password is passed
  *in* rather than returned — `lib.rs` owns `.mio/paseo.json` and reuses the persisted password
  across the always-restart, resolving the Lifecycle section's open decision in favour of "mint
  once per sandbox, reuse on every restart" (better UX; `paseo daemon start` just reads
  `PASEO_PASSWORD` from its environment each start, so reuse needs nothing special from Paseo).
  A new `RuntimeError::AgentWebUnavailable` variant carries the failure messages (no `AppError`
  / `errors.ts` change needed — it rides the existing transparent `Runtime` kind).
- `src-tauri/src/runtime/mod.rs`: implemented. `prepare_agent_web` first runs an idempotent
  `mise use node@lts npm:@getpaseo/cli` in the workspace (the install step that replaced the
  Containerfile bake — paseo and its node land in `mise.toml`/`mise.lock`, so `reproduce_remote`
  reinstalls them on remote deploys; the first open per workspace pays the download, and the two
  entries appear in the packages list UI like any other tool). **Adjustment 2 (no status
  check):** the planned
  step-2 `paseo daemon status` substring match is dropped entirely — since step 3 stops any
  running daemon anyway, the implementation just runs a best-effort `mise exec -- paseo daemon
  stop` (ignored failure when none is running) and then always starts fresh, avoiding any
  guessed status wording. **Adjustment 3 (mise exec):** the daemon is started as `podman exec -d --workdir
  /root/work --env PASEO_PASSWORD=<pw> <name> mise exec -- paseo daemon start --foreground
  --listen 0.0.0.0:6767 --web-ui --hostnames true` — the `mise exec` wrapper (not in the plan's
  literal command) puts workspace tools (opencode, pi) on PATH and applies `mise.toml [env]`, so
  provider discovery and the spawned-agent environment both work under `podman exec`'s
  non-interactive shell (which never reads `.bashrc`). Port read via `podman port <name>
  6767/tcp` + `parse_host_port`, as planned.
- `src-tauri/src/runtime/mock.rs`: `prepare_agent_web` returns `Ok(0)` (port only, per the
  adjusted signature).
- `src-tauri/src/lib.rs`: `prepare_agent_web_pane(...) -> Result<TerminalPane, AppError>`
  (`agentweb-{name}` forward, `.mio/paseo.json` persistence via the small `read_paseo_password`
  helper), plus the `open_agent_web` command (switch-or-embed guard keyed on
  `kind == AgentWeb`), registered in `invoke_handler`. The command originally returned the
  password for Phase C's manual login; Phase D's auto-login removed every consumer, so it now
  returns `()`. The AgentWeb forward is *not* inserted into `ForwardPortMap` (that map is
  sandbox-keyed and already owned by the Terminal forward); teardown goes through
  `close_app`'s per-pane `forward_name` instead.
- `src-tauri/src/workspace/mod.rs`: `embed_agent_web` added; first-open **does** activate
  immediately (the user just clicked the tab), so it follows `embed_terminal`'s active-pane
  bookkeeping — the two now share one `embed_external` helper rather than duplicating the body.
  An empty `init_script` is skipped instead of injected as a no-op.

### Implemented: Phase C — frontend: switcher tab and "Open Agents UI" action

- `src/routes/workspace/switcher/+page.svelte`: local `AppKind` union widened to `"terminal" |
  "dashboard" | "agentweb"` (also in `src/lib/workspace/state.svelte.ts`, the other copy of the
  pane types). A synthetic per-sandbox "Agents" tab renders after the real panes whenever no
  `agentweb` pane exists yet; its `onclick` calls `invoke("open_agent_web", { name, workspacePath })`
  and ignores the returned password (AgentManager is the display surface). Once the pane exists
  it renders like the other two (`activate` → `switch_pane`). `workspacePath` is carried by the
  chosen "extend `AppPane`" option: Rust's `AppPane` gained a `workspace_path` field (Phase B),
  so the switcher reads it from the sandbox group's first pane.
- `src/routes/projects/AgentManager.svelte`: a single sandbox-level "Open Agents UI" button
  (shown when at least one agent is installed) calls `open_agent_web` through the shared action
  runner. Originally also surfaced the returned password inline (select-all `<code>` + Copy
  button) for a one-time manual paste into Paseo's login screen — **removed in Phase D** once
  auto-login made that paste unnecessary; the password now stays inside
  `prepare_agent_web_pane`, which seeds the login with it. Strings are hard-coded
  English with `TODO(i18n)` markers pending the post-approval i18n pass.

### Implemented: Phase D — scripted auto-login

No password→token exchange endpoint exists to find — `paseo.sh/docs/security` confirms the web
client sends the raw password as `Authorization: Bearer <password>` on every request once it knows
one. What remained undocumented was how the client *remembers* a connection across page loads. A
live spike (Playwright/Chromium driven against an already-running sandbox's published Paseo port,
no Tauri app needed) found the answer in the minified web bundle
(`_expo/static/js/web/index-*.js`): on boot, `HostRuntime.loadFromStorage()` reads a JSON array
from the `@paseo:daemon-registry` localStorage key — a list of host profiles, each with one or
more `directTcp` connections shaped `{ id, type: "directTcp", endpoint, useTls, password }` — and
connects to each stored host immediately with no login screen.

**Correction (post-spike, debugged against paseo 0.1.103):** the spike's original conclusion that
`serverId` "doesn't need to match the daemon's real one" was wrong, and a placeholder id makes the
pane fail with "Connection timed out". After an otherwise-successful, authenticated connect the
client compares the daemon's reported `serverId` against the stored profile's and closes the
connection on mismatch (`Connection resolved to <real>, expected <stored>.`). The only exemption
is a stored id starting with `local:` (the desktop app's in-memory local-daemon flow), and
`loadFromStorage()` deletes `local:`-prefixed profiles from storage on load — so no placeholder
survives; the real id must be known before seeding. It is public: `GET /api/status` on the daemon
returns `{"status":"server_info","serverId":"srv_…","hostname":"…", ...}`.
`fetch_paseo_server_info` (`src-tauri/src/lib.rs`) fetches this through the Mutagen forward
(short retry loop — the forward listener may lag `forward_create`) and the registry entry is
seeded with that `serverId` and the reported `hostname` as label, matching byte-for-byte what a
completed manual login writes.

`prepare_agent_web_pane` (`src-tauri/src/lib.rs`) builds this exact structure — a single
connection with `endpoint: "localhost:{windows_port}"` (the client normalizes stored loopback
endpoints to `"localhost"` and recomputes connection ids to match, so a `127.0.0.1` entry only
survives as a redundant duplicate), `password` from the same `.mio/paseo.json` value already
threaded through — and serializes it into an `initialization_script` that calls
`window.localStorage.setItem('@paseo:daemon-registry', ...)` before the Paseo SPA boots. Verified
end-to-end: seeding this key against a real running sandbox's published port (via Playwright,
outside the Tauri app) skips straight to Paseo's home screen; without it, the same origin shows the
"Direct Connection" / "Paste pairing link" welcome screen. `AgentManager.svelte`'s manual password
display (Phase C) is removed, and `open_agent_web` no longer returns the password — the pane never
needs it typed in by hand, so no UI consumer remains.

## Lifecycle / cross-cutting concerns

- **Teardown.** `workspace::close_app` (`src-tauri/src/workspace/mod.rs:372-409`) already iterates
  every pane of a sandbox and terminates the Terminal pane's forward by `kind == AppKind::Terminal`
  check; extend that check to also terminate the `AgentWeb` pane's forward
  (`crate::sync::forward_terminate` keyed by its own `forward_name`), the same way Dashboard is
  correctly excluded today (it has no forward). The Paseo daemon process itself is **not** stopped
  on pane close — it stays running in the container (self-supervising, per `01_ideas.md`) so
  re-opening the Agents tab later is instant; only the Mutagen forward and the webview are torn
  down. This mirrors Zellij's own lifecycle (the `zellij` daemon also outlives a closed Terminal
  pane).
- **Sandbox destroy/undeploy.** No new code needed: destroying the container kills the Paseo
  daemon with it, and `close_app`-driven forward teardown already runs before/around destroy in
  the existing sandbox-removal flow.
- **`.mio/paseo.json`.** Written host-side next to the existing `.mio/zellij.json`, same rationale
  (`docs/techdebt.md` workaround: avoids shell-escaping JSON through `podman exec`, the workspace
  is already bind-mounted). Holds `{ "password": ... }`; unlike `zellij.json`'s token
  (single-use, re-minted every open), the password is stable for the container's lifetime and is
  re-read (not re-generated) whenever `open_agent_web` finds the daemon already running for reasons
  other than the environment-refresh restart in Phase B step 3. **Resolved at implementation
  time:** the password is minted once per sandbox (first `open_agent_web`) and reused on every
  subsequent restart — `lib.rs` reads it back from `.mio/paseo.json` and passes it into
  `prepare_agent_web`, so the environment-refresh restart never changes the password the user
  was shown.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A newly created sandbox has no "Open Agents UI" tab even after this feature ships | Container ports are `--publish`ed only at `podman create` time; existing containers never gain `PASEO_WEB_PORT` | Re-create the sandbox (destroy + recreate) to pick up the new `--publish` argument, same as the existing `ZELLIJ_WEB_PORT` gotcha |
| The embedded Agents pane shows the "Direct Connection" / "Paste pairing link" welcome screen instead of auto-logging in | The seeded `@paseo:daemon-registry` entry failed `normalizeStoredHostProfile` validation (empty `serverId`, no parseable connections) and was discarded on load, or the `initialization_script` didn't run before the SPA's first paint | Confirm `prepare_agent_web_pane` built the endpoint as `localhost:{windows_port}`; re-verify with the Playwright spike technique (seed the same localStorage key against the daemon's published port directly, outside the Tauri app) before assuming it's a Paseo-version regression |
| The embedded Agents pane loads but every connection attempt ends in "Connection timed out", while pasting the same endpoint + password into the welcome screen works | The seeded `@paseo:daemon-registry` entry's `serverId` doesn't match the daemon's real one; the client closes the (successfully authenticated) connection on mismatch, so the failure masquerades as a network timeout | Seed the real id — `fetch_paseo_server_info` gets it from `GET /api/status` before the registry entry is built; check that call didn't regress before suspecting the forward |
| `paseo daemon start` binds `127.0.0.1` and shows no web UI even though the process started fine | The defaults are loopback-only with the bundled UI disabled | Always pass `--listen 0.0.0.0:6767 --web-ui --hostnames true` explicitly, exactly as `01_ideas.md`'s spike found |
| `paseo daemon status`/`stop` crashes with `spawn ps ENOENT` | The base image lacks `procps` until Phase A adds it | Confirm `procps` landed in the `Containerfile` and `BASE_IMAGE_VERSION` was bumped; a sandbox on the old image needs re-creating |
| An agent launched through Paseo doesn't see the local-LLM provider even though `provision_pi`/`provision_opencode` already ran | The Paseo daemon may have cached its spawn environment from before provisioning was written | `open_agent_web` restarts the daemon on every open (Phase B step 3) specifically to avoid this; if it still happens, the restart-on-open logic regressed |
| A second `paseo daemon start` fails with "another Paseo daemon is already running" even after `kill -9`-ing the process you saw | Paseo runs a self-restarting supervisor in addition to the daemon child | Use `paseo daemon stop` (not a manual kill) before restarting, or rely on the idempotent status check in Phase B step 2 |
