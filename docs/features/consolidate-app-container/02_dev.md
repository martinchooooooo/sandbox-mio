# Consolidate AppContainer Webview — Build Plan

This document is the build plan for collapsing the AppContainer's three chrome/internal-route
webviews (`main-navigation-panel`, `main-switcher`, and a per-sandbox `workspace-<name>-dashboard`)
into a single webview, to eliminate the open/close jitter and flash those extra native webviews
cause today. **There is no `01_ideas.md` for this feature** — the user directed straight to
`/feature-dev` after a conversational design discussion; this document's "Resolved Decisions"
section stands in for that ideation's open-questions/answers pass. It continues and supersedes the
multi-webview model built by `docs/features/embedded-windows/02_dev.md` (Phases A–E, implemented)
and refined by `docs/features/responsive-panel/02_dev.md` and `docs/features/workspace-app-tabs/02_dev.md`
(both implemented) — none of those documents anticipated folding the internal-route webviews back
into one. **Status: Phases A–C implemented (`cargo check`/`cargo test`/`pnpm check` all pass); Phase
D (manual overlay-alignment validation) is Remaining — it needs a human at the running app.**

## Why today's split causes jitter

`create_main_window` (`src-tauri/src/workspace/mod.rs:73-123`) creates two chrome webviews
(`main-navigation-panel` at `WebviewUrl::App(Default::default())`, `main-switcher` at
`WebviewUrl::App("workspace/switcher")`), kept in sync only by `set_bounds` IPC round-trips on every
resize/collapse (`apply_layout`, `mod.rs:608-619`). Worse, `embed_dashboard_on_main`
(`mod.rs:258-299`) spins up a **brand-new webview** (`workspace-<name>-dashboard`) every time a
sandbox is opened, loading the SvelteKit bundle fresh in a new OS surface positioned at full content
bounds before it has rendered anything — this is the dominant source of the reported flash. None of
Nav, Switcher, or Dashboard need to be separate webviews: all three are same-origin `tauri://App`
routes with no Mutagen-forwarded external content, unlike Terminal/AgentWeb, which genuinely must
stay separate (`docs/features/embedded-windows/01_ideas.md`, "`<iframe>` rejected: `X-Frame-Options: DENY`").

## The concrete model

### One merged webview replaces three

`create_main_window` keeps exactly one child webview, `main-navigation-panel`, always sized to the
full window (`LogicalPosition(0,0)`, full `LogicalSize`) regardless of collapse state or whether any
sandbox is open. The `main-switcher` `add_child` call and `SWITCHER_LABEL` are deleted. This one
webview's SvelteKit document renders, via ordinary client-side routing and `{#if}` blocks (the same
mechanism `+layout.svelte` already uses to distinguish `isNavigationPanel`/`isWorkspaceChrome`,
`src/routes/+layout.svelte:26,33`): the nav rail/expanded panel, the two-level switcher bar
(currently `src/routes/workspace/switcher/+page.svelte`), and the active sandbox's Manage/Dashboard
content (currently `src/routes/workspace/dashboard/+page.svelte`) — all as sibling regions of one
page, not three separate documents. `embed_dashboard`/`embed_dashboard_on_main`
(`mod.rs:247-299`) are deleted entirely: Dashboard stops being a pane with a lifecycle and becomes
ordinary client-side view state.

### `AppKind` drops `Dashboard`; Terminal/AgentWeb overlays are unchanged

`AppKind` (`src-tauri/src/workspace/types.rs:23-27`) shrinks to `{ Terminal, AgentWeb }`. `AppPane`/
`WorkspaceState.apps` only ever holds Terminal/AgentWeb entries — the pane-lifecycle machinery
(`embed_external`, `switch_pane`, `close_pane`, `teardown_forwards`) is untouched for those two kinds,
because they still load external Mutagen-forwarded content and must remain genuine native child
webviews (`embed_external_on_main`, `mod.rs:162-213`, unchanged).

### Two layout regimes instead of one three-way split

