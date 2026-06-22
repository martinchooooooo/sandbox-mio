
# UI Components

This project uses [shadcn-svelte](https://shadcn-svelte.com) for UI primitives. Components live in `src/lib/components/ui/`.

When building UI features, always check `src/lib/components/ui/` first and reach for an existing component before writing custom markup.

Currently available components:
- `button`
- `field` (Field.Set, Field.Group, Field.Field, Field.Label, Field.Description, Field.Error, etc.)
- `input`
- `label`
- `separator`
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

Container and sync status strings returned by the Rust backend are mapped to translation keys inside `translateStatus()` in `src/routes/projects/+page.svelte`. When the backend introduces a new status value, add a corresponding `status_<value>` key to all locale files and add an entry to the `map` object inside `translateStatus()`.

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
