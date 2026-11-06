---
type: ideas
title: "Package Autocomplete Ideas"
feature: package-autocomplete
---
# Package Autocomplete Ideas

This document covers the ideation for adding autocomplete to the sandbox package install input. The install flow is fully implemented — the Rust `install_package` command runs `mise use <pkg>` in the container, `uninstall_package` runs `mise rm`, and `list_packages` reads `mise.toml` — but the frontend input in `src/routes/projects/SandboxItem.svelte` (lines 323–329) is a plain `<input>` that the user fills manually with no guidance on what is available. The proposed model replaces that input with an autocomplete-capable input that queries the mise tool registry to surface matching tool names as the user types.

## Current State

The package section lives entirely in `src/routes/projects/SandboxItem.svelte`. The input is a plain `<input>` bound to `packageInput` (line 47) and submits by pressing Enter or clicking the Install button (line 330). On submit, `installPackage()` (line 70) calls the Tauri command `install_package` (wired in `src-tauri/src/lib.rs` line 65), which delegates to `RuntimeManager::install` on `PodmanMiseRuntime` (`src-tauri/src/runtime/mod.rs` line 145) and runs `mise use <pkg>` inside the container over `podman exec`.

The `mise` binary is baked into the sandbox base image (`src-tauri/resources/Containerfile`). All sandboxes share the same pinned `MISE_VERSION`, so the registry reported by that binary is identical across all local sandboxes. No host-side `mise` binary is expected or installed by this project; the binary lives only inside the container.

Validation of the package name string happens server-side in `validate_package` (`src-tauri/src/runtime/mod.rs` line 44), which allows letters, digits, and the characters `- _ . / # : @ +` — all characters needed for mise tool specs like `node@22`, `aqua:cli/cli`, and `cargo:ripgrep`.

## Proposed Model

### New Tauri Command: `list_registry`

A new `list_registry(name: String)` command is added to `src-tauri/src/lib.rs` and a new `registry` method is added to the `RuntimeManager` trait in `src-tauri/src/runtime/types.rs`. The implementation in `src-tauri/src/runtime/mod.rs` runs `mise registry --json` inside the named container over `podman exec` and returns a list of `{ short: String, description: Option<String> }` structs. Using `--json` gives descriptions alongside short names in a single call; the text-format output has no description column. Since the registry is determined by the mise binary version baked into the base image, it is identical across all sandboxes, so the result is cached for the lifetime of the app process (a `tokio::sync::OnceCell<Vec<RegistryEntry>>` on `PodmanMiseRuntime`) — the first call pays the `podman exec` cost and subsequent calls from any sandbox return immediately.

### Autocomplete Input in SandboxItem

`packageInput` in `src/routes/projects/SandboxItem.svelte` is promoted from a plain `<input>` to an autocomplete-capable widget. When the user focuses the input or types, the widget filters the cached registry list client-side and displays matching suggestions with their description shown as a subtitle below the tool name. Selecting a suggestion populates `packageInput` with the short name and the existing `installPackage()` path proceeds unchanged. If the container is stopped, `list_registry` starts it (same as `mise_exec`), and if it fails the input degrades gracefully to the current plain-input behaviour with no suggestions shown.

### Client-side Filtering Over Full Registry

The registry list is fetched once per app session on first focus of any package input (lazy, not at mount time) and held in a Svelte module-level store so all sandbox rows share the same fetched list. Filtering is done entirely on the frontend by substring-matching the user's typed value against the tool short names — no round-trip per keystroke. The registry list is on the order of hundreds of entries (mise's registry is currently ~300 tools), so a linear scan is imperceptible.

## Technology Choices with Tradeoffs

### Autocomplete data source: container `mise registry`

`mise registry` is run inside the sandbox container via `podman exec`. This is the chosen path because it guarantees the returned tool list exactly matches what the pinned mise binary in the sandbox will accept — a tool name valid in the registry is installable, and a name not in the registry will fail at install time regardless. The alternatives evaluated:

- **Public mise registry API or raw GitHub file** — the mise registry is published in `https://github.com/jdx/mise/blob/main/registry.toml` and rendered at `https://mise.jdx.dev/registry.html`, but there is no stable, versioned JSON endpoint for the registry. Fetching raw GitHub content ties autocomplete to the latest registry revision, which may not match the pinned `MISE_VERSION` baked into the base image — a tool present in the latest registry may not be resolvable by the older in-container binary. Rejected because of the version-mismatch risk and the external HTTP dependency from a Tauri desktop app.
- **Host-side `mise registry`** — no `mise` binary is installed on the Windows host by this project; expecting one would add a prerequisite not needed anywhere else. Rejected.
- **Pre-bundled registry snapshot** — a snapshot baked into the app binary at build time goes stale whenever the base image's `MISE_VERSION` is bumped. Rejected because it requires a coordinated two-file update (binary + registry snapshot) on every version change and offers no benefit over the in-container command.

### Frontend autocomplete UI: shadcn-svelte Combobox

The autocomplete UI is built with the shadcn-svelte `combobox` component (added via `pnpm dlx shadcn-svelte@latest add combobox` as noted in `AGENTS.md`). The Combobox provides a popover-anchored suggestion list with keyboard navigation, is stylistically consistent with the rest of the UI, and stays within the shadcn-svelte component system the project already uses (`AGENTS.md` specifies reaching for shadcn-svelte components first). The alternatives evaluated:

