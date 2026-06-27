# Loading States for User Actions — Build Plan

This document is the build plan for the loading-states feature whose design is in `docs/features/loading-states/01_ideas.md`. The feature gives every user-triggered action in the Projects UI in-flight feedback and a per-project **status row** that surfaces both a pending message ("Deploying…") and an error message, replacing the current page-level `error` string for row-scoped actions. The feature is **frontend-only** — no Rust, no Tauri command, and no `messages` key removal; only additions. It supersedes one assumption from `01_ideas.md`: the ideas doc said loading must not change the `onError` contract, but this plan **removes the `onError` prop from `SandboxItem`** and routes each row's errors into that row's own status row instead. The work is organised into five phases (A–E); **all phases are Remaining — nothing is implemented yet.** The seams are `src/lib/components/ui/button/button.svelte` (the `loading` prop), a new `src/lib/actions.svelte.ts` (the action runner), a new `src/routes/projects/StatusRow.svelte` (the reusable status row), `src/routes/projects/SandboxItem.svelte` (per-row actions), and `src/routes/projects/+page.svelte` (the create-project action and list-load errors).

## The Concrete Model

### One action runner owns busy + message + error

A Svelte 5 rune factory `createActionRunner()` lives in `src/lib/actions.svelte.ts` (the `.svelte.ts` extension is required for `$state` outside a component). It returns an object with three reactive getters and one method:

- `busy` — `string | null`, the key of the action currently in flight (e.g. `"deploy"`), or `null` when idle.
- `message` — `string`, the human-readable pending message for the in-flight action (e.g. "Deploying to remote-1…"), or `""` when idle.
- `error` — `string`, the last error message from a failed action, or `""` when none.
- `run(key, thunk, opts?)` — the choreography: set `error = ""`, set `busy = key`, set `message = opts?.message ?? ""`, `await thunk()`, on rejection set `error = parseError(e).message`, and in a `finally` clear `busy = null` and `message = ""`. Returns a boolean (`true` on success) so callers can branch (e.g. clear an input only on success).

`parseError` is imported from `src/lib/errors.ts`, keeping the existing error-parsing contract. Each `SandboxItem` instance constructs its own runner, so `busy`/`message`/`error` are naturally scoped to one sandbox row. `+page.svelte` constructs a separate runner for the page-level create action.

### A reusable StatusRow component

`src/routes/projects/StatusRow.svelte` is the reusable artefact future features copy. It takes props `{ busy, message, error }` and renders exactly one line:

- When `busy !== null` and `message` is non-empty: a spinning Hugeicons glyph (`animate-spin`) followed by `message`, in muted text.
- When `error` is non-empty (and not busy): the error text in `text-destructive`.
- Otherwise: nothing (the element collapses).

The root element carries `role="status"` and `aria-live="polite"` so screen readers announce pending and error transitions. The component holds no state; it is a pure view over the runner's three values.

### The `loading` prop on Button

`src/lib/components/ui/button/button.svelte` gains `loading?: boolean` on `ButtonProps`. When `loading` is true the component renders the same spinning Hugeicons glyph inline-start of the children, sets the native `disabled` attribute, and sets `aria-busy="true"`. The existing `disabled:opacity-50` / `disabled:pointer-events-none` base classes and SVG sizing rules already style the disabled state and size the spinner, so no new classes are needed. Buttons pass `loading={runner.busy === "deploy"}` and `disabled={runner.busy !== null}` so the pressed button spins while its siblings grey out.

### Row markup unification

The raw header `<button>` elements in `SandboxItem.svelte` (`folder`, `terminal`, `undeploy`, `stop`, `start`, plus the package uninstall `✕`) migrate to the shared `Button` component so they can take `loading`. They use `variant="ghost"` `size="sm"` to preserve the current lightweight look; `destroy` uses the existing `variant="destructive"`. The deploy-confirm, cancel, install, and create controls are already `Button`s and only gain the `loading`/`disabled` props.

## Resolved Decisions

Each Open Question from `01_ideas.md` is resolved here:

