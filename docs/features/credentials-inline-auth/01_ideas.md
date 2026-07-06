# Credentials Inline Auth Ideas

This document covers the ideation for moving the Claude Code OAuth login UI from a separate Tauri `WebviewWindow` into an expandable inline card on the Credentials page (`src/routes/credentials/+page.svelte`). Today, clicking "Log in" opens a second window (`src/routes/agent-auth/+page.svelte`) that shows raw terminal output, extracts the OAuth URL via a client-side regex, and collects the pasted-back authorization code. Nothing of the inline-card design is implemented; the login machinery it reuses (subprocess spawn, output streaming, code submission) is fully implemented and unchanged by this feature. Related prior ideation lives in `docs/features/shared-agent-auth/01_ideas.md`, which designed the host-side credential store that this same login flow feeds into.

## Current State

- `src/routes/credentials/+page.svelte` — lists one row per catalogued service with a `credentialService` and `auth` entry (currently only `anthropic`/Claude Code), each row an inline `<div>` (not a component) with a status label and a `Log in`/`Log out` `Button`. `logIn()` (lines 38–95) runs `ensure_auth_container` → `install_agent` → `start_agent_auth` → opens a `WebviewWindow` at `/agent-auth?sandbox=...&agent=...&session=...` → waits only for that window's session to emit `Exit` → `capture_agent_credentials` → `destroy_sandbox`. This `WebviewWindow` call is the seam this feature removes.
- `src/routes/agent-auth/+page.svelte` — the separate window's page. Renders raw streamed output in a `<pre>` block, regex-scans each line for a URL (`https:\/\/[a-z0-9.-]+\.[a-z]{2,}\/[^\s]+`) and for the substring `"paste code"` to decide when to show a code-input box, and calls `openUrl` from `@tauri-apps/plugin-opener` for "Open in browser". This detection logic is the seam the inline card must reproduce.
- `src-tauri/src/agents/mod.rs` — `start_auth` spawns `claude auth login` inside a container via `podman exec` with piped stdio; `stream_output` reads raw bytes with an 80ms idle-timeout flush (not line-buffered `BufReader::lines()`) specifically because the CLI's paste prompt has no trailing newline; output is emitted as `agent-auth-output-<session_id>` events via `app.emit`; `send_auth_input` writes the pasted code to the child's stdin and removes the session from `AuthSessionMap` (one-shot, since PKCE only needs one round trip); `cancel_agent_auth` removes the session without writing, closing stdin as EOF.
- `src-tauri/src/agents/types.rs` — `AgentAuthEvent` is `{ Output { line } } | { Exit { code } }`; there is no "URL detected" or "awaiting code" event variant, so that classification happens entirely client-side today.
- `src/lib/components/ui/` — has `dialog/` (modal) and `tabs/` (mutually-exclusive panels) but no `Accordion` or `Collapsible` primitive; an expand/collapse inline card has no existing component to reuse.
- `src/AGENTS.md` — cross-route/shared state must live in a module-scoped `.svelte.ts` store above the router, and "Components must not call `listen()` for a stream a store already owns"; `src/routes/agent-auth/+page.svelte` currently violates this by calling `listen()` directly, which this feature should not repeat in its replacement.
- `messages/en.json` — only 6 `credentials_*` i18n keys exist (`credentials_title`, `credentials_intro`, `credentials_status_connected`, `credentials_status_not_connected`, `credentials_log_in`, `credentials_log_out`, `credentials_auth_window_title`); every string in `agent-auth/+page.svelte` ("Open in browser", "Paste authorization code here…", "Submit", success/failure text) is hardcoded English, not routed through Paraglide.

## Proposed Model

### Inline Expandable Row (`src/routes/credentials/+page.svelte`)

Each service row becomes a card that can be in a collapsed state (current appearance: label, status, Log in/Log out button) or an expanded state showing login progress inline, replacing the `WebviewWindow` call in `logIn()`. Clicking "Log in" starts the same `ensure_auth_container` → `install_agent` → `start_agent_auth` sequence, then expands the row in place instead of opening a window. The expand/collapse mechanism hangs off a new shadcn-svelte `Collapsible`/`Accordion` primitive added under `src/lib/components/ui/` (none exists today), per the root `AGENTS.md` rule to reuse `ui/` primitives and ask before adding a new shadcn component.

