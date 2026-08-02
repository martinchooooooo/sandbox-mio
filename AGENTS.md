# Repo Map

Sandbox-mio is a Tauri 2 desktop app (Windows-first) that manages per-project "sandboxes": Podman containers with a bind-mounted workspace, a mise-managed runtime, embedded zellij web terminals, installable AI coding agents, remote deploys with Mutagen file sync, and local LLM inference.

| Area | Contents | Orientation doc |
|---|---|---|
| `src/` | SvelteKit + Svelte 5 frontend (static adapter, runs in the Tauri webview) | `src/AGENTS.md` |
| `src-tauri/src/` | Rust backend: Tauri commands + trait-injected subsystems, each with an in-memory mock | `src-tauri/AGENTS.md` |
| `e2e/` | WebdriverIO tests in three tiers (`light`/`standard`/`heavy`) | `e2e/AGENTS.md` |
| `docs/features/<name>/` | Per-feature docs pipeline: core `01_ideas.md` → `02_dev.md` → `03_e2e.md`, plus the optional follow-ons `04_improvements.md` and `05_dev_review.md` | `docs/features/README.md` |
| `messages/` | Paraglide locale files (`en.json`, `de.json`, `ru.json`) | Translations section below |

Read the orientation doc for an area before editing files in that area. `AGENTS.md` files in subdirectories override the root file for their subtree.

# Feature Workflow

Features flow through the project skills, each writing one document under `docs/features/<feature>/`: `feature-ideas` (01_ideas.md, scoping) → `feature-dev` (02_dev.md, build plan) → `feature-implement` (code, phase by phase, keeps 02_dev.md status current) → `feature-e2e` (03_e2e.md, regression plan). When asked to build a feature, check for an existing `docs/features/<feature>/` directory first and resume from the most advanced document present. `02_dev.md` files record resolved design decisions — consult the feature's `02_dev.md` before changing behaviour that document specifies.

Those three documents are the core pipeline. A feature may also carry two optional follow-ons, written only when there is something to record: `04_improvements.md` holds work deferred out of scope and known seam limitations discovered during implementation, and `05_dev_review.md` holds post-implementation code review findings together with their resolutions. Neither is a prerequisite for any pipeline step, and neither replaces `02_dev.md` as the record of settled design.

# Verification Commands

Run before declaring any change done:

- `pnpm check` (repo root) — svelte-check over the frontend. Required after any `src/` change.
- `cargo check` (in `src-tauri/`) — required after any Rust change. `cargo test` runs the unit tests (`src-tauri/src/workspace/mod.rs`). The mock control plane deliberately has none: its correctness is proven by light specs instead (`docs/features/scriptable-mock-backend/02_dev.md`, Resolved Decision 9).
- `./build e2e light` (repo root; `build.exe` on Windows) — seconds, no Podman needed (`MIO_MOCK_BACKEND=1`); the cheapest end-to-end signal.
- `./build e2e` (repo root) — the `standard` tier against a real Podman machine; minutes. See `e2e/AGENTS.md` for tier selection.

## Tier cadence

Lean on `light` for day-to-day feedback and reserve `standard`/`heavy` for a periodic cadence,
not per change — `standard` proves real behaviour and remains the source of regression truth;
`light` just gets there faster while iterating:

- `./build e2e light` — every change that touches `src/` or a mocked `src-tauri/` subsystem.
- `./build e2e` (`standard`) — at merge, or daily if merges are infrequent.
- `./build e2e heavy` — weekly, or before a release.

No CI pipeline enforces this today (none exists in the repository — see
`docs/features/scriptable-mock-backend/04_improvements.md`); it is agent/developer habit codified
here and in `e2e/AGENTS.md`'s decision rules.

A feature is typically developed in its own git worktree, driven almost entirely by
`./build e2e light` — many worktrees can run the `light` tier concurrently, each isolated by a
per-worktree run slug (`e2e/lib/run-identity.ts`; see `e2e/AGENTS.md`). When a feature's light
specs are green, run `./build e2e` (`standard`) once at the end — `standard`/`heavy` runs are safe
alongside other worktrees' `light` runs, but only one `standard`/`heavy` run is allowed host-wide at
a time (`docs/features/parallel-worktree-e2e/02_dev.md`).

# Environment Variables

All read via `std::env::var` at call time, never cached:

- `PODMAN_CONNECTION` — Podman system connection name; default `podman-machine-default` (`src-tauri/src/backends/mod.rs`).
- `SANDBOX_WORKSPACE_ROOT` — root for sandbox workspace directories; default `<Documents>/sandbox-mio` (`workspace_root()` in `src-tauri/src/backends/mod.rs`).
- `MIO_MOCK_BACKEND=1` — swaps every subsystem trait to its in-memory mock; used by the e2e `light` tier (`mock_backend_enabled()` in `src-tauri/src/backends/mod.rs`).
- `MIO_TEST_MODEL` — overrides the model used by inference e2e specs.
- `MIO_APP_DATA_DIR` — app-data and log root; default the compiled-in `app_data_dir()`, and for logs `app_log_dir()` (`src-tauri/src/paths.rs`, `init_logging()` in `src-tauri/src/lib.rs`). Set by the e2e harness to a per-worktree directory, never by a shipped install.
- `MIO_SETTINGS_DIR` — relocates `settings.json`; takes precedence over `MIO_APP_DATA_DIR` (`settings_path()` in `src-tauri/src/settings/mod.rs`).
- `MIO_CREDENTIALS_DIR` — relocates the credential bundle store; takes precedence over `MIO_APP_DATA_DIR` (`store_dir()` in `src-tauri/src/credentials/mod.rs`).
- `MIO_PORT_BASE` — base of the e2e WebDriver port band each worktree's run claims a 16-port window from; default `45000` (`e2e/lib/run-identity.ts`).
- `MIO_DRIVER_PORT` — harness-set, not read by the Rust side: the `tauri-driver` WebDriver port allocated for the session (`beforeSession` in `e2e/wdio.conf.ts`, asserted by `e2e/test/specs/light/run-identity.spec.ts`).
- `MIO_NATIVE_PORT` — harness-set, not read by the Rust side: the `msedgedriver` native port paired with `MIO_DRIVER_PORT` (`e2e/wdio.conf.ts`).

# UI Components