- **Helper shape (plain function vs rune factory).** Resolved to a **rune factory** `createActionRunner()` in `src/lib/actions.svelte.ts`. The status row needs co-located reactive `busy`/`message`/`error`, which a factory owns cleanly; a plain function would force every component to declare and thread three `$state` fields by hand.
- **Whether the `folder` button needs loading.** Resolved: **yes**, wrapped through the runner for uniformity. `revealItemInDir` is fast but async; routing it through `run("folder", …)` costs nothing and keeps every action consistent. Its pending message is omitted (empty) because it resolves near-instantly; only its errors surface in the status row.
- **Loading affordance for the sync `Switch`.** Resolved: the `Switch` gets `disabled={runner.busy !== null}` and the status row shows the pending message ("Enabling sync…" / "Disabling sync…"). The existing optimistic flip/revert in `toggleSync` is kept unchanged; the status row is additive.
- **`destroyProject` and the blocking `confirm()`.** Resolved: the runner is invoked **after** `confirm()` returns, so the spinner never spins behind the native dialog. The two-step flow (initial destroy, then force on `UnsyncedChanges`) runs inside a single `run("destroy", …)` thunk that performs the `confirm()` and the second `invoke` internally.
- **Minimum spinner display time.** Resolved: **no floor.** Sub-second flicker is accepted to keep the runner simple; a delay floor can be added later inside `run()` without changing any caller.
- **Long-action affordance (deploy).** Resolved: the **status row message is the affordance** ("Deploying to <host>…"). No progress bar or row overlay this iteration.
- **Accessibility scope.** Resolved: `aria-busy` on each loading Button plus `role="status"` / `aria-live="polite"` on the StatusRow. No separate `aria-live` region is added.

One assumption from `01_ideas.md` is **superseded**: errors are no longer bubbled from `SandboxItem` to the page via `onError`. Each row renders its own errors in its own StatusRow. The page keeps a status row only for the create action and for list/host/deployment load failures.

## Remaining: Phase A — `loading` prop on Button

Edit `src/lib/components/ui/button/button.svelte`.

- Add `loading?: boolean` to the `ButtonProps` type (the `&`-extended object at line 37).
- Destructure `loading = false` in the `$props()` block (line 45) and remove it from `restProps` so it is not spread onto the DOM element.
- Compute the effective disabled state as `disabled || loading` and pass it to both the `<a>` and `<button>` branches' `disabled` / `aria-disabled`.
- Add `aria-busy={loading}` to both branches.
- Before `{@render children?.()}` in both branches, render the spinner when `loading`:

```svelte
{#if loading}
    <HugeiconsIcon icon={Loading03Icon} class="animate-spin" />
{/if}
{@render children?.()}
```

Import `HugeiconsIcon` from `@hugeicons/svelte` and `Loading03Icon` from `@hugeicons/core-free-icons` in the second `<script>` block, matching the existing usage in `src/routes/+layout.svelte:5-6`. (Confirm `Loading03Icon` exists in `@hugeicons/core-free-icons@4.2.0`; substitute another loader glyph from that package if not — this is the only icon-name risk and is checked in the spike.)

## Remaining: Phase B — Action Runner

Create `src/lib/actions.svelte.ts`.

```ts
import { parseError } from "$lib/errors";

export interface ActionRunner {
    readonly busy: string | null;
    readonly message: string;
    readonly error: string;
    run(key: string, thunk: () => Promise<unknown>, opts?: { message?: string }): Promise<boolean>;
}

export function createActionRunner(): ActionRunner {
    let busy = $state<string | null>(null);
    let message = $state("");
    let error = $state("");

    return {
        get busy() { return busy; },
        get message() { return message; },
        get error() { return error; },
        async run(key, thunk, opts) {
            error = "";
            busy = key;
            message = opts?.message ?? "";
            try {
                await thunk();
                return true;
            } catch (e) {
                error = parseError(e).message;
                return false;
            } finally {
                busy = null;
                message = "";
            }
        },
    };
}
```

No test harness or `invoke` import lives here — the runner is transport-agnostic; callers pass a thunk that wraps their own `invoke`. This keeps the runner reusable for any async action, including non-Tauri ones.

