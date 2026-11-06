---
type: dev-plan
title: "Workspace App Tabs Build Plan"
feature: workspace-app-tabs
---
# Workspace App Tabs Build Plan

This document is the build plan for reshaping the embedded workspace's tab model, continuing and superseding `01_ideas.md`. It commits the ideas document's proposed model to concrete seams: opening a sandbox embeds and activates **only** the Manage pane (Terminal stops being embedded eagerly and becomes a launcher-triggered lazy pane like Agents), the Manage route gains an app launcher, the switcher becomes a two-level per-sandbox block (name row over app-tab row with a home-icon Manage tab), and a new `close_pane` command closes exactly one app tab while its siblings and the Manage tab stay open. It also adopts the ideas document's spike outcome: the terminal webview URL must pin a stable per-sandbox session name so a closed-and-reopened Terminal tab reattaches to the same Zellij session. **Status: nothing in this plan is implemented yet** — all phases below are Remaining. The plan calls out three refactors that shrink the final code: one app catalogue driving both the switcher and the launcher (subsuming the duplicated `paneTitle` and the standalone "Open Agents UI" button), a shared `embed_external`-style activate-on-create path reused by the Manage pane, and a `close_app`/`close_pane` split over shared teardown helpers.

## The Concrete Model

### Backend pane lifecycle

`open_workspace` (`src-tauri/src/lib.rs:405`) embeds and activates the Manage pane only. It drops the eager `prepare_terminal_webview` + `embed_terminal` calls (`src-tauri/src/lib.rs:430-432`); the sole first-open embed is `embed_dashboard`, which is changed to **activate** the pane it creates rather than embedding it hidden. A user who opens a sandbox to install a package never pays the Zellij-start cost.

Terminal becomes a lazy pane reached through a new `open_terminal_pane` command (`src-tauri/src/lib.rs`), a structural copy of `open_agent_web` (`src-tauri/src/lib.rs:343`): if a `AppKind::Terminal` pane exists for the sandbox, `switch_pane` to it; otherwise run `prepare_terminal_webview` and `embed_terminal`. `open_terminal` (the standalone window, `src-tauri/src/lib.rs:370`) is untouched except for the shared session-name URL change described below.

`close_pane(name, kind)` (`src-tauri/src/lib.rs`, backed by `workspace::close_pane`, `src-tauri/src/workspace/mod.rs`) closes exactly one pane: remove its `AppPane`, close its webview, tear down only its forward, re-activate the sandbox's Manage pane, and re-layout. It never targets `AppKind::Dashboard`. `close_workspace`/`close_app` stays the whole-sandbox teardown for undeploy/destroy/Destroyed (`src-tauri/src/lib.rs:458,546,585,598`, `src-tauri/src/workspace/mod.rs:485`).

### App catalogue as the single source of truth

A new module `src/lib/workspace/apps.ts` exports a typed list of launchable app descriptors, each carrying: the `AppKind` discriminant, a display title, a `@hugeicons/core-free-icons` icon, the `open_*` command name to invoke, and an `available(ctx)` predicate. The two entries are **Terminal** (`available: () => true`) and **Agents** (`available: (ctx) => ctx.installedAgents.length > 0`, the exact predicate `AgentManager.svelte:35` already computes). Both the switcher (icon + title per tab) and the Manage launcher (which apps to list, open-vs-switch state) read this one module, so `paneTitle` (duplicated today in `switcher/+page.svelte:51`) and the ad-hoc "Open Agents UI" button (`AgentManager.svelte:170,248-256`) are both replaced by catalogue-driven rendering. Manage/Dashboard is **not** a catalogue entry: it is the always-present home surface, rendered specially (home icon, no launcher button, no per-tab close).

### Two-level switcher

`src/routes/workspace/switcher/+page.svelte` renders, per sandbox group (`bySandbox`, `switcher/+page.svelte:20`), two stacked rows: a top name row (sandbox name plus the whole-sandbox close `×` that calls `close_workspace`) and a bottom tab row. The bottom row always leads with a home-icon Manage tab (no `×`), followed by one tab per non-Dashboard pane that exists; each of those carries a per-tab `×` calling `close_pane`. Tab icons and titles come from `src/lib/workspace/apps.ts`. The taller two-row block requires `SWITCHER_H` (`src-tauri/src/workspace/mod.rs:33`) to grow from 40; the plan starts at **64** and confirms empirically (see Phase D).

### Manage launcher

