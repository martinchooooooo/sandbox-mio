# Repo Map

Sandbox-mio is a Tauri 2 desktop app (Windows-first) that manages per-project "sandboxes": Podman containers with a bind-mounted workspace, a mise-managed runtime, embedded zellij web terminals, installable AI coding agents, remote deploys with Mutagen file sync, and local LLM inference.

| Area | Contents | Orientation doc |
|---|---|---|
| `src/` | SvelteKit + Svelte 5 frontend (static adapter, runs in the Tauri webview) | `src/AGENTS.md` |
| `src-tauri/src/` | Rust backend: Tauri commands + trait-injected subsystems, each with an in-memory mock | `src-tauri/AGENTS.md` |
| `e2e/` | WebdriverIO tests in three tiers (`light`/`standard`/`heavy`) | `e2e/AGENTS.md` |
| `docs/features/<name>/` | Per-feature docs pipeline: `01_ideas.md` → `02_dev.md` → `03_e2e.md` | `docs/features/README.md` |
| `messages/` | Paraglide locale files (`en.json`, `de.json`, `ru.json`) | Translations section below |

Read the orientation doc for an area before editing files in that area. `AGENTS.md` files in subdirectories override the root file for their subtree.

# Feature Workflow

Features flow through the project skills, each writing one document under `docs/features/<feature>/`: `feature-ideas` (01_ideas.md, scoping) → `feature-dev` (02_dev.md, build plan) → `feature-implement` (code, phase by phase, keeps 02_dev.md status current) → `feature-e2e` (03_e2e.md, regression plan). When asked to build a feature, check for an existing `docs/features/<feature>/` directory first and resume from the most advanced document present. `02_dev.md` files record resolved design decisions — consult the feature's `02_dev.md` before changing behaviour that document specifies.

# Verification Commands

Run before declaring any change done:

- `pnpm check` (repo root) — svelte-check over the frontend. Required after any `src/` change.
- `cargo check` (in `src-tauri/`) — required after any Rust change. `cargo test` runs the unit tests (currently only `src-tauri/src/workspace/mod.rs` has any).
- `pnpm run test:light` (in `e2e/`) — seconds, no Podman needed (`MIO_MOCK_BACKEND=1`); the cheapest end-to-end signal.
- `pnpm test` (in `e2e/`) — the `standard` tier against a real Podman machine; minutes. See `e2e/AGENTS.md` for tier selection.

# Environment Variables

All read via `std::env::var` at call time, never cached:

- `PODMAN_CONNECTION` — Podman system connection name; default `podman-machine-default` (`src-tauri/src/backends/mod.rs`).
- `SANDBOX_WORKSPACE_ROOT` — root for sandbox workspace directories; default `<Documents>/sandbox-mio` (`workspace_root()` in `src-tauri/src/backends/mod.rs`).
- `MIO_MOCK_BACKEND=1` — swaps every subsystem trait to its in-memory mock; used by the e2e `light` tier (`mock_backend_enabled()` in `src-tauri/src/backends/mod.rs`).
- `MIO_TEST_MODEL` — overrides the model used by inference e2e specs.

# UI Components

This project uses [shadcn-svelte](https://shadcn-svelte.com) for UI primitives. Components live in `src/lib/components/ui/`.

When building UI features, always check `src/lib/components/ui/` first and reach for an existing component before writing custom markup.

Currently available components:
- `button`
- `field` (Field.Set, Field.Group, Field.Field, Field.Label, Field.Description, Field.Error, etc.)
- `input`
- `label`
- `separator`
- `sonner` (toast notifications; mount a single `<Toaster />` in `src/routes/+layout.svelte`, call `toast.success`/`toast.error` from `svelte-sonner`)
- `switch`

## Adding new components

If a shadcn-svelte component would be a good fit but isn't in `src/lib/components/ui/` yet, ask the user if they'd like to add it before building a custom alternative. Components can be added via:

```
pnpm dlx shadcn-svelte@latest add <component-name>
```

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

- Code should be self explanatory; Avoid comments as much as possible.
- If a comment is needed, the total comment must not be longer than 2 lines.
- Do not add comments that are referring to your current work or process, only leave comments that exaplin the WHY, not the WHAT.
- TODO's for future are the exception and definitely allowed

here are futher guidelines to follow if comments are needed:

1. **one or two lines, not paragraphs.** State the non-obvious contract (idempotency, ordering constraints, what the return value represents). Skip anything the parameter names and return type already convey.
2. **State the contract, not the implementation.** Describe what the function guarantees to the caller, not how it achieves it. Where the contract is non-obvious, state when the call returns or what it resolves to.
3. **No source code in comments.** Reference other items by name and file path, never by quoting their body.
4. **One sentence per idea.** No filler, no restating the function name.

# Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| New Tauri command exists in Rust but `invoke()` rejects with "command not found" | Command function not added to the `tauri::generate_handler![]` list in `src-tauri/src/lib.rs` | Add the command name to `generate_handler![]` in `src-tauri/src/lib.rs` |
| New `AppError` variant does not compile | `kind()` in `src-tauri/src/error.rs` has an exhaustive match over variants | Add a `kind()` arm in `src-tauri/src/error.rs` and mirror the string in `AppErrorKind` in `src/lib/errors.ts` |
| A status badge shows a raw English string like `up-to-date` in non-English locales | Backend emitted a status value with no `status_<value>` key | Add `status_<value>` to every file in `messages/` and an entry to the `map` in `translateStatus()` (`src/lib/sandbox.ts`) |
| Containerfile change has no effect on new sandboxes | `ensure_base_image` short-circuits on `podman image exists`, so an unchanged image tag keeps the stale image | Bump `BASE_IMAGE_VERSION` in `src-tauri/src/backends/mod.rs` whenever `src-tauri/resources/Containerfile` changes |
| Feature works with `pnpm run test:light` but fails against real Podman | The `light` tier runs entirely against in-memory mocks (`MIO_MOCK_BACKEND=1`) and proves UI/store logic only | Write or run the feature's `standard` tier spec (`e2e/test/specs/standard/`); `e2e/AGENTS.md` defines `standard` as the default tier |
| A `$state` store resets on route change | Component-local state is destroyed by the router | Put shared state in a module-scoped `.svelte.ts` store under `src/lib/` (pattern: `src/lib/activity/activity.svelte.ts`); see `src/AGENTS.md` |
