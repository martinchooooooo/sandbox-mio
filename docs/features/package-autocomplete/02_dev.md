---
type: dev-plan
title: "Package Autocomplete — Build Plan"
feature: package-autocomplete
---
# Package Autocomplete — Build Plan

This document is the build plan for the package-autocomplete feature described in `docs/features/package-autocomplete/01_ideas.md`. The feature adds autocomplete to the package install input in `src/routes/projects/SandboxItem.svelte`: a new `list_registry` Tauri command runs `mise registry --json` inside the sandbox container and returns name–description pairs; the frontend replaces the plain `<input>` with a Combobox-backed widget that filters the cached registry and shows suggestions with descriptions as the user types. The existing `install_package` / `uninstall_package` / `list_packages` commands and the `installPackage()` / `uninstallPackage()` handlers are unchanged — autocomplete is purely additive. **All work is Remaining — nothing is implemented yet.**

## The Concrete Model

### Rust: `RegistryEntry`, `registry` trait method, process-lifetime cache

`RegistryEntry` is a new serialisable struct in `src-tauri/src/runtime/types.rs`:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegistryEntry {
    pub short: String,
    pub description: Option<String>,
}
```

`RuntimeManager` (`src-tauri/src/runtime/types.rs`) gains a new method:

```rust
/// Run `mise registry --json` in container `name` and return the full tool list.
/// Implementations cache the result process-wide; the first call pays the `podman exec` cost.
async fn registry(&self, name: &str) -> Result<Vec<RegistryEntry>, RuntimeError>;
```

`PodmanMiseRuntime` (`src-tauri/src/runtime/mod.rs`) gains a `registry_cache` field and the implementation:

```rust
pub struct PodmanMiseRuntime {
    pub connection: String,
    registry_cache: tokio::sync::OnceCell<Vec<types::RegistryEntry>>,
}
```

`default_runtime()` initialises `registry_cache: tokio::sync::OnceCell::new()`.

The `registry` implementation calls `get_or_try_init` on the cell, running `mise registry --json` via `mise_exec` (which starts the container if stopped, exactly as `install` does). A local `#[derive(Deserialize)]` struct mirrors the full JSON shape so only `short` and `description` are extracted; `backends` and `aliases` are dropped before the result is stored:

```rust
async fn registry(&self, name: &str) -> Result<Vec<RegistryEntry>, RuntimeError> {
    self.registry_cache.get_or_try_init(|| async {
        #[derive(serde::Deserialize)]
        struct Raw { short: String, description: Option<String> }
        let json = self.mise_exec(name, &["registry", "--json"]).await?;
        let raw: Vec<Raw> = serde_json::from_str(&json)
            .map_err(|e| RuntimeError::ManifestParse(format!("mise registry --json: {e}")))?;
        Ok(raw.into_iter().map(|r| RegistryEntry { short: r.short, description: r.description }).collect())
    }).await.cloned()
}
```

### Rust: `list_registry` Tauri command

Added to `src-tauri/src/lib.rs` and registered in `invoke_handler`:

```rust
#[tauri::command]
async fn list_registry(
    name: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<Vec<runtime::types::RegistryEntry>, AppError> {
    Ok(runtime.registry(&name).await?)
}
```

`RuntimeError` already converts into `AppError` via `#[from]` in `src-tauri/src/error.rs`; no new variant is needed.

### Frontend: module-level registry store

`src/lib/registry.svelte.ts` (`.svelte.ts` required for `$state` at module level) holds the process-wide cache and the fetch function:

```ts
import { invoke } from "@tauri-apps/api/core";

export interface RegistryEntry { short: string; description?: string; }

let entries = $state<RegistryEntry[]>([]);
let fetched = false;

export function registryEntries(): RegistryEntry[] { return entries; }

export async function ensureRegistry(sandboxName: string): Promise<void> {
    if (fetched) return;
    fetched = true; // set before await to prevent concurrent calls
    try {
        entries = await invoke<RegistryEntry[]>("list_registry", { name: sandboxName });
    } catch {
        // Best-effort: leave entries empty; the input degrades to plain text with no suggestions.
        fetched = false; // allow a retry on next focus if the container was transiently unavailable
    }
}
```

`fetched` resets to `false` on error so a transiently-stopped container does not permanently poison the cache; on the next focus the fetch is retried. Once successful, `fetched` stays `true` for the process lifetime so `mise registry --json` runs at most once per session.