`src/routes/workspace/dashboard/+page.svelte` gains a launcher section above the existing `PackageManager`/`AgentManager`/`PortsManager` stack. It iterates `src/lib/workspace/apps.ts`, filtered by each descriptor's `available(ctx)` where `ctx` carries the sandbox's `installedAgents` (computed here the same way `AgentManager.svelte:35` does). For each available app it renders a button that reads open-vs-switch from the shared store (`workspaceApps()`, `src/lib/workspace/state.svelte.ts:30`) — "Open" if no pane of that kind exists for the sandbox, "Switch to" if one does — and invokes the descriptor's command (`open_terminal_pane` / `open_agent_web`), both of which are embed-or-switch and so are safe in either state.

## Resolved Decisions

1. **Which apps are launchable, and where each availability predicate lives.** Terminal (always) and Agents (`installedAgents.length > 0`) are the launchable set; user-exposed ports stay external-browser links in `PortsManager.svelte` and do **not** become app tabs (this also sidesteps the same-`AppKind` label collision, Decision 6). The source of truth is a new catalogue module `src/lib/workspace/apps.ts` holding each app's kind, title, icon, command, and `available()` predicate; a future app registers by adding one entry, mirroring `src/lib/agents/catalogue.ts`.

2. **Whether the Manage/Home tab is closable.** No. The home tab has no `×` and `close_pane` never accepts `AppKind::Dashboard`. Closing the whole sandbox is a separate affordance: the `×` on the switcher's per-sandbox **name row**, which keeps calling `close_workspace`.

3. **What happens when the last non-Home app tab is closed.** The sandbox stays open on its Manage tab and does not auto-close. `close_pane` re-activates the Dashboard pane and the global panel stays narrow, because the registry still holds that sandbox's Dashboard `AppPane` (so `apps.is_empty()` is false); only `close_workspace` returns the panel to full-bleed.

4. **Cost of re-launching a closed Terminal (session persistence).** Adopted from the spike: `prepare_terminal_webview` (`src-tauri/src/lib.rs:137`) must build the URL as `http://127.0.0.1:{windows_port}/{session}` instead of `/`, where `{session}` is a stable per-sandbox name so `/ws/terminal/{session}` reattaches to the same Zellij session on reopen. The stable name is the **sandbox name** (already unique per sandbox and already the pane key everywhere else); it is URL-path-escaped when interpolated. Because `open_terminal` (standalone) and `open_terminal_pane` (embedded) share `prepare_terminal_webview`, the fix applies to both paths at once — no separate change to `open_terminal`.

5. **New `SWITCHER_H` value.** Start at 64 logical px (two ~28px rows plus padding) in `src-tauri/src/workspace/mod.rs:33` and confirm no clipping/overlap against the new two-row markup during Phase D; adjust the constant if the rendered block needs more. It flows through `workspace_layout` and the existing geometry unit tests (`src-tauri/src/workspace/mod.rs:612-624`), which reference `SWITCHER_H` symbolically and so keep passing after the value change.

6. **Multiple instances of the same app kind.** Deferred by scoping the launcher to one-per-kind apps (Terminal, Agents), each representable under the existing `workspace-{name}-{suffix}` label scheme (`src-tauri/src/workspace/mod.rs:170-175`). Exposed ports — the only many-per-kind candidate — stay non-tabs (Decision 1), so no per-instance label discriminator is added.

7. **Grouping layout when several sandboxes are open.** Each sandbox renders its own two-row block (name row over tab row) laid out horizontally in the switcher bar, as today's flat grouping already does per sandbox. Narrow-width behaviour of multiple blocks is a CSS concern confirmed visually in Phase D, not a new mechanism.

## Implemented: Phase A — Backend pane lifecycle

Touches `src-tauri/src/lib.rs` and `src-tauri/src/workspace/mod.rs`.

- **`open_workspace` (`src-tauri/src/lib.rs:405`):** remove the `prepare_terminal_webview` + `embed_terminal` calls (`:430-432`); on first open call only `embed_dashboard`. The `exists`/`switch_app` early-return (`:424-428`) and the `watch_ports_file` bootstrap (`:417-422`) stay. Update the command doc comment (`:400-403`) to say it embeds and activates the Manage pane.
- **`embed_dashboard_on_main` (`src-tauri/src/workspace/mod.rs`):** now **activates** the created pane instead of hiding it, via a shared helper `activate_new_pane(window, registry, pane: AppPane, rects: WorkspaceRects)` extracted from `embed_external_on_main`'s previous inline push/take/insert block; both call sites now push the pane, swap `active_app`, update `sandbox_last_pane`, set `rects`, and hide whichever pane was previously active. Doc comment updated to state the pane becomes active on creation.
- **`open_terminal_pane` (new, `src-tauri/src/lib.rs`):** structural copy of `open_agent_web` with `AppKind::Terminal`, calling `prepare_terminal_webview` + `embed_terminal`. Registered in `generate_handler!`, next to `open_workspace`.
- **`switch_app_on_main` fallback (`src-tauri/src/workspace/mod.rs`):** no-record default changed from `AppKind::Terminal` to `AppKind::Dashboard`, the pane every open sandbox is guaranteed to have.
- **Verify:** `cargo check` / `cargo test` in `src-tauri/` — both pass; the geometry tests are unaffected.

