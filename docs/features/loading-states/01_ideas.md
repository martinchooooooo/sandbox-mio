---
type: ideas
title: "Loading States for User Actions"
feature: loading-states
---
# Loading States for User Actions

This document is the ideation for adding in-flight feedback (loading states) to every user-triggered action in the Projects UI, and for establishing a reusable pattern so future features get loading feedback for free. The Projects UI is a SvelteKit + Svelte 5 (runes) frontend that drives a Tauri/Rust backend over `invoke()`. Today exactly one action has any feedback — package install/uninstall, via a single `packageBusy` boolean in `src/routes/projects/SandboxItem.svelte` — and every other action (`start`, `stop`, `deploy`, `undeploy`, `destroy`, `toggleSync`, `openTerminal`, and `createProject` in `src/routes/projects/+page.svelte`) fires its `invoke()` and awaits silently, leaving the button visually unchanged for the entire round trip. Nothing else in this feature exists yet. The feature builds on three existing seams: the async action handlers in `src/routes/projects/SandboxItem.svelte` and `src/routes/projects/+page.svelte`, the shared `Button` component in `src/lib/components/ui/button/button.svelte`, and the Hugeicons icon dependency (`@hugeicons/svelte` + `@hugeicons/core-free-icons`) already in `package.json`.

## Current State

### The one existing loading pattern

`src/routes/projects/SandboxItem.svelte` declares `let packageBusy = $state(false)` (line 36). `installPackage()` and `uninstallPackage()` set it `true` at entry and `false` in a `finally` block (lines 61–88). The flag drives `disabled={packageBusy}` on the package `<input>`, the install `Button`, and each uninstall `✕` button, plus `disabled:opacity-50` styling. There is no spinner — the only signal is that the controls grey out and stop responding. This is the de-facto pattern the feature generalises.

### Actions with no feedback

Every other handler follows the shape `try { onError(""); await invoke(...); ... } catch (e) { onError(parseError(e).message); }` with no busy flag:

- `src/routes/projects/SandboxItem.svelte`: `deploy()` (95), `undeploy()` (107), `destroyProject()` (117, which also runs a `confirm()` dialog and a second force `invoke` on the `UnsyncedChanges` error kind), `toggleSync()` (140, optimistic — flips `syncOn` before awaiting and reverts on error), `start()` (152), `stop()` (160), `openTerminal()` (172, also opens a `WebviewWindow`).
- `src/routes/projects/+page.svelte`: `createProject()` (48).

### Markup inconsistency the feature must reconcile

The action controls are split across two element types. The package install, deploy-confirm, cancel, and create-project controls use the shared `Button` component (`src/lib/components/ui/button/button.svelte`). The header-row actions (`folder`, `sync` switch, `terminal`, `undeploy`, `stop`, `start`, `deploy`, `destroy`) and the uninstall `✕` are raw `<button>` elements with no variant, no disabled styling, and no slot for an icon (`src/routes/projects/SandboxItem.svelte` lines 225–242, 290–294). A uniform loading affordance requires these raw buttons to gain the same capability as `Button`, either by migrating them to `Button` or by teaching both to render a spinner.

### Button component capabilities

`src/lib/components/ui/button/button.svelte` already forwards a `disabled` prop and bakes in `disabled:pointer-events-none disabled:opacity-50` plus SVG sizing rules (`[&_svg:not([class*='size-'])]:size-4`), so an inline spinner icon will be sized correctly with no extra classes. It has no `loading` concept today; adding one is a natural extension of the existing prop surface (`ButtonProps`, line 37).

### Error and i18n conventions that constrain the design

All `invoke()` rejections are routed through `parseError(e).message` (`src/lib/errors.ts`) into an `onError` callback that the parent renders; loading states must not change this contract. User-visible strings must be i18n keys in `messages/en.json` mirrored to every locale (`messages/de.json`, `messages/ru.json`) and called as `m.<key>()` per `AGENTS.md`. A spinner is iconographic and needs no string, but any accessible label (e.g. an `aria-label` / `aria-busy` announcement) would.

## Proposed Model

### A `loading` prop on the shared Button

The seam is `ButtonProps` in `src/lib/components/ui/button/button.svelte`. A `loading?: boolean` prop, when true, renders a spinning Hugeicons icon (inline-start) and sets `disabled` and `aria-busy="true"`. Because the component already centres children with `inline-flex items-center justify-center gap-*` and sizes SVGs, the spinner sits beside or replaces the label without layout shift. Every control that is a `Button` then gets loading feedback by passing one prop, which is the foundation of the reusable pattern.

### A per-action busy discriminant instead of many booleans