This project uses [shadcn-svelte](https://shadcn-svelte.com) for UI primitives, re-skinned with the [retro-ui-svelte](https://retroui-svelte.netlify.app) registry's Sunset Orange (Rounded) theme — same `bits-ui` headless primitives and component file layout, different markup/classes/theme tokens (thick borders, hard drop-shadows, Archivo Black display font, Space Grotesk body font). Components live in `src/lib/components/ui/`.

When building UI features, always check `src/lib/components/ui/` first and reach for an existing component before writing custom markup.

Currently available components:
- `button`
- `collapsible`
- `command` (includes `command-dialog`)
- `dialog`
- `field` (Field.Set, Field.Group, Field.Field, Field.Label, Field.Description, Field.Error, etc.) — retro-ui-svelte has no `field` registry item; this component keeps shadcn-svelte markup, restyled by hand to match the retro look (see `docs/features/retro-ui-migration/02_dev.md`)
- `input`
- `input-group` — retro-ui-svelte has no `input-group` registry item either; same hand-restyle situation as `field`
- `label`
- `separator`
- `sonner` (toast notifications; mount a single `<Toaster />` in `src/routes/+layout.svelte`, call `toast.success`/`toast.error` from `svelte-sonner`)
- `switch`
- `tabs`
- `textarea`

## Adding new components

If a shadcn-svelte component would be a good fit but isn't in `src/lib/components/ui/` yet, ask the user if they'd like to add it before building a custom alternative. Pull from the retro-ui-svelte registry so new components match the Sunset Orange (Rounded) theme already established, not the default shadcn-svelte look:

```
pnpm dlx shadcn-svelte@latest add https://retroui-svelte.netlify.app/r/<component-name>.json
```

If retro-ui-svelte has no registry item for the component (check `https://retroui-svelte.netlify.app/r/<component-name>.json` for a 404), fall back to the plain shadcn-svelte registry (`pnpm dlx shadcn-svelte@latest add <component-name>`) and hand-restyle it to match: `border-2`, `shadow-md` (`hover:shadow-none` or `focus:shadow-xs` on interactive elements), `rounded-md` are the conventions used throughout the pulled components.

# Translations (i18n)

The app uses [Paraglide JS](https://paraglidejs.com/sveltekit) for internationalisation. Translation strings live in `messages/` as JSON files named by locale (e.g. `messages/en.json`, `messages/de.json`, `messages/ru.json`). 

## Adding or editing a translation key

Add the new key to `messages/en.json` first, then mirror the key in every other locale file (`messages/de.json`, `messages/ru.json`, etc.). Import the generated `m` object from `$lib/paraglide/messages.js` and call the key as a function (e.g. `m.my_key()`).

## Translating dynamic status strings

Container and sync status strings returned by the Rust backend are mapped to translation keys inside `translateStatus()` in `src/lib/sandbox.ts`. When the backend introduces a new status value, add a corresponding `status_<value>` key to all locale files and add an entry to the `map` object inside `translateStatus()`.

# Error Handling

## Rust (backend)

Three typed error enums, each in their module's `types.rs`:

`AppError` in `src-tauri/src/error.rs` is the IPC boundary type. It wraps `BackendError` and `SyncError` via `#[from]`, so `?` works across both in command handlers. Two command-layer variants live here too: `NotDeployed` and `UnsyncedChanges`.

All `#[tauri::command]` functions return `Result<T, AppError>`. Never use `Result<T, String>` for commands.

`AppError` serialises as `{ kind: string, message: string }` — **not** a plain string. `kind` is a stable discriminant matching the variant name (`"Backend"`, `"Sync"`, `"NotDeployed"`, `"UnsyncedChanges"`).

When adding a new `AppError` variant, add its arm to the `kind()` method or it won't compile.

## TypeScript (frontend)

`src/lib/errors.ts` exports:

- `AppError` interface — `{ kind: AppErrorKind, message: string }`
- `AppErrorKind` union — must stay in sync with Rust `kind()` arms
- `parseError(e)` — parse any `invoke()` rejection into `AppError`; falls back gracefully for non-Tauri throws

All `invoke()` catch blocks use `parseError(e).message` for display. Branch on `err.kind` for variant-specific logic (e.g. `"UnsyncedChanges"` triggers a confirmation dialog before force-destroy). Never use `String(e)` or substring matching on error messages.

# Code Comments

## Writing comments

- Max 2 lines per comment (3 if the constraint genuinely needs it)
- State the non-obvious contract or the WHY; never narrate the WHAT
- Avoid implementation details
- No source code in comments
- One idea per sentence
- Never write comments about your current work or process ("added X", "updated as requested", "new helper") — the git history is the record. The same applies at document scale: no work-log or change-summary files.
- TODO's for future are the exception and definitely allowed
- When two values in different files must stay equal, comment both ends with the other's path (pattern: `INFERENCE_VM_PORT` in `src-tauri/src/sync/mod.rs` ↔ `src/lib/agents/local-llm.ts`)
- Reference resolved design decisions by doc path (`docs/features/<x>/02_dev.md, Resolved Decision N`) instead of restating the rationale inline
- Exception: trait, interface, and public type declarations may carry fuller doc comments — they are the discovery surface for agents reading `types.rs`/module heads instead of whole files
- Use doc-comment form (`///`, `/** */`) on public items so tooling and outlines surface them; plain `//` belongs inside function bodies

## Editing code that has comments

- Comments are part of the code you're editing: update or delete an adjacent comment that your change makes false. A stale comment is worse than none.
- Never delete a comment stating an ordering constraint, invariant, or hard-won why unless you removed the constraint itself. If it's too long, compress it — don't drop it.

## Example

Bad — narrates the WHAT, adds process noise:

```rust
// Terminate the session and then flush. Updated to fix the undeploy bug.
```

Good — states the constraint the code can't show:

```rust
// Order is load-bearing: remove the container before terminating the session —
// terminating drops Mutagen's SSH connections, briefly blocking a following `podman rm`.
```

# Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| New Tauri command exists in Rust but `invoke()` rejects with "command not found" | Command function not added to the `tauri::generate_handler![]` list in `src-tauri/src/lib.rs` | Add the command name to `generate_handler![]` in `src-tauri/src/lib.rs` |
| New `AppError` variant does not compile | `kind()` in `src-tauri/src/error.rs` has an exhaustive match over variants | Add a `kind()` arm in `src-tauri/src/error.rs` and mirror the string in `AppErrorKind` in `src/lib/errors.ts` |
| A status badge shows a raw English string like `up-to-date` in non-English locales | Backend emitted a status value with no `status_<value>` key | Add `status_<value>` to every file in `messages/` and an entry to the `map` in `translateStatus()` (`src/lib/sandbox.ts`) |
| Containerfile change has no effect on new sandboxes | `ensure_base_image` short-circuits on `podman image exists`, so an unchanged image tag keeps the stale image | Bump `BASE_IMAGE_VERSION` in `src-tauri/src/backends/mod.rs` whenever `src-tauri/resources/Containerfile` changes |
| Feature works with `./build e2e light` but fails against real Podman | The `light` tier runs entirely against in-memory mocks (`MIO_MOCK_BACKEND=1`) and proves UI/store logic only | Write or run the feature's `standard` tier spec (`e2e/test/specs/standard/`); `e2e/AGENTS.md` defines `standard` as the default tier |
| A `$state` store resets on route change | Component-local state is destroyed by the router | Put shared state in a module-scoped `.svelte.ts` store under `src/lib/` (pattern: `src/lib/activity/activity.svelte.ts`); see `src/AGENTS.md` |
| A component added via `pnpm dlx shadcn-svelte@latest add <name>` (no URL) looks visually inconsistent with the rest of the app | The plain command pulls from the default shadcn-svelte registry, not retro-ui-svelte | Always use `pnpm dlx shadcn-svelte@latest add https://retroui-svelte.netlify.app/r/<name>.json` |
| `bg-destructive-hover`/`hover:bg-destructive-hover` has no visible hover effect on a retro-pulled `Button` | retro-ui-svelte's `button.svelte` references this class but never defines a `--destructive-hover` token in any of its 16 theme files (upstream gap) | `src/app.css` already defines `--destructive-hover`/`--color-destructive-hover`; if a future theme swap drops it, re-add a manually chosen darker/lighter destructive shade |