## Implemented: Phase B — `close_pane`

Touches `src-tauri/src/workspace/mod.rs` and `src-tauri/src/lib.rs`.

- **Refactored `close_app` into shared teardown.** `close_app`'s per-kind forward teardown loop was extracted into `teardown_forwards(app, name, panes: &[AppPane])`, now called by both `close_app` (whole sandbox) and `close_pane` (one pane). `close_app_on_main` was already isolated and needed no change; it is reused by `close_pane` with a one-element pane-label vec.
- **`close_pane(name, kind)` (`src-tauri/src/workspace/mod.rs`):** selects and removes the single `AppPane` matching `(sandbox == name, kind)`; no-ops if `kind == AppKind::Dashboard` (home tab not closable, Decision 2) or no matching pane exists. If the removed pane was `active_app`, sets `active_app` to the sandbox's Dashboard pane label and records it in `sandbox_last_pane` (re-activates Manage, Decision 3). Does not remove the `sandbox_last_pane` entry and does not treat the sandbox as empty. Reuses `close_app_on_main` and `teardown_forwards` for the one removed pane.
- **`close_pane` command (`src-tauri/src/lib.rs`):** async, `name: String, kind: AppKind, app: AppHandle`, calls `workspace::close_pane(...).await`, mirroring `close_workspace`. Registered in `generate_handler!` next to `close_workspace`.
- **Verify:** `cargo check` / `cargo test` in `src-tauri/` — both pass.

## Implemented: Phase C — Terminal session-name URL

Touches `src-tauri/src/lib.rs`.