The seam is the `<script>` state block of each component. Rather than one boolean per action (`startBusy`, `stopBusy`, `deployBusy`, …), the component tracks a single `let busy = $state<string | null>(null)` holding the *name* of the action currently in flight (e.g. `"deploy"`). A button shows `loading={busy === "deploy"}` and is `disabled={busy !== null}` so the pressed button spins while siblings grey out, preventing a second concurrent mutation on the same sandbox. `SandboxItem` is instantiated once per sandbox, so the discriminant is naturally scoped to a single row — clicking `start` on one sandbox does not disable another sandbox's buttons.

### A `runAction` helper that encodes the whole pattern

The seam is a new tiny frontend utility (proposed `src/lib/actions.ts` or co-located helper). It wraps the repeated `onError("") → set busy → await → parseError → finally clear busy` choreography so each handler shrinks to one call. Conceptually:

```
await runAction("deploy", () => invoke("deploy_sandbox", { name, host }), { onError, setBusy });
```

The helper sets the busy discriminant to the action name, clears the prior error, awaits the supplied thunk, routes any rejection through `parseError().message` to `onError`, and clears the busy flag in `finally`. This is the artefact future features copy: a new action gets correct loading + error behaviour by calling `runAction` rather than re-deriving the try/finally. The existing `packageBusy` logic collapses into this helper.

### Raw header buttons migrate to Button

The seam is `src/routes/projects/SandboxItem.svelte` lines 225–242. The raw `<button>` header actions become `Button` instances (likely `size="sm"` / `size="xs"`, `variant="ghost"` or `"outline"` to preserve the current lightweight look) so they can accept the `loading` prop. The `folder` button calls `revealItemInDir` synchronously-ish and may not need loading; the destructive `destroy` keeps its `variant="destructive"` styling that already exists in `buttonVariants`.

### Optimistic actions keep their revert, gain a spinner

The seam is `toggleSync()` (line 140). The sync `Switch` already flips optimistically and reverts on error; the loading state for it is additive — the switch (or an adjacent indicator) shows in-flight while the `enable_sync`/`disable_sync` round trip is pending, without removing the optimistic flip. This is called out separately because `Switch` is not a `Button` and the `loading` prop does not apply to it directly.

## Technology Choices with Tradeoffs

### Spinner source: Hugeicons + Tailwind `animate-spin`

Chosen because `@hugeicons/svelte` and `@hugeicons/core-free-icons` are already dependencies (`package.json`), so a loader/spinner glyph animated with Tailwind's built-in `animate-spin` utility (Tailwind v4 is present) adds no new dependency and inherits the Button's SVG sizing automatically.

Alternatives evaluated:

- **`tw-animate-css` keyframes** — already a dependency, but it is oriented at richer enter/exit animations; a continuous rotation is exactly what the stock `animate-spin` utility provides, so the extra library buys nothing here.
- **GSAP-driven spinner** — `gsap` is a dependency (used elsewhere for richer motion), but driving a perpetual rotation through GSAP means a JS animation loop per spinner and manual teardown on unmount; CSS `animate-spin` is GPU-composited and disappears with the element. Rejected as overkill for a utility spinner.
- **Hand-rolled inline `<svg>` spinner** — rejected because it reintroduces custom markup that `AGENTS.md` explicitly steers away from when an existing component/dependency covers the need, and it would not share Hugeicons' sizing conventions.

### State model: single busy discriminant per component

Chosen because one `string | null` expresses "which action is running" and lets the UI both spin the pressed button and disable its siblings with no extra state, while staying scoped per `SandboxItem` instance.

Alternatives evaluated:

- **One boolean per action** — rejected: six-plus `$state` booleans per row is verbose, easy to leave un-reset on a new code path, and cannot by itself express "disable the others while one runs" without combining them anyway.
- **A global busy store (Svelte store / context)** — rejected for the per-row case because a global flag would disable buttons across *all* sandboxes when one action runs; the discriminant must be row-scoped. A global store is only relevant for genuinely app-wide actions, which this feature does not have.
- **A `Set`/`Map` of in-flight action names** — rejected as premature: the UI intentionally allows only one mutation per sandbox row at a time, so a single discriminant is sufficient and simpler than a collection.

### Reuse mechanism: a `runAction` helper function

Chosen because the loading + error choreography is identical across nine handlers, and a plain async helper (no framework magic) is the smallest thing that removes the duplication and serves as the copyable pattern for future features.

Alternatives evaluated:

- **A Svelte 5 rune factory (`createAction()` returning `{ busy, run }`)** — a viable richer alternative that bundles the `$state` busy flag with the runner. Stronger ergonomics, but it couples the helper to Svelte's rune runtime and is harder to call from a parent that already owns the busy discriminant. Left as an open question rather than rejected outright.
- **Inline try/finally in every handler** — the status quo extended; rejected because it guarantees the same drift that left eight handlers with no feedback in the first place.
- **A wrapper component (`<AsyncButton onclick={…}>`)** that owns its own busy state — rejected as the primary mechanism because it hides the in-flight state from the parent, which sometimes needs to disable *sibling* controls; the `loading` prop + external discriminant keeps the parent in control. It remains a possible convenience layer on top.