### Frontend: Combobox widget in SandboxItem

The plain `<input>` in `src/routes/projects/SandboxItem.svelte` (lines 323–329) is replaced with an inline autocomplete widget. The widget uses the shadcn-svelte `Command` component (added via `pnpm dlx shadcn-svelte@latest add command`) for the suggestion list rather than the full `Popover`-anchored Combobox, because the package input is inline in a tight row and a popover anchor would clip against the list container. The input itself remains a plain `<input>` (same `bind:value={packageInput}`, same `placeholder`, same `onkeydown` Enter handler) so the existing page-object selector `input[placeholder='package name']` continues to work unchanged; a `<div class="relative">` wrapper holds a `Command.Root` dropdown that appears below the input when the filtered list is non-empty.

State additions to `SandboxItem.svelte`:

```ts
import { registryEntries, ensureRegistry } from "$lib/registry.svelte";
import * as Command from "$lib/components/ui/command";

let showSuggestions = $state(false);

const filteredEntries = $derived(
    packageInput.length === 0
        ? []
        : registryEntries().filter(e => e.short.includes(packageInput)).slice(0, 20)
);
```

`slice(0, 20)` caps the visible list to keep the dropdown compact; the substring match is case-sensitive because mise tool names are lowercase.

On input `onfocus`: call `ensureRegistry(project.name)` and set `showSuggestions = true`.  
On input `onblur`: set `showSuggestions = false` after a `setTimeout(0)` tick so a `Command.Item` click fires before the list disappears.  
`showSuggestions` is also set to `false` after `installPackage()` completes successfully.

The markup replacement (the `<input>` and surrounding `<div>` at lines 322–332 of `SandboxItem.svelte`):

```svelte
<div class="relative flex-1">
    <input
        class="w-full rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
        placeholder={m.projects_package_placeholder()}
        bind:value={packageInput}
        disabled={action.busy !== null}
        onfocus={() => { showSuggestions = true; ensureRegistry(project.name); }}
        onblur={() => setTimeout(() => (showSuggestions = false), 0)}
        onkeydown={(e) => e.key === "Enter" && installPackage()}
    />
    {#if showSuggestions && filteredEntries.length > 0}
        <Command.Root class="absolute left-0 top-full z-50 mt-1 w-full rounded border bg-popover shadow-md">
            <Command.List>
                {#each filteredEntries as entry (entry.short)}
                    <Command.Item
                        value={entry.short}
                        onSelect={() => { packageInput = entry.short; showSuggestions = false; }}
                        class="flex flex-col items-start px-2 py-1.5 text-xs"
                    >
                        <span class="font-mono">{entry.short}</span>
                        {#if entry.description}
                            <span class="text-muted-foreground">{entry.description}</span>
                        {/if}
                    </Command.Item>
                {/each}
            </Command.List>
        </Command.Root>
    {/if}
</div>
```

The outer `<div class="flex items-center gap-2">` that holds the label, input, and Install button is unchanged except that the `<input>` is now inside the new `<div class="relative flex-1">` wrapper instead of being a direct `flex-1` child.

## Resolved Decisions

- **`mise registry --json` output format.** Resolved: the 2026-06-28 spike confirmed the format as a JSON array of `{ short, backends, description?, aliases? }` objects (983 entries in the live container). The Rust implementation deserialises into a local `Raw` struct and maps to `RegistryEntry { short, description }`; `backends` and `aliases` are discarded.
- **Version hints in the suggestion list.** Resolved: suggestions show only the tool short name and its description — no pinned-version hints. Adding version suffixes (`node@22`, `node@lts`) would require either a curated list or a `mise ls-remote <tool>` round-trip per suggestion; both are out of scope. Users who need a specific version type `node@22` directly into the input after selecting or without selecting.
- **Graceful degradation when the container cannot start.** Resolved: `ensureRegistry` silently swallows `list_registry` failures and resets `fetched` to `false` so the next focus retries. No error is shown in the `StatusRow`; the input remains fully functional as a plain text field. Registry fetch failure is a best-effort enhancement miss, not an action failure.
- **Registry cache invalidation across base-image updates.** Resolved: the cache is process-lifetime. A base-image rebuild during the session is an edge case; the new registry is loaded on the next app restart. No invalidation hook is added to `ensure_base_image`.
- **Combobox vs. inline Command dropdown.** Resolved: the inline `Command.Root` dropdown positioned `absolute` below the input is used instead of the full `Popover`-anchored Combobox, because the Combobox pattern replaces the trigger with a Button that opens a palette on click, which changes the UX from "type to filter" to "click to open and then type". The inline pattern keeps the `<input>` as the direct interaction target and preserves the `input[placeholder='package name']` page-object selector.

