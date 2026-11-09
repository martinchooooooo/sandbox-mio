---
type: dev-plan
title: "Manage Dashboard Redesign: Build Plan"
feature: manage-dashboard-redesign
---
# Manage Dashboard Redesign: Build Plan

This document is the build plan for restructuring a sandbox's Manage dashboard (`src/lib/workspace/WorkspaceDashboard.svelte`) into an Overview plus a lazily mounted section sidebar, and for removing the dashboard's blocking and duplicate backend calls. It continues `docs/features/manage-dashboard-redesign/01_ideas.md` and supersedes it wherever the two differ; the user decisions of 2026-11-08 (Resolved Decisions 1 to 9) are carried forward unchanged. Status: Phases B to E are implemented; Phase A (baseline timing) was not run and is deferred per Resolved Decision 13. Deferred work (a host-native `mitmdump` for microsandbox sidecars) lives in `docs/features/manage-dashboard-redesign/04_improvements.md`.

## Concrete Model

### Dashboard shape

`WorkspaceDashboard.svelte` keeps its header row (sandbox name, Open folder button, close control) and the `StatusRow` (`src/routes/projects/StatusRow.svelte`) above the body. The body is one `SectionNav.Root` (`src/lib/components/ui/section-nav/`) that fills the remaining height: the section list on the left, the active section's content on the right. The content region is the scroll container; the section list stays in place while a long section scrolls. Section content keeps the existing `max-w-2xl` column width.

The sections, in list order, come from one catalogue, `DASHBOARD_SECTIONS` in the new file `src/lib/workspace/sections.ts`, following the `LAUNCHABLE_APPS` catalogue pattern in `src/lib/workspace/apps.ts`. Each catalogue entry carries an id, a Hugeicons icon (`@hugeicons/core-free-icons`), a label function, and a `visible(context)` predicate over the sandbox's provider capabilities and proxy status.

| Section id | Icon | Content | Visible when |
|---|---|---|---|
| `overview` | `DashboardSquare01Icon` | `src/lib/workspace/DashboardOverview.svelte` (launchers, `AgentManager.svelte`, status summary) | always |
| `tools` | `Wrench01Icon` | `src/routes/projects/PackageManager.svelte` | always |
| `ports` | `PlugSocketIcon` | `src/routes/projects/PortsManager.svelte` | always |
| `network` | `Shield01Icon` | `src/routes/projects/ProxyPanel.svelte` (which renders `NetPolicyEditor.svelte` for microsandbox), with a Secrets heading | always |
| `traffic` | `Activity01Icon` | `src/routes/projects/TrafficPanel.svelte`, or the existing "viewer off" message when the sandbox has no sidecar | always (Resolved Decision 14) |
| `remote` | `CloudServerIcon` | `src/routes/projects/RemoteManager.svelte` | `capabilitiesFor(provider)?.remoteTargets` is true (`src/lib/sandbox/state.svelte.ts`) |
| `danger` | `Alert02Icon` | the Destroy button, below a `SectionNav.Separator` | always |

The panel components move under the sections largely unchanged. `WorkspaceDashboard.svelte` keeps the dashboard-level `createActionRunner()` (`src/lib/actions.svelte.ts`) and passes the runner to every section, so a running action keeps the `StatusRow` visible and disables buttons in every section (Resolved Decision 17).

### Section-nav component

`src/lib/components/ui/section-nav/` is a shadcn-style compound component with no knowledge of sandboxes. Files: `index.ts` (exports `Root`, `List`, `Item`, `Separator`, `Content` plus `SectionNav*` aliases, matching `src/lib/components/ui/tabs/index.ts`), `section-nav.svelte`, `section-nav-list.svelte`, `section-nav-item.svelte`, `section-nav-separator.svelte`, `section-nav-content.svelte`, and `context.ts`.