## Remaining: Phase C — StatusRow Component

Create `src/routes/projects/StatusRow.svelte`.

```svelte
<script lang="ts">
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { Loading03Icon } from "@hugeicons/core-free-icons";

    let { busy, message, error }: {
        busy: string | null;
        message: string;
        error: string;
    } = $props();
</script>

{#if busy !== null && message}
    <div role="status" aria-live="polite" class="flex items-center gap-1.5 text-xs text-muted-foreground">
        <HugeiconsIcon icon={Loading03Icon} class="animate-spin size-3.5" />
        <span>{message}</span>
    </div>
{:else if error}
    <div role="status" aria-live="polite" class="text-xs text-destructive">{error}</div>
{/if}
```

The component renders nothing when idle and no error is set, so it adds no vertical space in the common case. It is reused by both `SandboxItem` and `+page.svelte`.

## Remaining: Phase D — Wire SandboxItem

Edit `src/routes/projects/SandboxItem.svelte`. This is the largest phase.

### Props and state

- **Remove** the `onError` prop from the `$props()` destructure (lines 28–31 region) and from every call site. The parent no longer receives row errors.
- **Remove** `let packageBusy = $state(false)` (line 36).
- Add `import { createActionRunner } from "$lib/actions.svelte";` and `import StatusRow from "./StatusRow.svelte";`.
- Add `const action = createActionRunner();` near the top of the script.

### Rewrite each handler through the runner

Every handler collapses to a `action.run(key, thunk, { message })` call. Examples:

- `start`: `action.run("start", () => invoke("start_sandbox", { name: project.name }), { message: m.projects_status_starting() })`.
- `stop`: key `"stop"`, message `m.projects_status_stopping()`.
- `deploy`: key `"deploy"`, message `m.projects_status_deploying({ host: chosenHost })`; on success close the picker and call `onChanged()` — branch on the boolean return: `if (await action.run(...)) { picking = false; onChanged(); }`.
- `undeploy`: key `"undeploy"`, message `m.projects_status_undeploying()`; `onChanged()` on success.
- `installPackage`: key `"install"`, message `m.projects_status_installing({ name: pkg })`; clear `packageInput` and `loadPackages()` on success. The `<input>` and install/uninstall buttons use `disabled={action.busy !== null}` (replacing `packageBusy`).
- `uninstallPackage`: key `"uninstall"`; `loadPackages()` on success.
- `openTerminal`: key `"terminal"`, message `m.projects_status_opening_terminal()`; the thunk performs the `invoke` and the `WebviewWindow` open; `terminalToken` is set inside the thunk on success.
- `openWorkspace`: key `"folder"`, no message; thunk calls `revealItemInDir`.
- `toggleSync`: keep the optimistic `syncOn` flip and the revert-on-error. Wrap the `invoke` in `action.run("sync", …, { message: on ? m.projects_status_enabling_sync() : m.projects_status_disabling_sync() })`; on a `false` return (error) revert `syncOn = !on`; on success `onChanged()`.
- `destroyProject`: invoke the runner **after** the `confirm()` decision. The thunk contains the full two-step logic — first `invoke("destroy_sandbox", { force: false })`, and inside its catch, if `parseError(e).kind === "UnsyncedChanges"` run `confirm()` and conditionally `invoke(..., { force: true })`; rethrow other errors so the runner records them. Because `confirm()` must not block the spinner, perform the `confirm()` for the non-`UnsyncedChanges` path before calling `action.run`; the `UnsyncedChanges` confirm happens inside the thunk only after the first call has already failed, which is acceptable since the spinner is already meaningful at that point. `onChanged()` on success.