## Remaining: Phase A — Rust backend

Touch `src-tauri/src/runtime/types.rs`:
- Add `RegistryEntry` struct with `#[derive(Debug, Clone, serde::Serialize)]`.
- Add `registry(&self, name: &str) -> Result<Vec<RegistryEntry>, RuntimeError>` to `RuntimeManager`.

Touch `src-tauri/src/runtime/mod.rs`:
- Add `registry_cache: tokio::sync::OnceCell<Vec<types::RegistryEntry>>` to `PodmanMiseRuntime`.
- Update `default_runtime()` to set `registry_cache: tokio::sync::OnceCell::new()`.
- Implement `registry` on `PodmanMiseRuntime`: call `get_or_try_init`, deserialise JSON via a local `Raw` struct with `#[derive(serde::Deserialize)]`, map to `RegistryEntry`, store and return `.cloned()`.

Touch `src-tauri/src/lib.rs`:
- Add `list_registry` command function (imports `runtime::types::RegistryEntry`).
- Add `list_registry` to `tauri::generate_handler!`.

No new `AppError` variant; no new locale keys; no Containerfile change.

## Remaining: Phase B — Frontend

Step 1 — add the `Command` component:
```
pnpm dlx shadcn-svelte@latest add command
```
This creates `src/lib/components/ui/command/` with the `Command.Root`, `Command.List`, `Command.Item`, and `Command.Input` sub-components.

Step 2 — create `src/lib/registry.svelte.ts` with `RegistryEntry`, `registryEntries()`, and `ensureRegistry(sandboxName)` as specified in the Concrete Model section above.

Step 3 — edit `src/routes/projects/SandboxItem.svelte`:
- Add imports for `registryEntries`, `ensureRegistry` from `$lib/registry.svelte` and `* as Command` from `$lib/components/ui/command`.
- Add `let showSuggestions = $state(false)`.
- Add `filteredEntries` derived value.
- Replace the plain `<input>` (lines 323–329) with the relative-wrapper + inline Command dropdown markup as specified in the Concrete Model section.
- The Install `<Button>` (line 330) and the packages chip list below remain unchanged.

No i18n keys are added; no locale files are touched.

## Lifecycle and Cross-Cutting Concerns

- **Module-level cache scope.** `registryEntries` and the `fetched` flag live at module scope in `registry.svelte.ts`, not inside `SandboxItem`. All sandbox rows share the same cache: focusing any row's package input fetches once, and all other rows benefit immediately. This is correct because all sandboxes run the same mise version.
- **Blur-before-click race.** The `onblur` handler that hides the dropdown runs before the `Command.Item` click handler fires. The `setTimeout(0)` defers `showSuggestions = false` by one microtask tick, giving the `onSelect` handler time to fire and set `packageInput`. Without this deferral, clicking a suggestion closes the dropdown before the selection is applied.
- **`z-index` and overflow.** The `<li>` row uses `flex flex-col gap-2 rounded-lg border p-3` without `overflow: hidden`, so the `absolute` dropdown can escape the row boundary. If a parent container uses `overflow: hidden`, move the dropdown to a Popover so it mounts in a portal.
- **`filteredEntries` is empty when input is empty.** This prevents the dropdown from opening on focus of an empty input, which would show the full 983-entry list. The dropdown appears only when the user has typed at least one character and at least one entry matches.
- **`action.busy` disables the input.** When `action.busy !== null` the input is `disabled`, which also fires `onblur` and closes the dropdown. No special handling is needed.
- **`onkeydown` Enter during suggestion display.** When the dropdown is open, pressing Enter calls `installPackage()` with whatever is in `packageInput` — which may be the user's partial text or a suggestion they have not clicked yet. This matches the existing behaviour (Enter installs the typed value). Keyboard selection within the Command list is handled by the Command component's own keyboard navigation; pressing Enter on a highlighted item fires `onSelect`, populates `packageInput`, and closes the dropdown. A second Enter then installs.