### Auth Session Store (new `.svelte.ts` module, e.g. `src/lib/agents/authSession.svelte.ts`)

A module-scoped store owns the `listen("agent-auth-output-<sessionId>", ...)` subscription for the in-progress session, per the `src/AGENTS.md` rule that components must not own stream listeners a store should own. The store holds, per session: the raw line buffer (for a collapsed "show raw output" toggle, see Open Questions), the detected OAuth URL (once found), whether the CLI is awaiting a pasted code, and terminal state (success/failure/exit code). The credentials page component reads accessors from this store instead of calling `listen()` itself — the same responsibility `agent-auth/+page.svelte` currently holds directly and which this feature does not want to duplicate per-row.

### URL and Prompt Detection (ported from `agent-auth/+page.svelte`)

The existing regex-based detection (URL pattern match, `"paste code"` substring match) moves into the new auth-session store unchanged in logic, just relocated from the standalone window's component into the shared store so the credentials page can react to the same signals. `src-tauri/src/agents/types.rs`'s `AgentAuthEvent` is not changed by this feature — detection stays client-side (see tradeoffs).

### Inline Code Submission

The expanded card shows a text input and submit button once the store reports "awaiting code," calling the existing `send_agent_auth_input` Tauri command with the same `sessionId` — no backend change needed, since stdin delivery is already keyed by session id and is agnostic to which window or component invoked it.

### Retirement of the Auth Window (`src/routes/agent-auth/+page.svelte`, `credentials_auth_window_title` key)

The `WebviewWindow` call in `logIn()` and the `/agent-auth` route are removed once the inline card covers the same flow; the `credentials_auth_window_title` i18n key becomes unused and is deleted along with it (see Open Questions on whether any fallback path still needs a window).

## Technology Choices with Tradeoffs

### Expand/collapse UI: new shadcn `Collapsible`/`Accordion` primitive (chosen)

Chosen: add a shadcn-svelte `Collapsible` (or `Accordion` if multiple rows should support independent expand state, which they should) component under `src/lib/components/ui/`.

Alternatives evaluated:

- **Reuse `Dialog`** — rejected because a modal dialog reintroduces the same problem the feature is fixing: the login UI would again cover the rest of the credentials list and feel like a "separate window" in spirit even though it is not a real OS window.
- **Hand-rolled `{#if expanded}` block with no dedicated component** — rejected because the root `AGENTS.md` convention is to reuse or add `ui/` primitives rather than one-off conditional markup, and a hand-rolled toggle would need to reinvent animation/ARIA state that a shadcn `Collapsible` already provides.

### Stream ownership: dedicated `.svelte.ts` store (chosen) vs `listen()` in the page component

Chosen: a module-scoped store owns the `agent-auth-output-<sessionId>` subscription, matching the pattern documented in `src/AGENTS.md`.

Alternative evaluated: **keep `listen()` inline in `credentials/+page.svelte`**, mirroring exactly what `agent-auth/+page.svelte` does today — rejected because it directly repeats the pattern `src/AGENTS.md` flags as wrong ("Components must not call `listen()` for a stream a store already owns"), and because the credentials page will need to track a session per row if the catalogue grows beyond one service, which a store models more naturally than page-local `$state`.

### URL/prompt detection location: keep client-side regex (chosen) vs move to Rust

Chosen: port the existing TypeScript regex/substring detection from `agent-auth/+page.svelte` into the new store, unchanged.

Alternative evaluated: **add a structured `AgentAuthEvent::UrlDetected`/`AgentAuthEvent::AwaitingCode` variant emitted from `src-tauri/src/agents/mod.rs`** — rejected for this feature because it would require the Rust side to parse CLI-specific output text (the same regex, just relocated), coupling `stream_output`'s generic byte-streaming logic to one agent's prompt wording; today's regex approach already works and is far cheaper to iterate on in TypeScript than in Rust, and no other agent's login flow currently needs this in `src-tauri/src/agents/mod.rs`.

### Fate of the `/agent-auth` window: remove entirely (chosen) vs keep as fallback

Chosen: delete `src/routes/agent-auth/+page.svelte`, the `WebviewWindow` call in `logIn()`, and the `credentials_auth_window_title` i18n key once the inline card is verified to reproduce the flow.