`workspace_layout` (`mod.rs:42-68`) is replaced by a function returning exactly one rect that
matters to Rust: the `overlay` rect where a Terminal/AgentWeb webview is positioned when one is the
active view — `(nav_width, switcher_h, window_width - nav_width, window_height - switcher_h)`. There
is no `navigation_panel`/`switcher`/`content` three-way split to apply via `set_bounds` any more,
because Nav+Switcher+Dashboard live in one already-full-window webview that lays itself out with
ordinary CSS flex, the same way `src/routes/projects/+page.svelte` already does. The merged webview
is **never** resized by Rust — it is always the full window — so collapse/expand and switcher
show/hide become pure client-side CSS/DOM changes with zero IPC, zero native geometry calls, zero
flash. Only entering or leaving a Terminal/AgentWeb tab still touches `set_bounds`/`show`/`hide` on
the one overlay webview, which is unavoidable (that content is a real external process).

### Active-view state splits into two independent halves

Previously `WorkspaceState.active_app: Option<String>` meant one thing: which pane (Dashboard,
Terminal, or AgentWeb) is the single visible surface, because all three competed for the same native
content rect. Now:

1. **Which sandbox + which view (dashboard vs. terminal vs. agentweb) the user is looking at** is
   pure client-side state inside the merged webview (a small store next to
   `src/lib/workspace/state.svelte.ts`, e.g. `activeSandbox`/`activeView`), driven by ordinary
   Svelte navigation — no Rust command needed to view a sandbox's Manage tab.
2. **Whether a native overlay webview is shown** stays Rust-owned: `WorkspaceState.active_app`
   keeps its existing meaning but can now only ever point at a Terminal/AgentWeb pane's label, or be
   `None` when the client-side active view is Dashboard for any sandbox (or nothing is open). The
   merged webview calls a (simplified) `switch_pane`/`close_pane` only when the user's view changes
   to/from a Terminal/AgentWeb tab; navigating to/within Dashboard views never calls Rust at all.

### The overlay's geometry must track the merged webview's CSS pixel-for-pixel

The nav-rail/expanded widths and switcher-bar height are still native constants
(`NAVIGATION_PANEL_EXPANDED_W`/`RAIL_W`, `SWITCHER_H`, `mod.rs:28-35`) because the overlay webview is
a sibling native surface stacked on top of the merged webview's own DOM, not a child laid out by that
DOM. If the merged webview's CSS for the nav width or switcher height ever drifts from these Rust
constants, the overlay renders in the wrong place — a visible seam or gap, i.e. exactly the kind of
glitch this feature is trying to remove elsewhere. This repo already has a precedent for accepting
this risk without new sync machinery: `docs/features/responsive-panel/02_dev.md` Resolved Decision 4
picked "manual sync, cross-referenced by a code comment" over inventing a shared-constant channel for
`GLOBAL_EXPANDED_W`/the 420px CSS breakpoint. This plan keeps that precedent (see Resolved Decisions)
rather than introducing a runtime constant-sharing mechanism, but flags it as the single biggest new
regression risk this refactor introduces, and calls for a manual devtools check as this plan's
validation step (see Phase D) rather than assuming it away.

## Resolved Decisions

1. **Constant-sharing mechanism for the overlay's geometry vs. the merged webview's CSS.** Manual
   sync with cross-referencing comments, matching the existing `GLOBAL_EXPANDED_W`/420px precedent
   (`responsive-panel/02_dev.md` Decision 4) — not a runtime-shared constant or a frontend-measured
   feedback loop. Rejected alternative: have the frontend measure its content-slot placeholder's
   `getBoundingClientRect()` and report it to Rust on every layout change, then `set_bounds` the
   overlay to that reported rect. Rejected because it adds an IPC round-trip to the hot path this
   feature is trying to make jitter-free (switching into/out of a Terminal/AgentWeb tab), trading
   one drift risk for a new race/latency risk, for a value (nav width, switcher height) that already
   changes only when a human edits a CSS rule or a Rust constant, not at runtime.
2. **What "workspace open" means without a Dashboard pane.** A sandbox counts as having an open
   workspace purely as client-side state (the user has navigated to that sandbox's Dashboard route,
   or it has an open Terminal/AgentWeb pane) — there is no longer a Rust-tracked notion of "open"
   for a sandbox that has only ever been viewed via Dashboard. `open_workspace`'s only remaining
   Rust-side responsibility is bringing the AppContainer window to focus; viewing a sandbox's
   Dashboard is otherwise a client-side navigation with no command call. Consequently `open_workspace`
   loses its `embed_dashboard` call and most of its existing body; whether it survives as a thin
   focus-only command or is replaced entirely by a JS-side `getCurrentWindow().setFocus()` call is a
   Phase A implementation detail, not a design fork.