### Affordance during in-flight: spin pressed + disable siblings

Chosen because it gives unambiguous feedback (the pressed control animates) and prevents conflicting concurrent mutations on one sandbox (e.g. `deploy` while `destroy` is mid-flight).

Alternatives evaluated:

- **Spin pressed button only, leave siblings active** — rejected because it permits a second mutation on the same sandbox mid-flight, which the backend may not order safely.
- **Overlay/skeleton over the whole row** — rejected as too heavy for sub-second actions and it obscures the status dot and name the user is watching; reserved only for actions known to be long (a possible future refinement, see Open Questions).

## Validation

No spike has been run. The approach is a generalisation of the already-working `packageBusy` pattern, so the core risk is low. The following should be confirmed before or during the build:

1. **A Hugeicons glyph animates cleanly under `animate-spin` inside `Button`** without size or baseline jitter, at `size="sm"`/`"xs"`.
2. **Migrating the raw header `<button>`s to `Button`** preserves the current compact header layout (spacing, the inline status text, the `→ host` label) and the e2e selectors that target those buttons by text still resolve.
3. **The `runAction` helper composes with `destroyProject`'s two-step flow** (the `confirm()` dialog and the conditional force-destroy retry) without the spinner spinning during the blocking `confirm()`.

A spike is offered: stand up the `loading` prop on `Button` plus `start`/`stop` wired through `runAction` on one sandbox row, then confirm the spinner shows for the full round trip and siblings disable. This would clear risks 1–3 cheaply. The user should be consulted on whether to spike or proceed straight to `02_dev.md`.

## Open Questions

- **Helper shape: plain `runAction` function vs `createAction` rune factory.** Whether the reusable artefact owns the busy state (rune factory) or operates on a busy discriminant the component already owns (plain function). `02_dev.md` should pick one and state where the helper file lives.
- **Whether the `folder` button needs loading at all.** `openWorkspace()` calls `revealItemInDir`; if it resolves near-instantly it may be exempt. The dev plan should decide the threshold for which actions get a spinner.
- **Loading affordance for the `Switch` (sync toggle).** `Switch` is not a `Button`; whether to show an adjacent spinner, a disabled state, or rely solely on the existing optimistic flip is undecided.
- **`destroyProject` and the blocking `confirm()`.** Whether the busy state should begin before or after the native `confirm()` dialog, and whether to replace `confirm()` with a non-blocking dialog so the spinner is meaningful, is undecided.
- **Minimum spinner display time.** Whether to enforce a small floor (e.g. ~150 ms) so very fast actions do not produce a one-frame flicker, or to accept the flicker, is undecided.
- **Long-action affordance.** Whether `deploy` (which can take many seconds) warrants more than a button spinner — a progress indicator or row-level overlay — is deferred but should be named in the dev plan.
- **Accessibility scope.** Whether `aria-busy` on the button is sufficient or whether an `aria-live` region should announce action start/finish for screen readers.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Spinner never disappears after an error | Busy flag set true but cleared only on the success path | Always clear the busy discriminant in a `finally` block — the `runAction` helper centralises this |
| All sandboxes' buttons disable when one action runs | Busy state lifted to a global/parent store instead of per-row | Keep the discriminant inside the per-instance `SandboxItem`, not in `+page.svelte` |
| Raw header buttons cannot show a spinner | They are bare `<button>` elements with no icon slot or disabled styling | Migrate them to the shared `Button` component (`src/lib/components/ui/button/button.svelte`) |
| Spinner spins during the `confirm()` dialog in destroy | Busy set before the blocking native dialog returns | Set busy only around the actual `invoke`, after `confirm()` resolves |
| New locale build fails or shows raw key | A user-visible loading/accessibility string added to `en.json` but not mirrored to `de.json`/`ru.json` | Mirror every new key to all locale files per `AGENTS.md`; prefer iconographic feedback that needs no string |
| Optimistic sync toggle double-reverts | Loading state added to `toggleSync` interferes with the existing optimistic flip/revert | Keep the spinner additive; do not move the `syncOn` flip into the busy logic |
| Layout shifts when label is replaced by spinner | Spinner and label have different widths | Render the spinner inline-start beside the label, or reserve width, using the Button's existing `gap`/SVG sizing |
| Second click queues a duplicate mutation | Sibling buttons not disabled while one action is in flight | Disable all row actions with `disabled={busy !== null}` while any action runs |
</content>
</invoke>