Alternative evaluated: **keep the window as an optional "pop out" or debug view** — rejected because the user's request is explicitly to stop showing a separate window, and keeping a second code path for the same login flow doubles the surface that must stay in sync with `src-tauri/src/agents/mod.rs` event shapes for no requested benefit.

## Validation

No spike has been run for this feature yet. The subprocess streaming, session-keyed stdin delivery, and URL-detection-via-regex mechanisms it depends on are already proven in production by the existing `agent-auth/+page.svelte` flow, so the main unproven surface is the shadcn `Collapsible`/`Accordion` addition and the store refactor, neither of which carries the container/CLI risk that would warrant a hands-on spike before `02_dev.md`.

## Open Questions

- **Raw output visibility**: should the expanded card ever show the raw CLI output lines (as `agent-auth/+page.svelte`'s `<pre>` block does today), e.g. behind a "show details" toggle for troubleshooting a stuck login, or should the card only ever show the derived URL/code-input/status and hide raw terminal text entirely? This determines whether the auth-session store needs to retain the full line buffer or can discard lines after scanning them.
- **Auto-expand timing**: does the card expand immediately on clicking "Log in" (showing a loading/spinner state while `ensure_auth_container`/`install_agent`/`start_agent_auth` run, before any URL exists), or only once the URL is detected? Affects what placeholder state the expanded card needs before the first `agent-auth-output-<sessionId>` event arrives.
- **Cancel affordance**: should the expanded card expose a "Cancel" action wired to the existing `cancel_agent_auth` command, and if the user collapses the card manually mid-login, does that collapse also cancel the session or leave it running in the background with the row showing an "in progress" collapsed state?
- **Concurrent logins**: the catalogue currently has one `credentialService` (`anthropic`), but if a second service is added later, can two rows be expanded and logging in simultaneously? This affects whether the auth-session store is keyed by a single "current session" or by a map of `serviceId -> session`.
- **Fallback window removal timing**: should `src/routes/agent-auth/+page.svelte` be deleted in the same change as the inline card ships, or kept one release cycle as a manual fallback in case the inline flow has an edge case (e.g. very long URLs, terminal-only prompts) not caught before release?

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Inline card never shows a URL even though login is progressing | `stream_output` in `src-tauri/src/agents/mod.rs` flushes on an 80ms idle timeout, not on newlines, because the CLI's paste prompt has no trailing newline — a naive line-buffered reader on the frontend could still miss partial lines | Reuse the existing per-event line handling from `agent-auth/+page.svelte` verbatim in the new store rather than re-parsing the raw byte stream |
| "Log in" button appears to do nothing for several seconds before the card expands | `ensure_auth_container` and `install_agent` (installing the CLI into a fresh auth container) run before `start_agent_auth` even starts the login process, so there is no `sessionId` yet to expand around | Show a loading state in the row as soon as `logIn()` starts, before a `sessionId` exists (see Open Questions: auto-expand timing) |
| Pasted code submission silently fails on a second attempt | `send_auth_input` in `src-tauri/src/agents/mod.rs` removes the session from `AuthSessionMap` after the first stdin write (one-shot design for PKCE) | Do not allow the inline card to show the code-input box again after one submission for the same session; treat a second code as requiring a fresh `start_agent_auth` call |
| Collapsing the card leaves the container/process running with no way to see it finished | If collapse does not cancel the session (see Open Questions), the auth-session store must keep listening even while the card is visually collapsed, or the `Exit` event is missed | Keep the store's `listen()` alive for the session regardless of the card's collapsed/expanded UI state; only tear it down on `Exit` or explicit cancel |
| New collapsible UI text is hardcoded English and diverges from the rest of the app | The `agent-auth` window's strings were never routed through Paraglide (`messages/en.json` only has the 6 `credentials_*` keys) | Add new keys to `messages/en.json` first, then mirror to `messages/de.json` and `messages/ru.json`, per root `AGENTS.md` |
| Deleting `/agent-auth` breaks a lingering reference | `credentials_auth_window_title` in `messages/en.json`/`de.json`/`ru.json` and the `WebviewWindow` import in `credentials/+page.svelte` are the only current references to the window route | Remove the i18n key from all three locale files and the `WebviewWindow` call together with the route file in the same change |