3. **Collapse/expand no longer depends on whether any sandbox is "open."** Previously the nav panel
   auto-shrunk from full-bleed the moment any pane existed (`has_content` in the old
   `workspace_layout`, `mod.rs:46-54`), because Dashboard's own webview was competing for space.
   Since Dashboard has no native footprint any more, the merged webview renders nav + switcher +
   dashboard content at full window width unconditionally (ordinary responsive flex layout, same as
   `src/routes/projects/+page.svelte` today); the nav rail only actually shrinks the visible
   in-page nav column via the existing collapse toggle, and Rust's `overlay` rect is only ever
   consulted — and the merged webview never resized — when a Terminal/AgentWeb tab is the active
   view. This directly removes the "Manage pane opens hidden and the window looks blank on first
   open" gotcha class the prior features had to guard against, because there is no separate Dashboard
   webview creation to race against.
4. **`AppKind::Dashboard` and its no-op guards are deleted, not deprecated.** `close_pane`'s
   `if kind == AppKind::Dashboard { return; }` guard (`mod.rs:459-461`) and every `match` arm handling
   `AppKind::Dashboard` (`teardown_forwards`, `mod.rs:498-502`; the `suffix` match in
   `embed_external_on_main`, `mod.rs:171-175`) are removed rather than left dead, because Dashboard is
   no longer a representable pane at all after this refactor — a future agent must not reintroduce a
   `Dashboard` pane path expecting these guards to still exist.
5. **Capability scoping loses the per-sandbox dashboard wildcard.** `capabilities/default.json`'s
   `webviews` list (`["main-navigation-panel", "main-switcher", "workspace-*-dashboard"]`) shrinks to
   `["main-navigation-panel"]` — there is no `main-switcher` webview or `workspace-*-dashboard`
   wildcard left to scope. The overlay webviews (`workspace-*-terminal`/`workspace-*-agentweb`) keep
   needing no capability entry, unchanged from today (external content, no `invoke` calls).

## Implemented: Phase A — collapse the Rust-side webview count

Touches `src-tauri/src/workspace/mod.rs` and `src-tauri/src/workspace/types.rs`.

- **`create_main_window`:** delete the `main-switcher` `add_child` block (`mod.rs:95-104`); the
  `main-navigation-panel` child is always created at full window size, with no `workspace_layout`
  call needed at construction time (there is nothing to collapse yet).
- **Delete `SWITCHER_LABEL`, `NAVIGATION_PANEL_LABEL`'s partner constant references to it, and
  `apply_layout`'s `set_webview_bounds(window, SWITCHER_LABEL, ...)` line** (`mod.rs:614`).
- **`workspace_layout`** (`mod.rs:42-68`) is rewritten to take only `(width, height, collapsed) ->
  Rect` (the overlay rect), dropping the `has_content` parameter and the `navigation_panel`/
  `switcher`/`content` three-way `WorkspaceRects` struct in favor of a single `Rect` — or, if keeping
  the geometry unit tests' shape is preferred, `WorkspaceRects` keeps just an `overlay: Rect` field.
  Existing unit tests (`mod.rs:658-714`) are rewritten against the new single-rect contract; the
  `without_content_the_navigation_panel_fills_the_window` case disappears (the nav panel always
  fills the window now, so there is nothing to assert per state).
- **Delete `embed_dashboard`, `embed_dashboard_on_main`** (`mod.rs:247-299`) and the `Dashboard`
  branch of `AppKind` (`types.rs:23-27`), plus every `match`/`if` arm naming it (Resolved Decision 4).
- **`activate_new_pane`** (`mod.rs:218-238`) keeps its shape for Terminal/AgentWeb but is no longer
  called from a Dashboard-creation path.
- **`WorkspaceState`** (`types.rs:70-80`): `rects: WorkspaceRects` becomes `overlay: Rect`;
  `sandbox_last_pane` keeps its existing meaning but can now only ever record a Terminal/AgentWeb
  label (never a Dashboard label), since Dashboard has no label any more.
- **Verify:** `cargo check` / `cargo test` in `src-tauri/`. Done: both pass; `workspace_layout` now
  takes `(width, height, collapsed) -> Rect` and `WorkspaceState.overlay: Rect` replaces
  `WorkspaceRects`, per the "or" alternative this phase allowed.

## Implemented: Phase B — commands: `open_workspace`, `switch_app`, `switch_pane`, `close_pane`

