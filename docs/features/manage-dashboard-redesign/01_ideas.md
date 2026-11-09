---
type: ideas
title: "Manage Dashboard Redesign: Fast Load and a Simple Overview"
feature: manage-dashboard-redesign
---
# Manage Dashboard Redesign: Fast Load and a Simple Overview

This document is the ideation for restructuring a sandbox's Manage dashboard so that it opens instantly and is approachable for a non-technical user. Today the Manage dashboard (`src/lib/workspace/WorkspaceDashboard.svelte`, rendered by `src/lib/workspace/WorkspaceContent.svelte` from the root layout) is one long scrolling column that stacks seven panels — packages, agents, ports, remote deploy, network proxy and policy, traffic, and destroy — and every panel fetches its own data on mount. The redesign splits the dashboard into an Overview (launch the terminal or agent web UI, install and log in coding agents, a one-line status summary) and a settings-style section sidebar for the advanced controls, where each section mounts and fetches only when opened. The user decided on 2026-11-08 to keep the project's own traffic viewer rather than adopting mitmproxy's `mitmweb` UI; the evaluation is recorded under Technology Choices, and the separate host-side mitmproxy memory saving is deferred to `04_improvements.md`. The user's other 2026-11-08 decisions (secrets placement, the summary's proxy source, an icon-collapsing sidebar built as a reusable `src/lib/components/ui/` component, per-sandbox section memory) are listed under Resolved Decisions. The panel components live in `src/routes/projects/` (`AgentManager.svelte`, `PackageManager.svelte`, `PortsManager.svelte`, `RemoteManager.svelte`, `ProxyPanel.svelte`, `NetPolicyEditor.svelte`, `TrafficPanel.svelte`); shared stores live in `src/lib/sandbox/state.svelte.ts`, `src/lib/workspace/packages.svelte.ts`, `src/lib/workspace/state.svelte.ts` and `src/lib/proxy/traffic.svelte.ts`.

## Current State

### Render gate

`WorkspaceDashboard.svelte` calls `list_sandboxes` in `onMount` only to learn the sandbox's `workspace_path` and `provider`, and renders nothing below the header until `workspacePath` is set. `list_sandboxes` (`src-tauri/src/commands/sandbox.rs`) calls `CliBackend::list` (`src-tauri/src/backends/mod.rs`), which lists every provider's instances one provider after another (a `podman ps` and an `msb` listing). The same project list is already held by the module store `src/lib/sandbox/state.svelte.ts` (`sandboxesList()`), which the projects list keeps current on a poll. The dashboard therefore blocks its whole body on a duplicate full listing.

### Mount-time backend calls

All panels mount together inside the `{#if workspacePath}` block, and `WorkspaceContent.svelte` wraps the dashboard in `{#key dashboardSandbox}`, so switching sandbox remounts and refetches everything.

| Panel (file) | Mount-time call | Cost class |
|---|---|---|
| `WorkspaceDashboard.svelte` | `list_sandboxes` | Full instance listing of every provider (blocks the body) |
| `AgentManager.svelte` | `list_credential_bundles`, then `inference_status` (awaited in sequence) | App-data read, in-memory state |
| `AgentManager.svelte` (`loadAgentStatuses`) | `get_agent_auth_status` per installed agent, awaited one after another | One or two `exec_in_container` reads per agent (`get_auth_status` in `src-tauri/src/agents/mod.rs`) |
| `packages.svelte.ts` (`ensurePackages`) | `list_packages` | Workspace `mise.toml` read; already de-duplicated and cached per sandbox |
| `ProxyPanel.svelte` | `proxy_status`, then `list_proxy_rules` | `proxy::status` (`src-tauri/src/proxy/mod.rs`) performs a full `list_instances` of the provider |
| `TrafficPanel.svelte` | `proxy_status` again, then `watch_proxy_flows` | A second full `list_instances`; starts the flow poller |
| `RemoteManager.svelte` | `ensureSandboxRemoteState()` | Shared store; already started by the projects list |
| `PortsManager.svelte` | none | Reads a live store fed by the backend poll |

Opening one dashboard therefore costs at least three full instance listings plus up to two in-sandbox `exec` calls per installed agent, and the agent checks run serially. No timing has been measured; the cost classes come from reading the code (see Validation).

### Layout

The dashboard is a single `max-w-2xl` column. The launcher buttons (`LAUNCHABLE_APPS` in `src/lib/workspace/apps.ts`) sit at the top, followed by every panel with equal visual weight, and the Destroy button at the bottom. Secrets are edited inside `ProxyPanel.svelte` (Podman injection rules) and `NetPolicyEditor.svelte` (microsandbox secret bindings), not in a section of their own.

## Proposed Model

### Overview section (default)

Opening a sandbox lands on Overview. Overview holds only what a first-time user needs:

- The launcher buttons (Terminal, Agents) from `LAUNCHABLE_APPS` in `src/lib/workspace/apps.ts`, unchanged in behaviour.
- The coding-agent list from `AgentManager.svelte`: install, log in, auth status.
- A one-line status summary (for example "Network: recorded · 2 ports open · Local"), each part a link that opens the matching advanced section.
- The Open folder button and the close control, which stay in the dashboard header.

### Section sidebar

A vertical section list sits at the left of the dashboard body, inside the content area to the right of the app's navigation column (`src/lib/workspace/NavigationColumn.svelte`). The list is a new reusable component, proposed as `src/lib/components/ui/section-nav/` (see Section-nav component). Proposed sections: Overview, Tools (`PackageManager.svelte`), Ports (`PortsManager.svelte`), Network (`ProxyPanel.svelte` and `NetPolicyEditor.svelte`, with secrets under their own heading), Traffic (`TrafficPanel.svelte`), Remote (`RemoteManager.svelte`), and, below a separator, Danger zone (Destroy). A section a provider cannot honour is hidden, following the existing invariant in `docs/architecture/overview.md` (for example Remote for a microsandbox sandbox, matching the current `{#if provider}` gating in `WorkspaceDashboard.svelte`). The existing panel components move under the sections largely unchanged; the redesign is a re-housing, not a rewrite of each panel.

### Section-nav component

`src/lib/components/ui/section-nav/` is a shadcn-style compound component following the file layout of the other components in `src/lib/components/ui/`: a root holding the active value, a list, an item carrying an icon and a label, and a content region per section. It is built on bits-ui's vertical `Tabs` (the same primitive behind `src/lib/components/ui/tabs/`), which supplies the tablist/tabpanel roles and roving keyboard focus. It is styled by hand to the retro look, as `field` and `input-group` already are (`border-2`, `shadow-md`, `rounded-md`), because retro-ui-svelte has no `sidebar` registry item (`https://retroui-svelte.netlify.app/r/sidebar.json` returned 404 on 2026-11-08).

- **Icon collapse.** When the space the component is given is narrow, the list collapses to icons only. Each collapsed item keeps its label as an accessible name and shows it in a tooltip. The collapse follows a container query on the component's own width (the `@container` pattern `src/lib/workspace/NavigationColumn.svelte` already uses), not the window width, because the navigation column's expanded or rail state changes the space available.
- **Tooltip dependency.** The collapsed state needs a tooltip primitive, which `src/lib/components/ui/` lacks; it is pulled from the retro-ui-svelte registry (`https://retroui-svelte.netlify.app/r/tooltip.json`, which exists).
- **Lazy content.** The content region renders its children only for the active value. The laziness lives inside the component, so no caller can mount every section by accident (see Common Gotchas).
- **Reuse.** Nothing in the component knows about sandboxes; the Settings page (`src/routes/settings/+page.svelte`) is a later candidate consumer.

### Lazy mounting

Only the active section's component is mounted, through the section-nav content region. A section's mount-time fetches therefore run only when the user opens that section. Overview's own cost becomes the agent auth checks plus the summary inputs.

### Instant first paint

The dashboard reads `workspace_path` and `provider` from `sandboxesList()` in `src/lib/sandbox/state.svelte.ts` instead of calling `list_sandboxes` itself, so the header, launchers and section sidebar render on the first frame. Data that is still loading shows per-row placeholders instead of a blank page. A sandbox created moments ago may not yet be in the store; the dashboard falls back to a refresh of that store (`refreshSandboxState()`) rather than its own listing.

### Per-sandbox caches for section data

Section data moves into module-scoped stores keyed by sandbox, following `src/lib/workspace/packages.svelte.ts`: a cached value shown immediately, then revalidated in the background. Two stores are new:

- A proxy-status store shared by the Network and Traffic sections and the Overview summary, so `proxy_status` runs once per sandbox instead of once per panel.
- An agent-auth store holding each installed agent's status, filled by running the `get_agent_auth_status` calls concurrently and rendering each row as its answer arrives.

The active section per sandbox also lives in a store (proposed `src/lib/workspace/dashboard.svelte.ts`). Reopening a sandbox, or switching sandbox and back, restores that sandbox's last section instead of resetting to Overview through the `{#key}` remount. The store is frontend memory only, so the last section survives navigation but not an app restart. Section memory mirrors how the sandbox switcher restores each sandbox's last active pane (`docs/architecture/workspace-ui.md`).

### Status summary inputs

Ports come from the existing live ports store, and deployment from `src/lib/sandbox/state.svelte.ts`. The proxy part reads the shared proxy-status store, which calls `proxy_status` (`src-tauri/src/commands/proxy.rs`). `proxy_status` is the authoritative answer: it reports whether the sidecar exists, whether it is running, and, for microsandbox, whether the SOCKS listener answers a health probe. The summary's proxy part shows a placeholder until `proxy_status` resolves; the rest of the summary renders immediately. Because the Network and Traffic sections read the same store, opening them afterwards costs no second `proxy_status` call.

## Technology Choices with Tradeoffs

### Navigation: settings-style section sidebar (chosen) vs. sub-tabs vs. collapsible sections vs. nested routes

Chosen by the user on 2026-11-08: a vertical section list inside the dashboard. A sidebar gives each advanced section a full-height page, names every section at a glance for a non-technical user, and scales to more sections (planned work such as `proxy-network-policy` and `proxy-credential-secrets` adds controls) without a crowded strip.

- **Horizontal sub-tabs** (the existing `Tabs` component in `src/lib/components/ui/tabs/`) — rejected by the user: equally lazy, but a horizontal strip of seven labels gets crowded at the rail-collapsed widths `docs/features/responsive-panel/02_dev.md` supports, and it reads as one more row of chrome under the sandbox switcher.
- **Collapsible sections on one scrolling page** (`src/lib/components/ui/collapsible/`) — rejected by the user: keeps the long-page shape that makes the dashboard intimidating, and expanded sections pile up again on one scroll.
- **Nested SvelteKit routes per section** (for example `/projects/<name>/network`) — rejected: the dashboard is not a route; the root layout renders it from the active-app state Rust reports (`docs/features/consolidate-app-container/02_dev.md`), so routing sections would fight the pane model and the `workspace://state` subscription in `src/lib/workspace/state.svelte.ts`.

### Sidebar primitive: a new retro-styled `section-nav` component (chosen) vs. shadcn-svelte `sidebar` vs. the existing `Tabs` used inline vs. a hand-rolled list

Chosen by the user on 2026-11-08: a new reusable compound component in `src/lib/components/ui/section-nav/`, built on bits-ui's vertical `Tabs` and collapsing to icons by container width (see Section-nav component).

- **shadcn-svelte `sidebar`** (`https://shadcn-svelte.com/registry/sidebar.json`, inspected 2026-11-08) — rejected: it has a built-in icon-collapse mode, but it is built to be the app's main sidebar. Its container is positioned `fixed` to the viewport at full screen height, its provider binds a global Ctrl+B shortcut and saves the collapsed state in a cookie, and it swaps to a slide-out sheet by window width. Nested inside the dashboard, to the right of `NavigationColumn.svelte`, all four behaviours would need forking. It also pulls five more registry items (`sheet`, `tooltip`, `skeleton`, `input`, `is-mobile`), has no retro-ui-svelte variant, and gives menu-button semantics rather than the tablist/tabpanel semantics of switching panels in place.
- **The existing `Tabs` (`src/lib/components/ui/tabs/`) used inline in the dashboard** — rejected: bits-ui `Tabs.Content` always renders its children, so every caller would have to remember to add laziness, and the icon collapse would not be reusable. The user wants the sidebar as a component of its own.
- **A hand-rolled button list** — rejected: loses the keyboard and ARIA behaviour bits-ui provides for free.

### Summary proxy source: `proxy_status` through a shared store (chosen) vs. a "has proxy" field on `list_sandboxes`

Chosen by the user on 2026-11-08: the most correct source, even if slower. `proxy_status` (`src-tauri/src/proxy/mod.rs`, `status`) reports existence, running state and the microsandbox health probe at the moment it is asked.

- **A "has proxy" field on `list_sandboxes`** (`src-tauri/src/commands/sandbox.rs`), derived from the instances `proxy::is_sidecar` already filters out — rejected: free to compute, but it reports existence only. A stopped sidecar (which still counts as viewer on, failing closed, per `docs/architecture/network.md`) or an unhealthy microsandbox listener would read the same as a healthy one. The value would also be as stale as the projects-list poll, and it changes the `Sandbox` IPC shape in `src/lib/sandbox.ts`.

### Initial data: read the shared sandbox store (chosen) vs. a new single-sandbox command vs. keeping the dashboard's own `list_sandboxes`

Chosen: read `sandboxesList()` from `src/lib/sandbox/state.svelte.ts`. The data is already in memory and already kept current.

- **A new `get_sandbox(name)` command** — rejected: still a provider round trip on the critical path, and the provider lookup inside it would list instances anyway.
- **Keeping the dashboard's own `list_sandboxes`** — rejected: the dashboard's body stays blank for one full listing of every provider on every open and every sandbox switch.

### Traffic inspection UI: keep the project's own viewer (chosen by the user) vs. host-side `mitmweb` vs. `mitmweb` inside the existing sidecars

Chosen by the user on 2026-11-08: keep `TrafficPanel.svelte` and move it under the Traffic section unchanged.

- **`mitmweb` on the Windows host** — rejected for this feature. For microsandbox sandboxes the reachability is proven: Spike 03 (`docs/features/microsandbox-network-policy/spike/03_chained-proxy.md`, 2026-11-01) ran Windows-native `mitmdump` on an authenticated `127.0.0.1` SOCKS5 port and chained a microsandbox sandbox to it, failing closed when the proxy was down. For Podman sandboxes it is not: the sandbox runs inside the WSL Podman machine, and reaching a Windows-host listener needs either a listener bound `0.0.0.0` inside the machine (rejected as too exposed in `docs/features/network-proxy/01_ideas.md`) or WSL mirrored networking, which is host-global. A host process also ties sandbox egress to the app being open, and cannot follow a sandbox to a remote host (`docs/features/proxy-remote-deploy/`). Finally, `mitmweb` shows request headers, so a secret injected by `src-tauri/resources/mitm_addon.py` (Podman) or substituted by microsandbox before the mitmproxy leg would appear in the UI. That breaks the invariant that no secret value is returned to any UI (`docs/architecture/network.md`).
- **`mitmweb` inside the existing sidecars, shown as a pane** — rejected: saves no memory, needs a new tunnel into a sidecar that runs no `sshd`, has the same secret-header exposure, and adds a second UI that does not match the retro-ui-svelte look (the same reasoning as `docs/features/proxy-traffic-viewer/01_ideas.md`).

On memory: a Podman sidecar is already a separate container, so it adds no memory to the sandbox container itself; it shares the Podman machine's memory. A microsandbox sidecar is a whole microVM per proxied sandbox, and replacing that VM with a host `mitmdump` is the real saving. That change is independent of which UI shows traffic, so it is deferred to `04_improvements.md`.

## Validation

No spike has been run for this feature. The cost table in Current State comes from reading the code, not from measurement. One finding is verified from source: bits-ui 2.18.1's `tabs-content.svelte` always renders its children, so `Tabs` alone does not lazy-mount sections. Before `02_dev.md` commits to phases, time each mount-time call against a real Podman sandbox (the `standard` tier environment) so the plan can confirm which change buys the most: removing the render gate, de-duplicating `proxy_status`, or running the agent auth checks concurrently.

## Resolved Decisions

User decisions of 2026-11-08. `02_dev.md` carries them forward.

1. **Navigation shape:** a settings-style section sidebar inside the dashboard (Technology Choices, Navigation).
2. **Overview contents:** the Terminal and Agents launchers, coding-agent install and login, and the status summary. Tools, Ports, Network, Traffic, Remote and Danger zone are sidebar sections.
3. **Traffic UI:** keep `TrafficPanel.svelte`; no `mitmweb` (Technology Choices, Traffic inspection UI).
4. **Secrets placement:** secrets stay inside the Network section, under their own heading.
5. **Summary proxy source:** `proxy_status` through the shared proxy-status store, the most correct source even if slower (Technology Choices, Summary proxy source).
6. **Narrow-width behaviour:** the section sidebar collapses to icons, built as a reusable retro-styled component in `src/lib/components/ui/section-nav/` rather than the shadcn-svelte `sidebar` (Section-nav component).
7. **Section memory:** reopening a sandbox restores its last section (Per-sandbox caches for section data).
8. **Tools on Overview:** not needed. A first-time user adds an agent from Overview and starts working; installing an agent through `AgentManager.svelte` already installs the agent's mise packages, so no first-time flow needs the Tools section.
9. **Collapse width:** `section-nav` collapses to icons when its expanded list would take more than 10% of the dashboard's width, measured with a container query on the dashboard body. Superseded by `docs/features/manage-dashboard-redesign/02_dev.md`, Resolved Decision 10, after the user delegated the threshold.

## Open Questions

- **Secrets placement.** Do secrets get their own sidebar section, or stay inside Network? Resolved: Resolved Decision 4.
- **Proxy presence for the summary.** Does `list_sandboxes` gain a per-sandbox "has proxy" field, or does the Overview summary call `proxy_status` through the new proxy-status store? Resolved: Resolved Decision 5.
- **Narrow-width behaviour.** Does the section sidebar collapse to icons, become a dropdown, or turn into a horizontal strip at narrow widths? Resolved: Resolved Decision 6.
- **Section memory.** Does the dashboard reopen each sandbox on its last section, or always on Overview? Resolved: Resolved Decision 7.
- **Tools on Overview.** Does any Overview flow need the Tools section to finish a first-time setup? Resolved: Resolved Decision 8.
- **Collapse width.** At which container width does `section-nav` collapse to icons? The available width ranges from the window minus the 380px expanded navigation column down to the window minus the 48px rail (`NAVIGATION_PANEL_EXPANDED_W` and `NAVIGATION_PANEL_RAIL_W` in `src-tauri/src/workspace/mod.rs`). Resolved: Resolved Decision 9.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Every section's backend calls still run on dashboard open after the move to a sidebar | bits-ui 2.18.1 `Tabs.Content` (`node_modules/bits-ui/dist/bits/tabs/components/tabs-content.svelte`) always renders its children and only hides inactive panels | Make the `src/lib/components/ui/section-nav/` content region render its children only for the active value, instead of passing them straight to `Tabs.Content` |
| The section sidebar stays expanded when the navigation column is expanded in a narrow window, or collapses in a wide one | The collapse was written as a viewport media query, but the width available to the dashboard depends on the navigation column's expanded or rail state | Drive the collapse with a container query on the `section-nav` root (`@container`, as in `src/lib/workspace/NavigationColumn.svelte`) |
| Collapsed sidebar icons are unlabeled for screen readers and unexplained on hover | The collapsed state hides the item label text | Keep the label as each item's accessible name and show the label in a retro-ui-svelte tooltip (`https://retroui-svelte.netlify.app/r/tooltip.json`) |
| The Overview summary shows a proxy state that contradicts the Network section | The summary and the Network section fetched `proxy_status` separately at different moments | Read both from the one per-sandbox proxy-status store, and refresh that store after any proxy action (restart, rule save, re-apply) |
| The dashboard resets to Overview whenever the user switches sandbox and back | `{#key dashboardSandbox}` in `src/lib/workspace/WorkspaceContent.svelte` remounts the dashboard, destroying component-local `$state` | Keep the active section per sandbox in a module store (proposed `src/lib/workspace/dashboard.svelte.ts`), following `src/lib/workspace/packages.svelte.ts` |
| A just-created sandbox's dashboard shows no workspace path | The dashboard reads `sandboxesList()` from `src/lib/sandbox/state.svelte.ts`, whose poll has not yet picked up the new sandbox | Call `refreshSandboxState()` when the sandbox is missing from the store, instead of reinstating a dashboard-local `list_sandboxes` call |
| `proxy_status` still runs twice per dashboard open | `ProxyPanel.svelte` and `TrafficPanel.svelte` each call `proxy_status` in their own `refresh()` | Route both through one per-sandbox proxy-status store; each `proxy::status` call in `src-tauri/src/proxy/mod.rs` lists every instance of the provider |
| Agent rows stay blank for seconds with several agents installed | `loadAgentStatuses()` in `src/routes/projects/AgentManager.svelte` awaits each `get_agent_auth_status` serially, and each one runs `exec_in_container` | Issue the calls concurrently and render each agent row as its status arrives |
| Standard-tier specs fail to find a panel's controls after the redesign | Specs such as `e2e/test/specs/standard/expose-port.spec.ts` and `e2e/test/specs/standard/agent-setup.spec.ts` expect every panel on the single dashboard page | Open the matching sidebar section in the spec before querying the panel's controls |
| A hidden section reappears for a provider that cannot honour it | Section visibility was hard-coded in the sidebar instead of following provider capabilities | Derive section visibility from the provider capabilities in `src/lib/sandbox/state.svelte.ts`, per the hidden-control invariant in `docs/architecture/overview.md` |