- **Root** wraps bits-ui `Tabs.Root` with `orientation="vertical"` and `activationMode="manual"` (Resolved Decision 11), exposes a bindable `value` and an `onValueChange` callback, and wraps its children in bits-ui `Tooltip.Provider`, because bits-ui 2.18.1 `Tooltip.Root` reads the provider context and fails without one. Root is the size container: it carries the named container `@container/section-nav` and fills the width it is given, so a consumer that lets Root fill the dashboard body gets the dashboard-body measurement Resolved Decision 9 asks for. Root publishes the active value through a Svelte context defined in `context.ts`, and carries `data-section-nav` as the e2e hook for a rendered dashboard body.
- **List** wraps `Tabs.List` and requires an accessible label. Collapsed width is `w-12`; expanded width is `10rem` and applies only under the container variant `@min-[48rem]/section-nav:` (Resolved Decision 10).
- **Item** wraps `Tabs.Trigger` with props `value`, `label` (a plain string) and an `icon` snippet. The label is always in the DOM as the item's accessible name: visually hidden by default (`sr-only`) and shown under `@min-[48rem]/section-nav:not-sr-only`. A retro-ui-svelte tooltip shows the label, and the tooltip opens only while the label element is visually hidden (a rendered width of at most 1px, the `sr-only` size; an `$effect` closes the tooltip when it opens against a wider label), so the container query stays the single source of the collapsed state. Items carry `data-section-nav-item={value}` and `data-state` (from bits-ui) as e2e hooks.
- **Separator** renders a horizontal rule between items (`src/lib/components/ui/separator/` restyled for the list).
- **Content** always renders bits-ui `Tabs.Content` (so every trigger's `aria-controls` points at an existing element) but renders its children only when its `value` equals the active value read from the Root context. The laziness therefore lives inside the component, and no consumer can mount every section by accident.

Styling is by hand to the retro look: `border-2`, `rounded-md`, `shadow-md` on the list, the active item filled `bg-primary text-primary-foreground` like the active state in `src/lib/components/ui/tabs/tabs-trigger.svelte`, `focus:shadow-xs` on items. The tooltip comes from `pnpm dlx shadcn-svelte@latest add https://retroui-svelte.netlify.app/r/tooltip.json` (approved by the user), which adds `src/lib/components/ui/tooltip/` (`index.ts`, `tooltip-content.svelte`, `tooltip-trigger.svelte`).

### Overview

`src/lib/workspace/DashboardOverview.svelte` holds the launcher buttons moved out of `WorkspaceDashboard.svelte` (the `LAUNCHABLE_APPS` loop, `launch()`, the `launcherAction` runner and its error line), `AgentManager.svelte`, and `src/lib/workspace/StatusSummary.svelte`.

`StatusSummary.svelte` renders one line of up to three parts, each a button that switches the dashboard to a section:

- **Network** (opens `network`): a placeholder until the proxy-status store answers, then "recorded" (sidecar exists, running, and `healthy` is not false), "stopped" (exists, not running), "unhealthy" (running, `healthy` false), "not recorded" (no sidecar), or "unknown" (the `proxy_status` call failed).
- **Ports** (opens `ports`): the count of `openForwardsFor(sandboxName)` (`src/lib/network/state.svelte.ts`) entries whose kind is `exposed`, the same filter `PortsManager.svelte` applies.
- **Remote** (opens `remote`), shown only when the `remote` section is visible: "Local" when `deploymentFor(sandboxName)` (`src/lib/sandbox/state.svelte.ts`) is undefined, otherwise the deployment host and `translateStatus()` (`src/lib/sandbox.ts`) of its status.

### Fast first paint

`WorkspaceDashboard.svelte` stops calling `list_sandboxes`. The dashboard derives `workspacePath` and `provider` as two separate primitive `$derived` values from the entry of `sandboxesList()` (`src/lib/sandbox/state.svelte.ts`) whose `name` equals `sandboxName`. The header renders on the first frame; the section body renders as soon as the store holds the sandbox, which is the first frame whenever the dashboard is opened from the projects list. When the sandbox is missing from the store after `ensureSandboxRemoteState()` resolves (a sandbox created moments ago), the dashboard calls `refreshSandboxState()` once; the body shows a loading placeholder until the entry appears.

### Per-sandbox stores

Three new module-scoped stores follow the shape of `src/lib/workspace/packages.svelte.ts`: a `$state` record keyed by sandbox name, an in-flight promise map that de-duplicates concurrent callers, and accessor functions.

- **Proxy status, `src/lib/proxy/status.svelte.ts`.** Owns the one `ProxyStatus` TypeScript interface (mirroring Rust `proxy::types::ProxyStatus`), replacing the copies in `ProxyPanel.svelte` and `TrafficPanel.svelte`. Exports `proxyStatusFor(name)` (value, error, loading), `ensureProxyStatus(name)` (fetches through `proxy_status` when there is no value or the value is older than `PROXY_STATUS_MAX_AGE_MS`, 5 seconds; Resolved Decision 12), `refreshProxyStatus(name)` (always fetches, de-duplicated), and `forgetProxyStatus(name)`. Consumers on mount: `StatusSummary.svelte`, `ProxyPanel.svelte`, `TrafficPanel.svelte`. Callers of `refreshProxyStatus` after a mutation: `TrafficPanel.svelte` after `restart_proxy`, and `ProxyPanel.svelte`'s `onReapplied` handler after `NetPolicyEditor.svelte` runs `reapply_sandbox_network`.
- **Agent auth, `src/lib/agents/authStatus.svelte.ts`.** Exports `agentAuthFor(name)` (a record from agent id to `AgentAuthStatus`, `src/lib/agents/types.ts`), `refreshAgentAuth(name, agents)` and `forgetAgentAuth(name)`. `refreshAgentAuth` issues one `get_agent_auth_status` call per installed agent with an `auth` descriptor, all at once, and writes each agent's entry as its own call settles, applying the expiry check `loadAgentStatuses()` applies today. A failed call writes the not-authenticated status, as today. `AgentManager.svelte` keeps the cached entries on screen and calls `refreshAgentAuth` on mount, whenever `installedAgents` changes, and after `useSavedLogin()`.
- **Section memory, `src/lib/workspace/dashboard.svelte.ts`.** Exports `sectionFor(name)` (default `overview`), `setSection(name, id)` and `forgetSection(name)`. Frontend memory only: the remembered section survives navigation and sandbox switches (including the `{#key dashboardSandbox}` remount in `src/lib/workspace/WorkspaceContent.svelte`) but not an app restart or a webview reload. When the remembered section is not visible for the sandbox, the dashboard shows `overview` (Resolved Decision 16).

`AgentManager.svelte`'s two remaining mount calls, `list_credential_bundles` and `inference_status`, run concurrently instead of one after the other; both stay component-local because both are cheap app-side reads.

### Network section and secrets

The Network section keeps `ProxyPanel.svelte` as its entry point and restructures the panel's markup into headings, without changing any backend call:

- **Traffic inspection** (existing heading `proxy_title`): the proxy status line, read from the proxy-status store.
- **Network policy** (microsandbox only): `NetPolicyEditor.svelte` as today: drift banner, egress mode, rules, violation action, Save, violations list, viewer switch and re-apply.
- **Secrets**: for Podman, the `.mio/proxy-rules.json` injection-rule list, the add-rule row and the set-secret-value row from `ProxyPanel.svelte`; for microsandbox, the secret-binding list, the bind row, the set-secret-value row and the echo warning from `NetPolicyEditor.svelte`. The microsandbox secret bindings are part of the one `.mio/net-policy.json` draft and stay inside `NetPolicyEditor.svelte` under their own heading, saved by the editor's single Save button (Resolved Decision 15).

### Traffic section

`TrafficPanel.svelte` reads the proxy-status store instead of calling `proxy_status`, and keeps starting the flow watch (`ensureTrafficState()` and `watchFlows()` in `src/lib/proxy/traffic.svelte.ts`) when the sidecar exists. With no sidecar, the panel renders the existing `proxy_off_podman` or `proxy_off_msb` message instead of nothing, so the section is never blank.

## Resolved Decisions

User decisions of 2026-11-08, carried forward unchanged from `docs/features/manage-dashboard-redesign/01_ideas.md`:

1. **Navigation shape:** a settings-style section sidebar inside the dashboard, built from `SectionNav` in `src/lib/components/ui/section-nav/`.
2. **Overview contents:** the Terminal and Agents launchers, coding-agent install and login (`AgentManager.svelte`), and the status summary (`StatusSummary.svelte`); Tools, Ports, Network, Traffic, Remote and Danger zone are sidebar sections.
3. **Traffic UI:** `TrafficPanel.svelte` stays and moves under the Traffic section; no `mitmweb`.
4. **Secrets placement:** secrets stay inside the Network section under their own heading (mechanism in Resolved Decision 15).
5. **Summary proxy source:** `proxy_status` through the proxy-status store `src/lib/proxy/status.svelte.ts`, the most correct source even if slower.
6. **Narrow-width behaviour:** the section list collapses to icons with tooltips, in the reusable retro-styled `src/lib/components/ui/section-nav/` component rather than the shadcn-svelte `sidebar`.
7. **Section memory:** reopening a sandbox restores its last section through `src/lib/workspace/dashboard.svelte.ts`.
8. **Tools on Overview:** not needed; installing an agent from Overview already installs the agent's mise packages.
9. **Collapse width:** the user first asked for a collapse when the expanded list would take more than 10% of the dashboard body's width, then on 2026-11-08 delegated the threshold to the planning agent; Resolved Decision 10 replaces the 10% rule.

The Open Questions of `01_ideas.md`, restated with their resolution:

- **Secrets placement** (own section or inside Network): inside Network under a Secrets heading, per Resolved Decisions 4 and 15.
- **Proxy presence for the summary** (`list_sandboxes` field or `proxy_status`): `proxy_status` through the shared store, per Resolved Decisions 5 and 12; the `Sandbox` IPC shape in `src/lib/sandbox.ts` does not change.
- **Narrow-width behaviour** (icons, dropdown or horizontal strip): icons with tooltips, per Resolved Decision 6.
- **Section memory** (last section or always Overview): last section per sandbox, per Resolved Decisions 7 and 16.
- **Tools on Overview**: no Overview flow needs Tools, per Resolved Decision 8.
- **Collapse width**: a `48rem` container-query breakpoint on the dashboard body, so the list follows the navigation column's expanded or rail state, per Resolved Decisions 9 and 10.

Decisions taken by the planning agent on 2026-11-08 where the ideation was silent. The user confirmed Resolved Decisions 11 to 17 on 2026-11-08 and delegated Resolved Decision 10 to the agent, which settled it as written below.

10. **Collapse threshold follows the navigation column (agent decision, user-delegated).** The expanded list width is fixed at `10rem`, and the container variant that expands the list is `@min-[48rem]/section-nav:` on `SectionNav.Root`, which fills the dashboard body. Both `@min-[48rem]` literals live in `section-nav-list.svelte` and `section-nav-item.svelte`; comment both ends with the other's path. At the default 1024 logical-px window (`inner_size` in `src-tauri/src/workspace/mod.rs`) the body is about 644px with the navigation column expanded (380px, `NAVIGATION_PANEL_EXPANDED_W`) and about 976px with the rail (48px, `NAVIGATION_PANEL_RAIL_W`), so the list shows icons beside the expanded navigation column and labels beside the rail; from a window of about 1150px the list keeps its labels either way. The user's 10% rule was rejected after review: at a 10rem list it expands only from a 1600px body, which leaves the list icon-only on default and most laptop windows.
11. **Manual tab activation (agent decision, user-confirmed).** `SectionNav.Root` uses bits-ui `activationMode="manual"`: arrow keys move focus between items, Enter, Space or a click switches section. Automatic activation would mount, and fetch for, every section the keyboard focus passes over, defeating lazy mounting.
12. **Proxy-status freshness (agent decision, user-confirmed).** A cached `proxy_status` answer younger than `PROXY_STATUS_MAX_AGE_MS` (5 seconds, in `src/lib/proxy/status.svelte.ts`) is reused by `ensureProxyStatus`; an older one is refetched, and every proxy mutation calls `refreshProxyStatus`. Opening Overview then Network within 5 seconds costs one `proxy_status` call; returning after a minute revalidates, so a crashed sidecar does not stay hidden behind the cache.
13. **Baseline timing does not block the build (agent decision, user-confirmed).** Phase A records timings when a real Podman sandbox is available, but Phases B to D proceed without the numbers: each change (no render gate, one `proxy_status` per open, concurrent auth checks, lazy sections) is justified by the cost classes in `01_ideas.md` alone. The planning agent could not run a real Podman sandbox and recorded no numbers.
14. **Traffic is always visible (agent decision, user-confirmed).** The `traffic` section is shown for every sandbox, because having a sidecar is a per-sandbox creation choice that the microsandbox re-apply flow can change, not a provider capability; the hidden-control invariant in `docs/architecture/overview.md` covers only provider capabilities. Without a sidecar the section shows the existing "viewer off" message.
15. **Microsandbox secrets share the policy Save (agent decision, user-confirmed).** The microsandbox secret bindings stay inside `NetPolicyEditor.svelte`, under a Secrets heading, saved together with the policy by the editor's one Save button, because both live in one `.mio/net-policy.json` draft. Splitting the editor into two components would need a shared draft store and turns a re-housing into a rewrite.
16. **Section memory fallback and cleanup (agent decision, user-confirmed).** A remembered section that is not visible for the sandbox (for example `remote` before provider capabilities load) shows `overview` without overwriting the memory; destroying a sandbox from the Danger zone calls `forgetSection`, `forgetProxyStatus` and `forgetAgentAuth`, so a recreated sandbox with the same name starts clean.
17. **Unsaved section state on section switch (agent decision, user-confirmed).** Leaving a section unmounts the section, so an unsaved Network policy draft, half-typed inputs and the inline agent-install log are discarded, exactly as a sandbox switch discards them today; no leave-guard is added. A running action is not lost: the dashboard-level action runner keeps the `StatusRow` and button-disabling across sections, and an agent install stays visible as an `agent-install` task in the activity center (`src/lib/activity/activity.svelte.ts`).
18. **No new host-global state.** The feature adds no path the app reads or writes, no host port, no host-global external resource, no bundled binary, no teardown and no harness environment variable; every new store is frontend memory in the webview. Specs touching the dashboard keep their current tiers and run in parallel worktrees as today.

## Remaining: Phase A — Baseline timing measurement

No code is committed in Phase A. Every Podman and `msb` invocation is already logged with its duration (`ms`) by the process seam in `src-tauri/src/process/mod.rs` (the `podman ok` line, and `log_call_outcome!` for `msb`), into `mio.log` in the log directory that the `logging initialized` line names at startup (`init_logging()` in `src-tauri/src/logging.rs`).

- Against a real Podman sandbox with two authenticating agents installed (Claude Code and OpenCode), run `pnpm tauri dev`, open the sandbox's Manage dashboard from the projects list, and copy every `podman`/`msb` line logged from the click until the log goes quiet.
- Attribute each line to a mount-time call from the table in `01_ideas.md` (Current State): `podman ps` for `list_sandboxes` and each `proxy_status`, `podman exec` for each `get_agent_auth_status`.
- Record time-to-first-content (the WebView2 DevTools Performance panel, from click to the first panel paint) before Phase B and after Phase D.
- Write the numbers into a `## Measurements` section of this document, with the date and the machine.

Exit: the Measurements section exists, or this phase is marked deferred in `feature.md` if no Podman machine is available (Resolved Decision 13).

Not run during implementation (2026-11-08): the Podman machine was running, but the host had no sandbox and no installed agents, and the click-to-first-paint measurement needs an interactive WebView2 DevTools session. No numbers were recorded. Phases B to D shipped without them, per Resolved Decision 13.

## Implemented: Phase B — Shared stores and fast first paint

The single-page layout stays; Phase B removes the blocking and duplicate calls so the change ships, and is measurable, on its own.

- `src/lib/workspace/WorkspaceDashboard.svelte`: delete the `onMount` `list_sandboxes` call; derive `workspacePath` and `provider` from `sandboxesList()`; call `ensureSandboxRemoteState()` on mount and `refreshSandboxState()` once when the sandbox is missing; gate the body on the store entry instead of on a fetched `workspacePath`.
- New `src/lib/proxy/status.svelte.ts` with `ProxyStatus`, `proxyStatusFor`, `ensureProxyStatus`, `refreshProxyStatus`, `forgetProxyStatus` and `PROXY_STATUS_MAX_AGE_MS`.
- `src/routes/projects/ProxyPanel.svelte`: drop the local `ProxyStatus` interface and the `proxy_status` call in `refresh()`; read `proxyStatusFor`, call `ensureProxyStatus` on mount and `refreshProxyStatus` in the `onReapplied` handler passed to `NetPolicyEditor.svelte`; keep `list_proxy_rules` component-local.
- `src/routes/projects/TrafficPanel.svelte`: drop the local `ProxyStatus` interface and the `proxy_status` call; read the store, call `refreshProxyStatus` after `restart_proxy`; keep the `watching` guard around `watchFlows()`.
- New `src/lib/agents/authStatus.svelte.ts` with `agentAuthFor`, `refreshAgentAuth`, `forgetAgentAuth`.
- `src/routes/projects/AgentManager.svelte`: replace `agentStatuses` and the serial `loadAgentStatuses()` with the agent-auth store; run `loadCredentialBundles()` and `loadInferenceStatus()` concurrently.
- `src/AGENTS.md`: add the three new stores (proxy status, agent auth, and, in Phase D, section memory) to the Module-Scoped Rune Stores list.

Exit: `pnpm check` passes; `./build.exe e2e light` stays green with no spec change; opening a dashboard logs one `proxy_status`-driven listing instead of two and no dashboard-side `list_sandboxes`.

## Implemented: Phase C — Tooltip and the section-nav component

- Run `pnpm dlx shadcn-svelte@latest add https://retroui-svelte.netlify.app/r/tooltip.json`, producing `src/lib/components/ui/tooltip/`. Check the generated imports resolve (`$UTILS$` placeholder replaced with `$lib/utils`).
- Create `src/lib/components/ui/section-nav/` (`index.ts`, `section-nav.svelte`, `section-nav-list.svelte`, `section-nav-item.svelte`, `section-nav-separator.svelte`, `section-nav-content.svelte`, `context.ts`) as described in Section-nav component.
- Root `AGENTS.md`, UI Components list: add `tooltip`, and add `section-nav` with a note that retro-ui-svelte has no `sidebar` item, so `section-nav` is hand-built on bits-ui vertical `Tabs` and hand-restyled, like `field` and `input-group`.

Exit: `pnpm check` passes. The component has no consumer yet; Phase D proves it in the app.

## Implemented: Phase D — Dashboard restructure

- New `src/lib/workspace/sections.ts`: `DashboardSectionId`, `DASHBOARD_SECTIONS` (id, icon, label function, `visible`), with labels as `TODO(i18n)` English literals.
- New `src/lib/workspace/dashboard.svelte.ts`: `sectionFor`, `setSection`, `forgetSection`.
- New `src/lib/workspace/DashboardOverview.svelte` and `src/lib/workspace/StatusSummary.svelte` as described in Overview.
- `src/lib/workspace/WorkspaceDashboard.svelte`: replace the stacked panels with `SectionNav.Root` bound to `sectionFor(sandboxName)` through `setSection`; render one `SectionNav.Item` per visible catalogue entry and a `SectionNav.Content` per section; move the launcher markup and `launch()` into `DashboardOverview.svelte`; render the Destroy button in the `danger` section; call the three `forget*` functions after a successful `destroySandbox()`; change `<main>` into a full-height flex column whose content region scrolls.
- `src/routes/projects/ProxyPanel.svelte` and `src/routes/projects/NetPolicyEditor.svelte`: regroup the existing markup under Traffic inspection, Network policy and Secrets headings, without changing any `invoke` call.
- `src/routes/projects/TrafficPanel.svelte`: render the `proxy_off_podman`/`proxy_off_msb` message when the sidecar does not exist; the panel takes an optional `provider` prop to pick the message.
- `src/lib/workspace/packages.svelte.ts`: update the `ensurePackages` doc comment, which names the components that mount together (PackageManager now mounts only in the Tools section).
- `e2e/test/pageobjects/workspace-dashboard.page.ts`: `waitForRendered` waits on `[data-section-nav]` (an element present for every section) instead of the package input, which now exists only in the Tools section; add `openSection(id)`, which clicks `[data-section-nav-item="<id>"]` and waits for its `data-state="active"`; scope `installedPackages()` to the active section's content instead of every `span.font-mono` in the document.
- Adapt the specs that query a panel outside Overview, so each opens the section first. Coverage of the new behaviour (collapse threshold, section memory, lazy mounting, summary links) belongs to `03_e2e.md`, written by the `feature-e2e` skill.

| Spec | Section the spec must open | Note |
|---|---|---|
| `e2e/test/specs/light/workspace-dashboard-switch-ui.spec.ts` | `tools` | Section memory is per sandbox: sandbox A keeps `tools` across the A/B switch, so A's reopen step needs no `openSection`, while sandbox B opens on Overview and the spec opens `tools` before asserting A's package is absent |
| `e2e/test/specs/light/microsandbox-provider-ui.spec.ts` | `remote` (Podman) | For microsandbox, assert the `remote` item is absent; asserting the Deploy button absent is vacuous once sections mount lazily |
| `e2e/test/specs/standard/expose-port.spec.ts` | `ports` | |
| `e2e/test/specs/standard/package-autocomplete.spec.ts` | `tools` | |
| `e2e/test/specs/standard/runtime-management.spec.ts` | `tools` | |
| `e2e/test/specs/standard/embedded-workspace.spec.ts` | `tools` | Uses `packageInput` directly |
| `e2e/test/specs/standard/agent-setup.spec.ts` | none (Overview) | Affected only through `waitForRendered`; the page reload between steps resets section memory to Overview |
| `e2e/test/specs/standard/agent-web.spec.ts` | none (Overview) | Affected only through `waitForRendered` |
| `e2e/test/specs/standard/opencode-agent.spec.ts` | none (Overview) | Affected only through `waitForRendered` |

`e2e/test/pageobjects/project-row.component.ts` also drives the dashboard (`deployTo`, `undeploy`, `destroy`, `toggleSyncOffOn`); each now opens `remote` (deploy, undeploy, sync) or `danger` (destroy) after `waitForRendered`, which keeps `standard/deploy-remote.spec.ts` and every spec that destroys through the page object working.

`e2e/test/specs/light/microsandbox-network-policy-ui.spec.ts` was checked and is not affected: the spec drives policy and secrets through `invokeCmd` and never opens the dashboard.

Exit: `pnpm check` passes; `./build.exe e2e light` is green; the user reviews the working dashboard (wording and look) before Phase E.

## Implemented: Phase E — i18n and documentation close-out (after user approval)

The user approved the working feature on 2026-11-09. The new keys carry a `dashboard_` prefix; the Traffic section label reuses `traffic_title`, and the summary's open-port count is one Paraglide plural message (`dashboard_summary_ports`), since Russian needs more than two plural forms.

- Run only after the user approves the working feature, per `.claude/skills/feature-implement/SKILL.md` (i18n section): replace every `TODO(i18n)` literal from Phases B to D (section labels, the section list's accessible label, summary parts, the Secrets heading, the loading placeholder) with Paraglide keys in `messages/en.json`, mirrored to `messages/de.json` and `messages/ru.json`, until no `TODO(i18n)` remains. Reuse existing keys where the wording matches (`proxy_title`, `traffic_title`, `netpolicy_title`, `projects_destroy`, `proxy_off_podman`, `proxy_off_msb`).
- `docs/architecture/workspace-ui.md`, Tabs section: describe Manage as an Overview plus lazily mounted sections whose last selection is remembered per sandbox, and add the invariant that a dashboard section fetches only while shown.
- `feature.md`: `status: implemented`, status note, and a `**Implement**` entry in `docs/features/log.md`; run `./build.exe docs --fix` then `./build.exe docs`.

## Lifecycle and Cross-Cutting Concerns

- **State derivation.** The dashboard's sandbox facts come from `src/lib/sandbox/state.svelte.ts`, proxy state from `src/lib/proxy/status.svelte.ts`, agent auth from `src/lib/agents/authStatus.svelte.ts`, packages from `src/lib/workspace/packages.svelte.ts`, ports from `src/lib/network/state.svelte.ts`, and the active section from `src/lib/workspace/dashboard.svelte.ts`. No section component calls `proxy_status`, `list_sandboxes` or `get_agent_auth_status` directly.
- **Cleanup.** Section unmount stops section-local pollers (the `NetPolicyEditor.svelte` violation poll returns its interval from its `$effect`), so a hidden section costs nothing. The backend flow poller started by `watch_proxy_flows` keeps running after the Traffic section closes, as it already does after the dashboard closes; the `proxy://flows-changed` subscription in `src/lib/proxy/traffic.svelte.ts` keeps accumulating, so reopening Traffic shows the history without a gap. Destroy clears the three new per-sandbox stores (Resolved Decision 16).
- **Superseded approaches.** The dashboard-local `list_sandboxes` call in `WorkspaceDashboard.svelte` is removed because the shared store already holds the same list and the call blanked the body for one full provider listing on every open and switch; do not reinstate the call for a missing sandbox, use `refreshSandboxState()`. The per-panel `proxy_status` calls and duplicated `ProxyStatus` interfaces in `ProxyPanel.svelte` and `TrafficPanel.svelte` are replaced by `src/lib/proxy/status.svelte.ts`, because each call lists every instance of the provider. The serial `loadAgentStatuses()` loop in `AgentManager.svelte` is replaced by concurrent calls in the agent-auth store. The `{#key dashboardSandbox}` remount in `WorkspaceContent.svelte` stays: the remount is the fix guarded by `workspace-dashboard-switch-ui.spec.ts`, and section memory survives the remount through its store.
- **Planned features landing in the Network section.** `docs/features/proxy-toggle-management/`, `docs/features/proxy-credential-secrets/` and `docs/features/proxy-network-policy/` (all `planned`) add controls to `ProxyPanel.svelte` or `NetPolicyEditor.svelte`. Those controls land in the Network section, read proxy state from `src/lib/proxy/status.svelte.ts`, and call `refreshProxyStatus` after any mutation that changes the sidecar.
- **Out of scope.** A refresh of agent auth status when the separate `/agent-auth` login window finishes is not added: the dashboard already learns the new status only on remount, and the store keeps that behaviour (revalidation on every Overview mount). The host-native `mitmdump` saving stays in `04_improvements.md`.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Every section's backend calls still run on dashboard open | bits-ui 2.18.1 `Tabs.Content` (`node_modules/bits-ui/dist/bits/tabs/components/tabs-content.svelte`) always renders its children and only hides inactive panels | Render section children only through `SectionNav.Content` in `src/lib/components/ui/section-nav/section-nav-content.svelte`, which gates children on the active value from the Root context |
| Arrowing through the section list fires every section's fetches | bits-ui `Tabs` with `activationMode="automatic"` activates, and so mounts, each item the keyboard focus lands on | Keep `activationMode="manual"` on `SectionNav.Root` (`docs/features/manage-dashboard-redesign/02_dev.md`, Resolved Decision 11) |
| "Tooltip.Provider" context error when a collapsed section-nav item is hovered | bits-ui `Tooltip.Root` reads the `Tooltip.Provider` context and the retro-ui-svelte tooltip does not add a provider | `SectionNav.Root` wraps its children in bits-ui `Tooltip.Provider`; consumers add nothing |
| The section list collapses or expands at the wrong width | The `@min-[48rem]/section-nav:` variants in `section-nav-list.svelte` and `section-nav-item.svelte` drifted apart, or `SectionNav.Root` does not fill the dashboard body | Keep the two breakpoint literals equal, per `docs/features/manage-dashboard-redesign/02_dev.md`, Resolved Decision 10, and let `SectionNav.Root` take the full body width in `WorkspaceDashboard.svelte` |
| A container-query class on the section list has no effect | Tailwind 4 only generates classes it finds as literal strings, so a variant assembled at runtime is never emitted | Write `@min-[48rem]/section-nav:` variants as full literal class strings |
| The tooltip still appears on hover when the list is expanded | The tooltip content is portalled to `document.body`, outside the `section-nav` container, so container variants cannot hide the tooltip | Open the tooltip only while the item's label element is visually hidden (rendered width at most 1px, the `sr-only` size), as `section-nav-item.svelte` does |
| Collapsed items are unlabeled for screen readers | The label was removed from the DOM instead of visually hidden | Keep the label rendered with `sr-only` below the threshold and `not-sr-only` above it, so the label stays the item's accessible name |
| Dashboard effects rerun every 3 seconds | The dashboard derived the whole `Project` object from `sandboxesList()`, whose poll in `src/lib/sandbox/state.svelte.ts` replaces every object, so the identity changes on each poll | Derive `workspacePath` and `provider` as separate primitive `$derived` values in `WorkspaceDashboard.svelte` |
| A just-created sandbox's dashboard shows only the loading placeholder | `sandboxesList()` in `src/lib/sandbox/state.svelte.ts` has not yet polled the new sandbox | Call `refreshSandboxState()` once when the sandbox is missing, never a dashboard-local `list_sandboxes` |
| The Overview summary contradicts the Network or Traffic section | A consumer called `proxy_status` itself, or a proxy mutation did not refresh the store | Read proxy state only from `src/lib/proxy/status.svelte.ts` and call `refreshProxyStatus` after `restart_proxy`, `reapply_sandbox_network` or any later sidecar-changing command |
| A recreated sandbox briefly shows the destroyed sandbox's proxy, auth or section state | The per-sandbox stores are keyed by name and a destroy did not clear them | Call `forgetProxyStatus`, `forgetAgentAuth` and `forgetSection` after a successful destroy in `WorkspaceDashboard.svelte` |
| The dashboard opens on Overview although the user left the sandbox on Remote | The remembered section is hidden until `capabilitiesFor(provider)` in `src/lib/sandbox/state.svelte.ts` loads, so the dashboard falls back to Overview | Expected fallback; keep the memory unchanged on fallback so the section returns once capabilities load (`docs/features/manage-dashboard-redesign/02_dev.md`, Resolved Decision 16) |
| A light spec asserting a control is absent passes vacuously | Sections mount lazily, so a control in a closed section is absent whether or not the provider gate works | Assert on the presence of the `[data-section-nav-item]` for the section, or open the section before asserting on its controls |
| A spec times out in `waitForRendered` or cannot find a panel's inputs | `e2e/test/pageobjects/workspace-dashboard.page.ts` waited on the package input, which exists only in the Tools section | Wait on `[data-section-nav]` and call `openSection(id)` before querying a section's controls |
| `installedPackages()` returns proxy rules or other monospace text | The page object read every `span.font-mono` in the document | Scope the query to the active section content in `e2e/test/pageobjects/workspace-dashboard.page.ts` |
| `pnpm dlx shadcn-svelte@latest add <registry url>` changes `package.json` and `pnpm-lock.yaml` | The CLI installs the component's dependencies at their newest versions, so it bumped the pinned `bits-ui` (2.18.1 to 2.20.6) and `tailwind-variants` (3.2.2 to 4.0.1) | Revert `package.json` and `pnpm-lock.yaml` after adding a component, then run `pnpm install --frozen-lockfile`; Phase C did this for the tooltip |
| The tooltip stays open after the list expands, or opens on an expanded list | bits-ui `Tooltip.Root` assigns its own `open` prop on hover, so a one-way `open` prop plus an `onOpenChange` veto leaves the tooltip open | Bind `open` and close it from an `$effect` when the label is wider than 1px, as `section-nav-item.svelte` does |
| An unsaved network policy edit disappears after visiting another section | Leaving a section unmounts the section and discards component-local drafts | Expected behaviour (`docs/features/manage-dashboard-redesign/02_dev.md`, Resolved Decision 17); save before switching |