Touches `src-tauri/src/lib.rs` and `src-tauri/src/workspace/mod.rs`.

- **`open_workspace`** (`src-tauri/src/lib.rs`): dropped the `embed_dashboard` call and the
  exists/`switch_app` branch entirely (Resolved Decision 2). Implementation note: unlike the literal
  "only remaining job is bringing the window to focus" reading, the command keeps its
  `resolve_connection`/`watch_ports_file` body — that work is unrelated to Dashboard-as-a-pane, is
  still the only trigger for `.mio/ports.json` watching, and `PortsManager.svelte` depends on it
  running when a sandbox's Dashboard is first viewed. Window-focus is a non-issue in practice: the
  click site (`SandboxItem.svelte`) already runs inside the one remaining webview, so there is no
  separate window to focus.
- Added one small Rust addition beyond this document's literal text: a `hide_active_pane` command
  (`src-tauri/src/workspace/mod.rs`, registered in `generate_handler!`) that hides whichever
  Terminal/AgentWeb overlay is shown and clears `active_app`, with no replacement pane. Phase C's
  text says clicking the home/Manage tab should hide the overlay via "a small helper, not a new
  command," but no existing command can express "hide with no new target" — `switch_app`/
  `switch_pane` both require an existing pane to activate, and closing the pane (`close_pane`) would
  wrongly tear down the forward. This one-command addition is the minimal fix; flagged here per the
  skill's "stop and say so, don't silently deviate" instruction rather than left unmentioned.
