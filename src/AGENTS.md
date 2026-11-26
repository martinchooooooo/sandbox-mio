# Frontend Orientation

The `src/` tree is a SvelteKit app (Svelte 5 runes, static adapter, Tailwind 4) rendered inside the Tauri webview. All backend access goes through the typed wrappers in `src/lib/api/`, which call the commands listed in `src-tauri/src/commands/mod.rs`; there is no HTTP API. UI conventions (shadcn-svelte components, Paraglide i18n, error handling) are specified in the root `AGENTS.md` — read that first; only frontend-specific structure is documented here.

## Route Map

- `src/routes/projects/` — Home: the page plus the two components only it mounts, the sandbox card (`SandboxItem.svelte`) and the create card (`CreateSandboxItem.svelte`). Nothing under `src/lib/` imports from `src/routes/`; a component that library code mounts lives in a `src/lib/` feature folder instead.
- `src/lib/workspace/` — not a route: the app shell rendered by the root layout (app sidebar `AppSidebar.svelte`, sandbox view `SandboxView.svelte`, Manage dashboard and its title bar `SandboxTitleBar.svelte`, launchable-app catalogue in `apps.ts`); pane state comes from the Rust `WorkspaceRegistry` via `state.svelte.ts`, and every page/sandbox navigation action lives in `navigation.svelte.ts`. See `docs/architecture/workspace-ui.md`.
- `src/routes/models/` — local LLM catalogue, download, and inference control.
- `src/routes/agent-auth/`, `src/routes/credentials/` — agent login flow and shared credential bundles.
- `src/routes/settings/`, `src/routes/intro/` — settings page and first-run intro.
- `src/lib/onboarding/` — not a route: the illustrated onboarding book the welcome flow (`src/lib/welcome/`) shows, its reusable page kit, and the background first-run setup store. See `src/lib/onboarding/AGENTS.md` before adding a chapter.
- `src/lib/story-stage/` — not a route: the whiteboard animation engine used by the onboarding book. See `src/lib/story-stage/AGENTS.md`.
- Manage dashboard panels (mounted by `src/lib/workspace/WorkspaceDashboard.svelte` and `DashboardOverview.svelte`) live in the `src/lib/` folder of what they manage: `agents/`, `packages/` (mise tools, with their per-sandbox store), `network/` (ports and SSH access), `proxy/`, `net-policy/` (the egress policy editor, its review dialog and the policy types) and `remote/`. `StatusRow.svelte`, the busy/message/error line shared by Home cards, the title bar and the dashboard, is in `src/lib/sandbox/`.
- `src/routes/+layout.svelte` — mounts the single `<Toaster />`, the app sidebar (which holds the activity center), and the content area: the sandbox view when a sandbox is in view, otherwise the routed page.

## IPC

Components, stores and helpers never call `invoke()` from `@tauri-apps/api/core` directly. Every backend command the frontend calls has a typed async wrapper in `src/lib/api/`, one module per command domain file in `src-tauri/src/commands/`, and call sites import the wrapper.

- A wrapper passes its arguments through under the exact names the Rust command declares (Tauri converts to camelCase) and only rethrows: error handling stays at the call site through `parseError()`.
- Result and argument types sit beside the wrapper. A type that already lives in a feature's `types.ts` or store is imported, not copied; a type declared inside a component moves into the wrapper module.
- Channel parameters are typed per command (`Channel<T>`); creating the channel and its `onmessage` handler stays at the call site. Event subscriptions (`listen()`) are not wrapped; they stay in the owning store.
- A command with no frontend caller has no wrapper yet; add the wrapper with the first caller. A wrapper whose name collides with a store function is imported under a `...Command` alias.

## Module-Scoped Rune Stores

State shared across routes lives in `.svelte.ts` files under `src/lib/` with module-level `$state` declared above the router, exported through accessor functions, and hydrated by an idempotent `ensure*()`/start function called once from the root layout. Component-local `$state` is destroyed on navigation, so cross-route state must use a store following the established pattern. Existing stores:

- `src/lib/activity/activity.svelte.ts` — task ledger hydrated from `list_tasks`, kept live by `activity://task-updated`; also folds container status per sandbox from the `listen_events` stream.
- `src/lib/workspace/state.svelte.ts` — the single `workspace://state` subscription plus the sandbox in view; consumed by the layout and the app sidebar.
- `src/lib/proxy/status.svelte.ts` — per-sandbox `proxy_status` cache (5 s freshness) shared by the Manage dashboard's Overview summary, Network and Traffic sections; call `refreshProxyStatus` after any sidecar-changing command.
- `src/lib/agents/authStatus.svelte.ts` — per-sandbox coding-agent login status, checked concurrently and kept across dashboard remounts.
- `src/lib/workspace/dashboard.svelte.ts` — the Manage dashboard section last opened per sandbox (frontend memory; survives the dashboard remount on sandbox switch).
- `src/lib/onboarding/setup.svelte.ts` — first-run setup (machine, base image, model and the first sandbox) as one shared run, started in the background by the welcome flow and rendered by the four cards of `SetupProgress.svelte`.
- `src/lib/agent-chat/learning.svelte.ts` — per-sandbox chat learning-mode overrides from `settings.json`, loaded once by `ensureLearning()`.
- `src/lib/models/model.svelte.ts` — the one Model job (download, wake, sleep) plus machine specs, engine and serving status, shared by the Models page and first-run onboarding; hydrated by `loadModels()` on the Models page mount rather than an `ensure*()` from the root layout.
- `src/lib/registry.svelte.ts` — mise package registry cache (also the smallest reference example of the pattern).

