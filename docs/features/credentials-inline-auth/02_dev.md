---
type: dev-plan
title: "Credentials Inline Auth Dev Plan"
feature: credentials-inline-auth
---
# Credentials Inline Auth Dev Plan

This document is the build plan for moving the Claude Code login UI on the Credentials page (`src/routes/credentials/+page.svelte`) from a separate `WebviewWindow` into an inline expandable card, continuing `docs/features/credentials-inline-auth/01_ideas.md`. It supersedes one ideas-document choice (see Resolved Decisions: "Fallback window removal timing") after discovering during planning that `src/routes/agent-auth/+page.svelte` is shared infrastructure, also used by `src/routes/projects/AgentManager.svelte` for per-sandbox logins — it is not deleted by this feature. Status: nothing in this plan is implemented yet; all phases (A–E) are remaining work.

## The Concrete Model

The credentials page keeps its own `logIn()` flow (`ensure_auth_container` → `install_agent` → `start_agent_auth`) but stops opening a `WebviewWindow` on success. Instead, a new module-scoped store, `src/lib/agents/authSession.svelte.ts`, attaches `listen("agent-auth-output-<sessionId>", ...)` directly and exposes reactive state: output lines, detected OAuth URL, an "awaiting code" flag, and exit code. `src/routes/credentials/+page.svelte` renders each service row inside a shadcn-svelte `Collapsible` (new primitive, `src/lib/components/ui/collapsible/`), reading the store's state to show a spinner (before a URL is found), the URL plus an "Open in browser" button (once found), a code-input box (once the CLI is awaiting a code), and a "Show details" toggle that reveals the raw output lines the store already buffers. The page's own `Exit`-handling logic (today split between the page's own one-shot Exit listener and the separate window's `onMount` listener) is consolidated into a single handler in `credentials/+page.svelte` that calls `capture_agent_credentials`, `seed_agent_onboarding` (ported from `agent-auth/+page.svelte`'s `onMount`, currently the only place that seeds onboarding for the ephemeral auth container), then `destroy_sandbox`. `src/routes/agent-auth/+page.svelte` and its usage from `src/routes/projects/AgentManager.svelte:132-153` (`openAgentAuth`) are untouched — the store is a new, additional consumer of the same `agent-auth-output-<sessionId>` event contract, not a replacement for the window's.

## Resolved Decisions

- **Raw output visibility**: the expanded card gets a "Show details" toggle (collapsed by default) that reveals the same raw output lines `agent-auth/+page.svelte`'s `<pre>` block shows today. The `authSession.svelte.ts` store keeps the full line buffer for the active session (sessions are short-lived, so no truncation is needed).
- **Auto-expand timing**: the card expands immediately when "Log in" is clicked, before a `sessionId` exists. It shows a generic "Signing in…" progress state during `ensure_auth_container`/`install_agent`, then the store-driven URL/code UI once `start_agent_auth` returns a `sessionId` and events start arriving.
- **Cancel affordance**: the expanded card gets a "Cancel" button wired to the existing `cancel_agent_auth` Tauri command. Manually collapsing the card (clicking the row header again) does not cancel the session — the store keeps listening in the background, and the collapsed row shows a "Signing in…" status derived from the store instead of the normal connected/not-connected status. Re-expanding reattaches to the same store state because the store is a module-scoped singleton, not component-local `$state`.
- **Concurrent logins**: v1 supports exactly one active session across all rows. This falls out of the existing `action.busy !== null` disabling of every row's `Log in`/`Log out` button in `credentials/+page.svelte` (`action` from `createActionRunner()`, `src/lib/actions.svelte`) — no new locking mechanism is needed. The `authSession.svelte.ts` store holds a single current session, not a map, matching this constraint; multi-session support is left for if/when the catalogue grows past one `credentialService`.
- **Fallback window removal timing — SUPERSEDES `01_ideas.md`**: the ideas document proposed deleting `src/routes/agent-auth/+page.svelte` entirely. That is now rejected: `src/routes/projects/AgentManager.svelte:132-153` (`openAgentAuth`) opens the same `/agent-auth` route for per-sandbox logins on a real sandbox, independent of the Credentials page. Deleting the route would break per-sandbox login in `AgentManager.svelte`, which is out of scope for this feature (the user's request was specific to the Credentials page). Only the `WebviewWindow` call inside `credentials/+page.svelte`'s `logIn()` (lines 87-93 today) is removed; the route file, its component, and `AgentManager.svelte`'s usage of it are untouched. The `credentials_auth_window_title` i18n key is still removed (see Phase D) because it was only ever used by the call site being deleted — `AgentManager.svelte`'s window title is a hardcoded string (`${descriptor.label} — authenticate`), not this key.

## Implementation Phases

## Implemented: Phase A — Collapsible UI primitive

- Add a shadcn-svelte `Collapsible` component: `pnpm dlx shadcn-svelte@latest add collapsible`, landing under `src/lib/components/ui/collapsible/` (mirroring the existing `src/lib/components/ui/dialog/` and `src/lib/components/ui/tabs/` shape — `.svelte` parts plus `index.ts`).
- No existing code references this primitive yet; this phase only adds the dependency and generated files, no wiring.

## Implemented: Phase B — Auth session store

- New file `src/lib/agents/authSession.svelte.ts`. Exposes a singleton (module-level `$state`) object with: `sessionId: string | null`, `phase: "idle" | "starting" | "awaiting-url" | "awaiting-code" | "exited"`, `outputLines: string[]`, `authUrl: string | null`, `exitCode: number | null`.
- Function `start(sessionId: string)`: attaches `listen<AgentAuthEvent>("agent-auth-output-" + sessionId, ...)` (same event name pattern used today in `credentials/+page.svelte:62` and `agent-auth/+page.svelte:53`), ports the URL regex (`/https:\/\/[a-z0-9.-]+\.[a-z]{2,}\/[^\s]+/i`) and the `"paste code"` substring check verbatim from `agent-auth/+page.svelte:58-61`, and updates `phase`/`authUrl`/`outputLines` as `Output` events arrive; sets `phase = "exited"` and `exitCode` on the `Exit` event.
- Function `reset()`: clears all state and unlistens, called once the credentials page has finished its own Exit handling (capture/seed/destroy) and the card is dismissed.
- This store's event contract does not change `src-tauri/src/agents/types.rs`'s `AgentAuthEvent` — detection stays client-side, matching `01_ideas.md`'s "URL/prompt detection location" decision.

## Implemented: Phase C — Wire the inline card into `credentials/+page.svelte`

- Remove the `WebviewWindow` import and the `new WebviewWindow(...)` call (current lines 87-93) from `logIn()`.
- Remove the ad-hoc `listen(...)` call already inside `logIn()` (current lines 62-85) and replace its `Exit`-branch body (currently `capture_agent_credentials` → `destroy_sandbox` in a `finally`) with a single consolidated Exit handler that additionally calls `seed_agent_onboarding` with `onboardingSeed(agent)` (import from `$lib/agents/catalogue`, same helper `agent-auth/+page.svelte:66-71` uses) before `capture_agent_credentials`, preserving today's ordering where onboarding is seeded into the auth container before its credential files are captured out.
- Call `authSession.start(sessionId)` right after `start_agent_auth` resolves; the consolidated Exit handler subscribes to the store's `phase === "exited"` transition (a Svelte `$effect`) instead of listening to the raw Tauri event a second time.
- Each service row (`{#each services as service}` block, current lines 112-139) wraps its existing header markup in a `Collapsible.Root`, bound open when `authSession.sessionId` is active for that row's `serviceId` (v1: any active session implies the one row that started it, per the single-session decision above) or the user has manually expanded/collapsed it.
- Expanded content, in order: progress text (`"starting"` phase), the detected URL with an "Open in browser" button (`openUrl` from `@tauri-apps/plugin-opener`, same call as `agent-auth/+page.svelte:103`), the code-input box calling `send_agent_auth_input` with `authSession.sessionId` once `phase === "awaiting-code"`, a "Cancel" button calling `cancel_agent_auth` with `authSession.sessionId`, and a "Show details" toggle revealing `authSession.outputLines`.
- After a successful Exit (`exitCode === 0`), call `authSession.reset()` and `loadBundles()` (existing function) to refresh the row back to its collapsed connected state.

## Remaining: Phase D — i18n keys

- Add to `messages/en.json` (then mirror to `messages/de.json` and `messages/ru.json`, per root `AGENTS.md`): keys for "Signing in…", "Open in browser", "Paste authorization code here…", "Submit", "Cancel", "Show details", "Hide details", "Authentication failed (exit code {code})." Exact key names follow the existing `credentials_*` prefix convention (e.g. `credentials_signing_in`, `credentials_open_in_browser`, `credentials_paste_code_placeholder`, `credentials_submit`, `credentials_cancel`, `credentials_show_details`, `credentials_hide_details`, `credentials_auth_failed`).
- Remove `credentials_auth_window_title` from `messages/en.json`, `messages/de.json`, and `messages/ru.json` — its only call site (`credentials/+page.svelte`'s `WebviewWindow` title) is deleted in Phase C, and `AgentManager.svelte`'s window title is a separate hardcoded string this feature does not touch.

## Remaining: Phase E — Optional cleanup (out of primary scope)

- `src/routes/agent-auth/+page.svelte` still contains its own copy of the URL/prompt-detection logic (`agent-auth/+page.svelte:58-61`) after Phase B introduces the same logic in `authSession.svelte.ts`. Refactoring the window component to consume the shared store instead of its local `$state` would remove this duplication, but is not required for the Credentials page feature to ship, since `AgentManager.svelte`'s per-sandbox flow is out of scope for this feature. Flagged here so a future agent does not reintroduce a third copy of the same regex when touching per-sandbox auth UI.

## Lifecycle / Cross-Cutting Concerns

- **Auth container cleanup on failure**: unchanged from today — the consolidated Exit handler in Phase C keeps the existing `finally`-style guarantee that `destroy_sandbox` runs for the ephemeral `mio-auth-<serviceId>` container regardless of whether `capture_agent_credentials` succeeds, same as current `credentials/+page.svelte:78-83`.
- **Store lifetime vs component lifetime**: `authSession.svelte.ts` is module-scoped, so its `listen()` subscription survives the credentials page row being collapsed (per the Cancel-affordance decision above). It does not survive full navigation away from `/credentials` combined with app reload; no persistence across app restarts is in scope, matching the ephemeral, single-session nature of a login attempt.
- **Session single-flight**: because `action.busy !== null` already disables every row's buttons while any `action.run(...)` call is in flight (existing behavior in `credentials/+page.svelte`), Phase C does not need new guard logic to prevent starting a second session while one is active.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Per-sandbox login from `AgentManager.svelte` stops working after this feature ships | A future agent deletes `src/routes/agent-auth/+page.svelte` believing it is now dead code, missing that `AgentManager.svelte:132-153` (`openAgentAuth`) still opens it for per-sandbox logins | Never delete `agent-auth/+page.svelte` as part of this feature; only the `WebviewWindow` call inside `credentials/+page.svelte`'s `logIn()` is removed |
| Onboarding never gets seeded into the ephemeral auth container, and captured credentials miss expected onboarding state | The seed call lived only in `agent-auth/+page.svelte`'s `onMount` `Exit` handler; removing the `WebviewWindow` call without porting `seed_agent_onboarding` + `onboardingSeed(agent)` into the new consolidated Exit handler silently drops it | Phase C explicitly ports the `seed_agent_onboarding` call, ordered before `capture_agent_credentials`, into `credentials/+page.svelte`'s new Exit handler |
| Expanded card shows no URL even though the CLI printed one | The 80ms idle-flush behavior of `stream_output` in `src-tauri/src/agents/mod.rs` is unchanged; the regex must be applied per `Output` event exactly as ported in Phase B, not re-derived | Copy the regex and `"paste code"` check verbatim from `agent-auth/+page.svelte:58-61` into `authSession.svelte.ts`, do not rewrite the detection logic |
| Collapsing a row mid-login silently cancels it, orphaning the auth container | A future implementation wires the `Collapsible`'s `onOpenChange` to call `cancel_agent_auth` | Per the Cancel-affordance decision, only the explicit "Cancel" button calls `cancel_agent_auth`; collapsing the row must not |
| `credentials_auth_window_title` still referenced after Phase D | The key is removed from the three locale files but a stale import or reference to `m.credentials_auth_window_title` remains in `credentials/+page.svelte` | Remove the `WebviewWindow` call (which was the only reference) in the same commit as the i18n key removal |
| Second login attempt while one is already expanded silently no-ops the button click | `action.busy !== null` already disables all `Log in`/`Log out` buttons project-wide during any in-flight `action.run` call | Expected behavior per the Concurrent-logins decision; do not add a separate per-row disabled check that could drift from `action.busy` |