- **`switch_app`** (`mod.rs:304-332`): now only ever targets a Terminal/AgentWeb pane (there is no
  Dashboard pane to fall back to). Its "no record for this sandbox" fallback
  (previously defaulting to the sandbox's Dashboard pane, `workspace-app-tabs/02_dev.md` Decision/Phase A)
  must become "no-op / do nothing to the overlay" when the sandbox has no open Terminal/AgentWeb pane,
  since the client-side router — not `switch_app` — is what puts the user on that sandbox's Dashboard
  view.
- **`switch_pane`/`close_pane`** (`mod.rs:337-366`, `458-493`): both drop their `AppKind::Dashboard`
  special-casing (Resolved Decision 4); `close_pane`'s "re-activate the sandbox's Dashboard pane"
  behavior (`mod.rs:471-479`) becomes "clear `active_app`/hide the overlay and let the client-side
  view fall back to Dashboard," since there is no Dashboard pane label to re-activate.
- **Verify:** `cargo check` / `cargo test` in `src-tauri/`.

## Implemented: Phase C — frontend: fold Switcher and Dashboard into the NavigationPanel document

Touches `src/routes/+layout.svelte`, `src/lib/workspace/state.svelte.ts`,
`src/lib/workspace/WorkspaceSwitcher.svelte` and `src/lib/workspace/WorkspaceDashboard.svelte`
(new, replacing the deleted `src/routes/workspace/switcher/+page.svelte` and
`src/routes/workspace/dashboard/+page.svelte` routes — the "moved into a component" option this
phase allowed, not the goto()-routed alternative), `src/routes/projects/SandboxItem.svelte`,
`src-tauri/capabilities/default.json`.

- **New client-side view state** in `state.svelte.ts`: just `activeSandbox` (which sandbox's
  Dashboard to show) plus `setActiveSandbox`/`activeSandboxName` accessors. **Dashboard visibility
  is derived, not tracked as its own state**: `activeAppLabel() === null && activeSandbox === name`
  (`+layout.svelte`, `WorkspaceSwitcher.svelte`). An earlier version of this phase also tracked an
  explicit client-side `activeView` ("dashboard"/"terminal"/"agentweb") set *optimistically* before
  the `invoke` that opens a Terminal/AgentWeb pane — that hid the Dashboard immediately regardless
  of whether the pane actually embedded, so a slow or failing `open_terminal_pane`/`open_agent_web`
  left the screen showing neither Dashboard nor Terminal (reported after initial implementation).
  Deriving visibility from Rust's own confirmed `active_app` fixes this: the Dashboard only hides
  once the overlay is actually shown, and stays shown if the invoke fails or never resolves.
  `isSandboxOpen` is redefined per Resolved Decision 2 to also count a sandbox whose Dashboard is
  the current `activeSandbox`.
- **`WorkspaceSwitcher.svelte`**: the old `switcher/+page.svelte` markup, now reading
  `workspaceApps()`/`activeAppLabel()` from the shared store instead of its own `workspace://state`
  listener (it runs inside the same document now). The home/Manage tab is unconditional per sandbox
  group (there is no `dashboard`-kind `AppPane` to look up any more, matching the Gotcha table);
  its active state comes from `activeAppLabel() === null && activeSandboxName() === sandbox`, not
  `apps`.
- **`WorkspaceDashboard.svelte`**: the old `dashboard/+page.svelte` content, taking `sandboxName` as
  a prop instead of reading it off `page.url.searchParams`. Its launcher buttons call `invoke()`
  with no optimistic client-side state change beforehand (see above); `launcherAction.error` is now
  rendered so a launch failure is visible instead of silently swallowed.
- **`+layout.svelte`** restructured into a flex row: a left nav column (width `w-[380px]`/`w-12`,
  cross-referencing `NAVIGATION_PANEL_EXPANDED_W`/`NAVIGATION_PANEL_RAIL_W` in a comment per Resolved
  Decision 1) holding `GlobalNav` + the routed page (`{@render children?.()}`) + `ActivityCenter`,
  and — only when a Terminal/AgentWeb pane is open somewhere or a sandbox's Dashboard is derived as
  active — a right column holding `WorkspaceSwitcher` (when any pane is open) above the Dashboard
  content (when derived active; otherwise the Terminal/AgentWeb overlay covers that region
  natively). This is a bigger restructuring than a plain if/else swap because the nav column's own
  collapse-to-rail toggle must keep working via CSS width alone now, with the switcher/dashboard
  region as an independent sibling rather than a separate document. Gating this column's existence
  on the *derived* Dashboard flag (not a separately-set client flag) also matters here: it is what
  keeps the nav column from flashing full-width and back while a new Terminal/AgentWeb pane is still
  embedding.
- **Clicking a switcher tab for Terminal/AgentWeb** still calls `invoke("switch_pane", ...)` /
  `invoke("open_terminal_pane"/"open_agent_web", ...)` exactly as today, plus a client-side
  `setActiveSandbox` call (so a later return-to-Dashboard lands on the right sandbox). Clicking the
  home/Manage tab or a sandbox in the projects list (`SandboxItem.svelte`'s `openWorkspace()`) calls
  `setActiveSandbox` plus the new `hide_active_pane` command (see Phase B's noted deviation) to hide
  whatever overlay — possibly a *different* sandbox's — was showing; `open_workspace` itself still
  fires too, but only for its ports-watching side effect (see Phase B).
- **`capabilities/default.json`**: shrunk `webviews` to `["main-navigation-panel"]` (Resolved
  Decision 5).
- **Verify:** `pnpm check` (svelte-check) — passes with no new errors (5 pre-existing errors remain
  in untouched `src/lib/components/ui/command`/`dialog` files, confirmed unrelated via `git status`).
  Manual run confirming the projects list → Dashboard → Terminal → back-to-Dashboard flow shows no
  blank-webview flash was **not** done in this pass — see Phase D below.

## Remaining: Phase D — validation: overlay-vs-DOM alignment

Not previously needed because Nav/Switcher/Dashboard used to each own their exact native rect; now
the overlay's rect must match a boundary drawn only in CSS inside a sibling document. Before trusting
Phase C's layout:

- Manually open a sandbox's Terminal tab, resize the window, and toggle collapse with devtools open
  on the overlay webview, confirming its `set_bounds` rect lines up pixel-for-pixel against the
  merged webview's rendered nav-width/switcher-height boundary at both the expanded and collapsed nav
  widths, and at at least one non-default DPI scale factor (mirroring the embedded-windows spike's
  200%-scaling check, `docs/features/embedded-windows/01_ideas.md` Validation).
- Confirm no single-frame flash of stale Dashboard content is visible when switching from Dashboard
  to a Terminal/AgentWeb tab and back, since the overlay now sits on top of a persistently-rendered
  Dashboard rather than beside a hidden one.
- This is a manual check, not a new automated e2e assertion; record the outcome in `03_e2e.md` when
  that document is written.

## Lifecycle / Cross-Cutting Concerns

- **Teardown.** `close_app`/`close_workspace` (`mod.rs:422-454`) no longer needs to close a
  `Dashboard`-kind pane or its (nonexistent) forward — `teardown_forwards`'s `AppKind::Dashboard =>
  None` arm (`mod.rs:501`) is deleted along with the variant, not left as dead code (Resolved
  Decision 4). Undeploy/destroy/`WindowEvent::Destroyed` teardown paths are otherwise unchanged:
  they still only ever had real cleanup work for Terminal/AgentWeb forwards.
- **`sandbox_last_pane` cleanup.** Still keyed by sandbox, but can now only ever hold a Terminal/
  AgentWeb label; a sandbox that was only ever viewed via Dashboard never gets an entry, which is
  fine because there is no Dashboard label to restore — the client-side `activeView` state (Phase C)
  is what a re-focused sandbox falls back to, not `sandbox_last_pane`.
- **`generate_handler!` registration.** No new commands are added by this refactor; if `open_workspace`
  is deleted outright (Phase B), remove its entry from the `generate_handler!` list
  (`src-tauri/src/lib.rs`) and its frontend call sites, or `invoke()` calls into it will 404.
- **DPI and units.** The one remaining native geometry call (the overlay's `set_bounds`) keeps using
  `LogicalPosition`/`LogicalSize` exactly as today; no new DPI concern beyond Phase D's validation
  that the merged webview's CSS boundary and the overlay's logical rect agree.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Opening a sandbox's Manage tab still shows a blank flash | A leftover `embed_dashboard`/`workspace-*-dashboard` call site was not fully removed, or the frontend still routes to a separate document instead of switching client-side state | Confirm no code path creates a `workspace-*-dashboard` webview any more; Dashboard is client-side view state only (Resolved Decision 2) |
| The Terminal/AgentWeb overlay renders in the wrong place or with a visible seam | The merged webview's CSS nav-width/switcher-height drifted from `NAVIGATION_PANEL_EXPANDED_W`/`RAIL_W`/`SWITCHER_H` (`mod.rs:28-35`), since nothing enforces they match at runtime (Resolved Decision 1) | Cross-reference the Rust constants in a comment next to the CSS values, and re-run the Phase D manual alignment check after either changes |
| `close_pane`/`teardown_forwards` panics or mis-branches on `AppKind::Dashboard` | A call site still matches on the deleted `Dashboard` variant | Delete every arm naming `AppKind::Dashboard`, don't stub it out (Resolved Decision 4) — the compiler's exhaustiveness check on the two-variant `AppKind` enum will catch any survivor |
| The switcher's Manage/home tab has no visible "active" state after the merge | The two-level switcher's `bySandbox` grouping (`switcher/+page.svelte:16-24`) was written assuming a `dashboard`-kind `AppPane` to look up (`paneFor(panes, "dashboard")`, `switcher/+page.svelte:77,91`) | Derive the home tab's active/inactive state from the new client-side `activeSandbox`/`activeView` store instead of searching `apps` for a `dashboard` pane, since that pane no longer exists |
| `capabilities/default.json`'s dashboard-route `invoke` calls (`install_agent`, etc.) are suddenly denied | The `workspace-*-dashboard` wildcard was removed from `webviews` (Resolved Decision 5) but the Dashboard UI still runs inside `main-navigation-panel`, which is still covered | Nothing to fix if Dashboard content now renders inside `main-navigation-panel` — verify the UI is actually mounted in that webview, not accidentally left loading as a separate route document that lost its capability coverage |
| Resizing the OS window while a Terminal tab is active leaves the overlay masking the wrong region | The resize handler still assumes the old three-rect `apply_layout` | `relayout`/`apply_layout` now only ever `set_bounds`s the single overlay rect (or nothing, if the active view is Dashboard) — confirm the resize listener was updated, not left calling the deleted `navigation_panel`/`switcher` bound-setting lines |
| Terminal opens fine, but returning to the Manage tab shows a blank content area (confirmed live: the Dashboard's markup is present in the DOM, just zero-height) | `WorkspaceSwitcher.svelte`'s tab-bar `<div>` used Tailwind `h-screen` (100vh) instead of a height matching `SWITCHER_H` (64px). In the flex column, that fixed-100vh switcher bar consumes the whole column, squeezing its `flex-1` Dashboard sibling to zero height. The Terminal/AgentWeb overlay never reveals this because it's a *native* webview positioned by Rust's own `SWITCHER_H`-based `workspace_layout`, entirely independent of this broken CSS | Use `h-16` (64px, matching `SWITCHER_H`) with `shrink-0`, not `h-screen`, on the switcher bar's root `<div>` |