`loadPackages` in `onMount` routes its error through `action.error` directly (set `action`'s error via a tiny `run("load", …)` with no message, or assign through a dedicated path) rather than the removed `onError`.

### Markup

- Migrate the raw header `<button>`s (lines 225, 231–242) and the uninstall `✕` (lines 290–294) to `Button` with `variant="ghost"` (or `"destructive"` for destroy) `size="sm"`, each with `loading={action.busy === "<key>"}` and `disabled={action.busy !== null}`.
- Add `disabled={action.busy !== null}` to the sync `Switch` (line 228).
- Add `loading`/`disabled` to the existing deploy-confirm and install `Button`s.
- Insert `<StatusRow busy={action.busy} message={action.message} error={action.error} />` as a row inside the `<li>`, below the header `<div>` (after line 244) so each sandbox shows its own pending/error line. The existing terminal-token banner and package section remain.

### i18n keys

Add these keys to `messages/en.json`, then mirror them to `messages/de.json` and `messages/ru.json` (per `AGENTS.md`):

- `projects_status_starting`, `projects_status_stopping`, `projects_status_undeploying`, `projects_status_opening_terminal`, `projects_status_enabling_sync`, `projects_status_disabling_sync` — plain strings.
- `projects_status_deploying` — parameterised `"Deploying to {host}…"`.
- `projects_status_installing` — parameterised `"Installing {name}…"`.
- `projects_status_uninstalling` — parameterised `"Uninstalling {name}…"`.
- `projects_status_creating` — used by Phase E.

## Remaining: Phase E — Wire +page.svelte

Edit `src/routes/projects/+page.svelte`.

- Add `import { createActionRunner } from "$lib/actions.svelte";` and `import StatusRow from "./StatusRow.svelte";` and `const action = createActionRunner();`.
- Rewrite `createProject` (line 48) as `if (await action.run("create", () => invoke("create_sandbox", { name }), { message: m.projects_status_creating({ name }) })) { projectName = ""; await loadProjects(); }`.
- Route `loadProjects`/`loadHosts`/`loadDeployments` failures into `action.error` (replacing the standalone `error` string), or keep the existing `error` string solely for these load failures — **decision: keep the `let error` string for load failures** (they are not button-triggered) and add a separate `<StatusRow>` for the create action. This avoids a load error being cleared by an unrelated create click.
- Add the create `Button` (line 97) `loading={action.busy === "create"}` and `disabled={action.busy !== null}`.
- Render `<StatusRow busy={action.busy} message={action.message} error={action.error} />` near the create field (replacing or alongside the existing `{#if error}` block at line 103).
- **Remove** the `onError={(msg) => (error = msg)}` prop from the `<SandboxItem>` instantiation (line 114), since `SandboxItem` no longer accepts `onError`.

## Lifecycle and Cross-Cutting Concerns

- **Runner scoping.** Each `SandboxItem` owns its runner, so concurrent actions on different sandboxes are independent — starting sandbox A never disables sandbox B's buttons. The page-level runner is separate again, so a create-in-flight does not disable any row.
- **Single mutation per row.** While `action.busy !== null`, all of a row's buttons are disabled, preventing a second concurrent backend mutation on the same sandbox. The package `<input>` is likewise disabled.
- **Error/message clearing.** `run()` clears `error` at the start of every action, so a successful retry wipes a prior error; the `finally` always clears `busy` and `message`, so a thrown error can never leave a permanent spinner. This is the single point that prevents the "spinner never disappears" gotcha from `01_ideas.md`.
- **Optimistic sync toggle.** The `syncOn` flip stays outside the runner's state; the runner only tracks the in-flight message and any error, and the caller reverts `syncOn` on a `false` return. The two mechanisms do not interfere.
- **No backend changes.** No Rust file, no `#[tauri::command]`, and no `AppError` variant is touched. The `parseError`/`AppError` contract (`src/lib/errors.ts`) is consumed unchanged inside `run()`.
- **i18n completeness.** Every new `projects_status_*` key must exist in all three locale files or the Paraglide build fails / shows a raw key; parameterised keys (`{host}`, `{name}`) must keep the placeholder in every locale.
- **Icon name risk.** `Loading03Icon` is the assumed loader glyph from `@hugeicons/core-free-icons`; if absent, any rotating loader glyph from that package works identically since it is only spun via `animate-spin`. This is the one item to confirm in the spike before Phase A lands.