- **HTML `<datalist>`** — native browser autocomplete; no new component, zero JS complexity. Rejected because `<datalist>` renders browser-native suggestion UI that does not match the app's design system, and its appearance inside WebView2/Webview varies across platforms and is not styleable with Tailwind.
- **Custom dropdown `<div>` with `<ul>`** — full style control without adding a component. Rejected because it requires reimplementing keyboard navigation, focus management, and ARIA roles that the shadcn-svelte Combobox already provides and that are easy to get subtly wrong.

### Cache scope: process-lifetime `OnceCell` on `PodmanMiseRuntime`

The registry list is cached as a `tokio::sync::OnceCell<Vec<String>>` on the `PodmanMiseRuntime` struct, populated on the first `registry()` call and reused for all subsequent calls until the app restarts. The alternatives evaluated:

- **Per-call (no cache)** — every keystroke or focus event would trigger a `podman exec mise registry` round trip, which takes hundreds of milliseconds and would start the container if stopped. Rejected as too slow.
- **Time-to-live (TTL) cache** — refreshes the registry every N minutes. Rejected because the registry is determined by the mise binary version in the base image, which changes only when `MISE_VERSION` is bumped in the Containerfile; a TTL adds complexity for no real benefit between restarts.
- **Frontend Svelte store** — the registry could be fetched once and held in a Svelte module-level `$state` variable. This is the right scope for the *frontend* copy (shared across all sandbox rows in the same page), but the Rust side still benefits from a cache to avoid re-running `podman exec` on every Tauri IPC call. Both layers cache: the Rust `OnceCell` absorbs repeated IPC calls, the Svelte store avoids IPC once the data is in the frontend.

### Fetch trigger: on first input focus, lazy

The registry fetch is triggered when the user first focuses any package input in the app, not at component mount time. This means `list_registry` is never called for users who never click the package input (e.g. users who only manage deployments). The alternative of fetching at mount — so that suggestions are ready before the user types — was considered and rejected: it would call `podman exec` on every sandbox card render, potentially starting stopped containers unnecessarily.

## Validation

A spike was run on 2026-06-28 by running `podman exec dfsdf mise registry` against the live sandbox container on `podman-machine-default`.

Cleared risks:

- **`mise registry --json` output format** — confirmed. Returns a JSON array; each object has `short` (the install name), `backends` (array of strings), an optional `description` string, and an optional `aliases` array. Example: `{ "short": "1password", "backends": ["vfox:...", "aqua:..."], "description": "Password manager developed by AgileBits Inc", "aliases": ["1password-cli", "op"] }`. Command: `podman exec dfsdf mise registry --json | head -40`.
- **Descriptions are present and useful** — every sampled entry had a `description` field; these are the same strings shown by `mise use` interactive search. They will be surfaced as subtitles in the Combobox suggestion list.
- **Registry size** — confirmed at **983 entries** (text format count; JSON entry count is the same). A linear substring scan on the frontend is imperceptible at this size. Command: `podman exec dfsdf mise registry | wc -l`.
- **Aliases exist as sibling top-level entries** — `1password`, `1password-cli`, and `op` each appear as their own `short` entry with identical backends and description. The suggestion list surfaces all three; they are valid distinct install targets and a user searching `op` will find the entry directly.
- **Multiple backends per entry are common** — e.g. `act` lists `aqua:nektos/act` and `asdf:gr1m0h/asdf-act`. Backends are dropped server-side; only `short` and `description` reach the frontend.

## Open Questions

- **`mise registry` output format inside the container.** Resolved by the 2026-06-28 spike: `mise registry --json` returns a JSON array of `{ short, backends, description?, aliases? }` objects. The `registry()` implementation deserialises this and returns `Vec<RegistryEntry>` with `short` and `description` fields; backends and aliases are dropped before the result leaves Rust.
- **Version hints in the suggestion list.** Whether suggestions show only the tool short name (`node`) or also a common pinned form (`node@lts`, `node@22`) to guide new users toward version-qualified installs is undecided. Showing only names is simpler; appending common suffixes requires a curated list or a second `mise ls-remote <tool>` call per suggestion that is likely too slow.
- **Graceful degradation when the container cannot start.** If `list_registry` fails (container unreachable, connection down), the input degrades to plain text with no suggestions. Whether an error is surfaced in the `StatusRow` or silently swallowed is undecided; a silent fallback avoids noise for common transient failures, but a logged warning aids debugging.
- **Registry refresh across base-image updates.** When `BASE_IMAGE_VERSION` bumps (the user rebuilds the base image), the cached `OnceCell` is stale until the app restarts. Whether the cache should be invalidated on `ensure_base_image` completion or left as process-lifetime is undecided; the base image is rarely rebuilt mid-session so process-lifetime is probably fine.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `list_registry` starts a stopped container silently | `mise_exec` calls `podman start` before every `podman exec`, including the registry fetch | Call `list_registry` lazily on user focus, not at mount, so stopped containers are not started until the user actively tries to install a package |
| Suggestions mismatch what the installed mise will accept | Registry fetched from a different mise version than the one in the container | Always fetch from `mise registry` inside the container, never from the public GitHub registry file or a bundled snapshot |
| Using the text-format `mise registry` output loses descriptions | The plain text format has no description column; descriptions are only in `--json` | Always use `mise registry --json` and deserialise into `{ short, description }` structs |
| Combobox not available in `src/lib/components/ui/` | shadcn-svelte's combobox has not been added yet | Run `pnpm dlx shadcn-svelte@latest add combobox` before implementing the frontend (per the `AGENTS.md` convention) |
| All sandbox rows trigger `list_registry` on mount | Fetching at mount runs `podman exec` for every card rendered | Trigger the fetch on first user focus of the input, not in `onMount` |