Event flow is one-directional: Rust emits (`activity://task-updated`, `workspace://state`, container events), one store subscribes, components read accessors. Components must not call `listen()` for a stream a store already owns.

## Shared Non-Store Modules

- `src/lib/sandbox.ts` — `Project`/`Deployment` types, `translateStatus()` (backend status string → translation key), `syncColor()` (status dot idiom).
- `src/lib/errors.ts` — `parseError()` and `AppErrorKind`; mandatory in every catch block around a `src/lib/api/` call per the root `AGENTS.md`.
- `src/lib/agents/` and `src/lib/models/` — static catalogues plus per-agent provisioning descriptors; extend the catalogue file rather than branching on names in components.
- `src/lib/actions.svelte.ts`, `src/lib/utils.ts`, `src/lib/portal.ts` — Svelte actions, `cn()` class merging, portal helper.
- `src/lib/motion.ts` — Svelte transition presets (`rise`, `pop`) for the motion system below.

## Motion

Motion is applied at the shell and primitive level, so most new UI animates without opting in. Tokens and utilities live in `src/app.css`; exit presets live in `src/lib/motion.ts`.

- Already automatic: every routed page fades up on navigation (`data-route-outlet`), the sandbox view fades in (`SandboxView.svelte`), and every `tabs`/`section-nav`/`collapsible` content fade in, and dialogs and tooltips animate via their primitives. Links, buttons and tabs ease their hover colours by default.
- Something appearing: add a class, no JS. `animate-enter` for a surface arriving, `animate-fade-in` for content swapping in place, `animate-pop-in` for a floating element, `stagger` on a list or stack whose children should cascade.
- Something that must animate out: use `in:rise out:rise` or `in:pop out:pop` from `src/lib/motion.ts`, never Svelte's raw `fade`/`fly` and never `transition:` (it loses the faster exit).
- Do not nest movement: inside an element that already rises or slides, use `animate-fade-in`.
- Reduced motion is handled centrally: the tokens drop movement and keep fades, so do not add per-component `prefers-reduced-motion` checks, except for infinite indicator keyframes, which carry their own `no-preference` query (pattern: `src/lib/agent-chat/AgentAvatar.svelte`).

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Shared state resets when navigating between routes | `$state` declared inside a component is destroyed by the router | Move state into a module-scoped `.svelte.ts` store under `src/lib/` following `src/lib/registry.svelte.ts` |
| Duplicate toast/status updates for one backend event | A component calls `listen()` for a stream an existing store already subscribes to | Read the store accessor (`src/lib/activity/activity.svelte.ts` or `src/lib/workspace/state.svelte.ts`) instead of adding a `listen()` call |
| Dynamic key lookup on the Paraglide `m` object returns undefined | Paraglide generates one function per key; `m` cannot be indexed with a runtime string | Build an explicit key→function map, as in `kindLabels` in `src/lib/activity/activity.svelte.ts` and `translateStatus()` in `src/lib/sandbox.ts` |
| Error dialog shows `[object Object]` or raw JSON | A wrapper rejection stringified instead of parsed | Use `parseError(e).message` from `src/lib/errors.ts`; branch on `parseError(e).kind` for variant logic |
| A translated string silently shows English text in non-English locales | A translation key was added to `messages/en.json` but not mirrored to the other locale files, so Paraglide falls back to the base locale | Mirror every new key across all files in `messages/`, starting from `messages/en.json` |
| An e2e spec reports a freshly shown element as not displayed | WebDriver treats `opacity: 0` (on the element or any ancestor) as hidden, so a fade starting from 0 hides it mid-animation | Start fades at 0.01, as the keyframes in `src/app.css` and `MIN_OPACITY` in `src/lib/motion.ts` do; never write a custom fade from 0 |
| A `position: fixed` child is offset or clipped inside an animated container | A transform left on an ancestor turns it into the containing block | Animate with the `translate`/`scale` properties and `backwards` fill, as the `src/app.css` keyframes do; never `forwards`/`both` |
| New UI built with custom markup duplicates an existing primitive | shadcn-svelte component already exists in `src/lib/components/ui/` | Check `src/lib/components/ui/` first; add missing shadcn-svelte components per the root `AGENTS.md` |