- **`prepare_terminal_webview`:** builds `url` via `tauri::Url::parse` + `path_segments_mut().push(name)`, yielding `http://127.0.0.1:{windows_port}/{session}` with `{session}` a URL-path-escaped form of the sandbox `name` (Decision 4; `url`'s `path_segments_mut` percent-encodes the pushed segment). The persisted `.mio/zellij.json` `url` and the init script's `location.replace(location.pathname + location.search)` both already key off the same `url`/`location.pathname`, so pinning the path required no init-script edit. `open_terminal` (standalone) shares `prepare_terminal_webview`, so the fix applies to both paths.
- **Verify:** `cargo check` / `cargo test` in `src-tauri/` — both pass; behavioural reattach confirmation is deferred to `03_e2e.md` / the `standard` tier (needs real Podman).

## Implemented: Phase D — Frontend catalogue, switcher, launcher

Touches `src/lib/workspace/apps.ts` (new), `src/lib/workspace/state.svelte.ts`, `src/routes/workspace/switcher/+page.svelte`, `src/routes/workspace/dashboard/+page.svelte`, and `src/routes/projects/AgentManager.svelte`.

- **`src/lib/workspace/apps.ts` (new):** exports `LAUNCHABLE_APPS`, a typed array of `{ kind: AppKind; title: string; icon; command: string; available(ctx): boolean }` with the Terminal (`open_terminal_pane`, always available) and Agents (`open_agent_web`, `installedAgents.length > 0`) entries (Decision 1). `HOME_APP` holds the home-tab metadata (title + `Home01Icon`) separately since Dashboard is not launchable. Icons from `@hugeicons/core-free-icons` (`TerminalIcon`, `BotIcon`, `Home01Icon`).
- **`src/lib/workspace/state.svelte.ts`:** added `sandboxHasPane(name, kind)` so the launcher derives open-vs-switch without re-deriving from `workspaceApps()` inline; the single `workspace://state` subscription is unchanged.
- **`switcher/+page.svelte`:** rewritten as the two-level block (Model §Two-level switcher): a name row (sandbox name + whole-sandbox `×` calling `close_workspace`) over a tab row (home-icon Manage tab from `HOME_APP`, then one tab per `LAUNCHABLE_APPS` entry that has an open pane, each with its own `×` calling `close_pane`). The local `paneTitle` and the inline "open Agents" button are gone, subsumed by the catalogue and the Manage launcher.
- **`dashboard/+page.svelte`:** added a launcher section above the existing `PackageManager`/`AgentManager`/`PortsManager` stack, computing `installedAgents` the same way `AgentManager.svelte` does (loads `packages` via `list_packages` in `onMount`) and passing it as `LAUNCHABLE_APPS`'s `available()` context. Each launcher button is the shadcn `Button` component (`loading` prop disables + `aria-busy`s it while its own `launcherAction.run(app.kind, ...)` is in flight, all other launcher buttons disabled meanwhile) and swaps its icon to a spinning `LoaderPinwheelIcon` (the `StatusRow.svelte` spinner idiom) for the duration — `prepare_terminal_webview`/`prepare_agent_web_pane` are the slow first-launch path, so the spinner covers that wait. Shows "Switch to"/"Open" from `sandboxHasPane`.
- **`AgentManager.svelte`:** removed the "Open Agents UI" button block and its `openAgentsUi` handler; the launcher subsumes it.
- **`SWITCHER_H` confirmation:** set to 64 in `src-tauri/src/workspace/mod.rs`; visual clipping check against the live two-row markup is deferred to manual verification/`03_e2e.md` (no automated geometry assertion exists for the switcher's rendered DOM height, only the Rust-side rect unit tests, which still pass unchanged).
- **Verify:** `cargo check`/`cargo test` in `src-tauri/` pass; `pnpm check` (repo root) shows no new errors — the 5 pre-existing `svelte-check` errors in `src/lib/components/ui/command/` and `dialog/` reproduce identically on a clean tree and are unrelated to this feature; `pnpm run test:light` in `e2e/` run for the `light`-tier UI/store signal.

## Implemented: Phase E — i18n

Ran after user sign-off on Phases A–D's behaviour and wording (including the launcher's loading-spinner addition).

- Added `workspace_tab_manage`, `workspace_tab_terminal`, `workspace_tab_agents`, `workspace_launcher_open`, `workspace_launcher_switch`, `workspace_close_sandbox_label` (`{sandbox}` param), `workspace_close_tab_label` (`{title}` param) to `messages/en.json`, mirrored in `messages/de.json` and `messages/ru.json`.
- `src/lib/workspace/apps.ts`: `AppDescriptor.title`/`HOME_APP.title` changed from `string` to `() => string`, each entry bound directly to its `m.workspace_tab_*` function (explicit key→function map, not a dynamic `m[key]` lookup — `AGENTS.md` Common Gotchas). Consumers call `.title()`.
- `switcher/+page.svelte`: close aria-labels now call `m.workspace_close_sandbox_label({ sandbox })` / `m.workspace_close_tab_label({ title: app.title() })`; tab/home labels call `.title()`.
- `dashboard/+page.svelte`: launcher "Open"/"Switch to" text now calls `m.workspace_launcher_open()`/`m.workspace_launcher_switch()`.
- Final `grep` for `TODO(i18n)` across `src/lib/workspace/` and `src/routes/workspace/` returns no matches (the three remaining repo-wide hits are in `CreateSandboxItem.svelte`, pre-existing and out of scope for this feature).
- **Verify:** `pnpm check` — no new errors (the 5 pre-existing `svelte-check` errors in `src/lib/components/ui/command/` and `dialog/` reproduce identically on a clean tree).

## Lifecycle / Cross-Cutting Concerns

- **Active-pane invariant.** Every open sandbox now always has an active Manage pane (Phase A makes Dashboard activate-on-create; Phase B re-activates it on `close_pane`; Phase A changes the `switch_app` fallback to it). Nothing should leave a sandbox open with `active_app` pointing at a closed pane or at `None` while panes remain.
- **Forward teardown scoping.** Terminal and AgentWeb panes each own one Mutagen forward; Dashboard owns none (`close_app:436-443`). `close_pane` must tear down only the removed pane's forward and must not touch siblings' forwards — the extracted `teardown_forwards` helper is called with just the removed pane. Whole-sandbox teardown (`close_app`) still tears down all of them.
- **Zellij daemon survives tab close.** Closing a Terminal tab issues no Zellij stop (spike-confirmed); the in-container `zellij web` daemon and its named session persist, and reopening re-runs `prepare_terminal_webview` cheaply because `prepare_terminal`'s `--status` check is idempotent (`src-tauri/src/runtime/mod.rs:217-225`). Session **state** persists only via the Phase C stable-path URL.
- **`generate_handler!` registration.** The two new commands (`open_terminal_pane`, `close_pane`) must be added to the list in `src-tauri/src/lib.rs:1433-1499`, or `invoke()` rejects with "command not found" (`CLAUDE.md` Common Gotchas).
- **`AppKind` serialisation.** `close_pane`'s command takes `kind: AppKind`, which already `#[serde(rename_all = "lowercase")]`-serialises (`src-tauri/src/workspace/types.rs:21-27`) and round-trips through `switch_pane` today, so the frontend passes `"terminal"`/`"agentweb"` unchanged.
