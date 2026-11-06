---
type: dev-plan
title: "Retro UI Migration — Build Plan"
feature: retro-ui-migration
---
# Retro UI Migration — Build Plan

This document is the build plan for migrating Sandbox-mio's UI primitives from
stock shadcn-svelte to the [retro-ui-svelte](https://github.com/Mhamad6000/retroui-svelte)
registry. No `01_ideas.md` exists for this feature: the option space was
resolved directly in conversation with the user rather than in a separate
ideation document, so this document's "Resolved Decisions" section stands in
for that step. Status: **nothing implemented yet**; this is the plan only.

## The concrete model

retro-ui-svelte is not an independent component library. It is a
[shadcn-svelte](https://shadcn-svelte.com) registry: same underlying
`bits-ui` headless primitives, same install mechanism
(`pnpm dlx shadcn-svelte@latest add <url>`), same component file layout under
`src/lib/components/ui/<component>/`. It only supplies different markup,
Tailwind classes, and CSS theme tokens (heavier borders, hard drop-shadows,
bold display font for headings). Migration is therefore a **re-skin**, not a
rewrite: component call sites (`<Button>`, `<Dialog.Root>`, etc.) keep their
existing props and composition API. The work is (1) swap the CSS theme
tokens/fonts in `src/app.css`, (2) re-pull each existing component from
`https://retroui-svelte.netlify.app/r/<component>.json` instead of the
default shadcn-svelte registry, re-applying any project-specific
customizations the CLI overwrite clobbers, and (3) visually sweep every
consuming file for spacing/layout breakage caused by the heavier retro
styling.

Current shadcn-svelte components under `src/lib/components/ui/` (verified by
`Glob` against the working tree, which is a superset of what
`AGENTS.md`'s "UI Components" section currently lists):

- `button`
- `field` (`Field.Set`, `Field.Group`, `Field.Field`, `Field.Label`, `Field.Description`, `Field.Error`, `Field.Legend`, `Field.Title`, `Field.Separator`, `Field.Content`)
- `input`
- `input-group`
- `label`
- `separator`
- `sonner`
- `switch`
- `command` (includes `command-dialog`)
- `dialog`
- `textarea`
- `tabs`
- `collapsible`

Files under `src/` that import from `$lib/components/ui/*` (verified by
`Grep` for `from "$lib/components/ui/` across `src/`, 19 consuming files plus
the 6 internal cross-imports inside `ui/` itself):

- `src/lib/components/GlobalNav.svelte`
- `src/lib/workspace/NavigationColumn.svelte`
- `src/lib/workspace/WorkspaceDashboard.svelte`
- `src/lib/activity/ActivityCenter.svelte`
- `src/routes/+layout.svelte`
- `src/routes/+page.svelte`
- `src/routes/settings/+page.svelte`
- `src/routes/models/+page.svelte`
- `src/routes/credentials/+page.svelte`
- `src/routes/agent-auth/+page.svelte`
- `src/routes/projects/+page.svelte`
- `src/routes/projects/PortsManager.svelte`
- `src/routes/projects/AgentManager.svelte`
- `src/routes/projects/PackageManager.svelte`
- `src/routes/projects/SandboxItem.svelte`
- `src/routes/projects/CreateSandboxItem.svelte`
- `src/lib/components/ui/input-group/input-group-textarea.svelte`
- `src/lib/components/ui/input-group/input-group-input.svelte`
- `src/lib/components/ui/input-group/input-group-button.svelte`
- `src/lib/components/ui/dialog/dialog-footer.svelte`
- `src/lib/components/ui/dialog/dialog-content.svelte`
- `src/lib/components/ui/command/command-input.svelte`
- `src/lib/components/ui/command/command-dialog.svelte`
- `src/lib/components/ui/field/field-separator.svelte`
- `src/lib/components/ui/field/field-label.svelte`

Theme tokens live in `src/app.css`: `:root` block defines light-mode
variables (e.g. `--background: oklch(1 0 0)` at line 10, `--radius: 0.625rem`
at line 33), `.dark` block (from line 45) defines dark-mode overrides, and
the `@theme inline` block (from line 78) maps those variables into Tailwind's
theme (`--color-background`, `--radius-sm` through `--radius-4xl`).
Shadcn-svelte's registry config is `components.json` at the repo root.

## Resolved Decisions

- **Which retro-ui-svelte theme to adopt** — Sunset Orange, Rounded variant. Sourced from the "16 Beautiful Themes" set advertised on the retro-ui-svelte docs site; the rounded corner style is used rather than the sharp/squared alternative some retro themes offer.
- **Whether this is a full library swap or a re-skin** — re-skin. retro-ui-svelte is confirmed (via the project GitHub page and docs site) to be built on top of the shadcn-svelte registry system, so component APIs and the `bits-ui` primitives underneath do not change; only visual theme tokens and per-component markup/classes change.
- **Execution timing** — deferred. The user asked for the plan to be written down first; no phase has been started as of this document.

## Implementation phases

### Implemented: Phase 0 — Theme foundation

`src/app.css`'s `:root`/`.dark` blocks now hold the exact `light`/`dark` `cssVars`
from retro-ui-svelte's `https://retroui-svelte.netlify.app/r/theme-orange.json`
registry item (theme value `orange`, display label "Sunset Orange" per that
project's `src/lib/user-config.svelte.ts`), with `--radius` overridden to
`0.7rem` — the project's "Rounded" `radius-style` option (`box` is the
alternative, `0rem`) rather than the theme file's own default of `0.5rem`.
Two extra tokens the theme JSON doesn't define were added by inference from
the pulled component markup: `--input`/`--ring` (not present in
`theme-orange.json`, needed because `input`/`switch`/`dialog` reference
`bg-input`, `border-ring`, `ring-ring`; set to `--border`'s color and
`--primary`'s color respectively) and `--destructive-hover` (button.svelte's
markup references `hover:bg-destructive-hover`, a token retro-ui-svelte itself
never defines in any of its 16 theme files — a gotcha in the upstream
library; a manually chosen darker/lighter destructive shade was added so the
hover state isn't silently inert).

The `@theme inline` block gained `--font-sans` → `'Space Grotesk Variable'`,
`--font-head` → `'Archivo Black'`, the `--shadow-xs` through `--shadow-2xl`
hard-drop-shadow tokens (`Npx Npx 0 0 var(--border)`), and
`--color-primary-hover`/`--color-destructive-hover` so `bg-primary-hover`/
`bg-destructive-hover` utility classes resolve. Fonts are installed as
`@fontsource/archivo-black` and `@fontsource-variable/space-grotesk` (added to
`devDependencies`), replacing the prior `@fontsource-variable/inter` import
(and a stale `@fontsource-variable/figtree` import that had no matching
dependency in `package.json` at all).

`components.json`'s assumption held: no edit was needed since retro-ui-svelte
components are pulled via full per-component URLs, not a registry alias.

Verified via `pnpm check` — the only failures are 5 pre-existing errors in
`command.svelte`/`dialog-footer.svelte`/`dialog-content.svelte`/
`command-input.svelte` unrelated to this change (confirmed identical with
Phase 0's `app.css`/`package.json` changes stashed out).

### Implemented: Phase 1 — Re-pull components from the retro registry

**Plan deviation found and resolved during implementation:** retro-ui-svelte's
registry has no `field.json` or `input-group.json` — its full component list
(`static/r/*.json` in the `Mhamad6000/retroui-svelte` repo) does not include
`field` or `input-group` at all. Confirmed with the user (asked via
`AskUserQuestion`), who chose to hand-restyle these two rather than leave them
stock or defer them. The other 11 components (`button`, `label`, `separator`,
`input`, `textarea`, `switch`, `tabs`, `collapsible`, `dialog`, `command`,
`sonner`) all exist upstream and were re-pulled via
`pnpm dlx shadcn-svelte@latest add -y -o https://retroui-svelte.netlify.app/r/<name>.json`
in the planned order.

**field/input-group hand-restyle:** both already used semantic Tailwind
tokens, so they inherited the Sunset Orange palette automatically from
Phase 0's `app.css`. Only `field-label.svelte`'s bordered-wrapper state
(`has-[>[data-slot=field]]:*`) and `input-group.svelte`'s container needed
edits — changed `rounded-lg`/`border` to `rounded-md`/`border-2`/`shadow-md`
to match the `border-2`/`shadow-md`/`rounded-md` convention observed across
every pulled retro component (e.g. `input.svelte`, `dialog-content.svelte`).

**Customizations re-applied per component after overwrite** (found by
reading each component's pre-overwrite source, since `git diff` against `git
HEAD` is a no-op — HEAD already contains whatever customization exists; the
useful comparison is against the pulled registry file, not git history):

- `button.svelte`: the `loading` prop (disables + sets `aria-busy` for the
  button's duration) that retro's version doesn't have.
- `tabs-list.svelte`/`tabs-trigger.svelte`: retro's tabs has no `variant`
  prop at all (single box-pill look only), but `GlobalNav.svelte` passes
  `variant="line"` for the nav bar. Re-added a `variant` (`default`/`line`)
  prop using `group-data-[variant=x]/tabs-list:` selectors, keeping retro's
  pill look as `default` and adding a flatter underline `line` look.
- `dialog-content.svelte`/`dialog-footer.svelte`: swapped retro's
  `@lucide/svelte` close icon for the project's `@hugeicons/svelte`
  (`Cancel01Icon`), and restored the `Button`-wrapped close-button pattern
  (via `{#snippet child({ props })}`) plus `dialog-footer`'s
  `showCloseButton` prop, both dropped by retro's simpler markup. Also
  deleted `dialog.svelte`/`dialog-portal.svelte` — orphaned once retro's
  `index.ts` defines `Root`/`Portal` directly from `bits-ui` instead of
  wrapping them in their own files.
- `command.svelte`: retro's version has no `api`/`CommandRootApi` bindable
  at all. The project's prior version had one, but grep found zero
  consumers and it was the source of one of the two pre-existing
  `pnpm check` errors (`export` inside a non-`module` script) — dropping it
  (adopting retro's version as-is) fixed that error as a side effect.
  Deleted the now-orphaned `command-loading.svelte` (retro's `index.ts`
  aliases `Loading` straight from `CommandPrimitive.Loading`).
- `command-input.svelte`: restored the project's `InputGroup`-composed
  input (`{#snippet child({ props })}<InputGroup.Input {...props} .../>`)
  instead of retro's plain `<input>`, so it matches the input-group
  reskin, and swapped retro's `@lucide/svelte` search icon for
  `@hugeicons/svelte`'s `SearchIcon`. Adding an explicit
  `{ props }: { props: Record<string, unknown> }` type annotation on the
  snippet fixed the other pre-existing `pnpm check` error (implicit `any`).
- `command-item.svelte`: retro's version never renders `children` at all
  (relies on it flowing through un-destructured `restProps`) and has no
  selected-state checkmark. Restored the project's explicit
  `{@render children?.()}` plus a trailing Hugeicons `Tick02Icon` checkmark
  for `data-checked` state, since adding the checkmark requires
  destructuring (and therefore explicitly rendering) `children`.
- `sonner.svelte`: retro's version adds `toastOptions`/`unstyled` classes
  for the hard-border/shadow toast look but drops the project's five
  Hugeicons snippets (`loadingIcon`/`successIcon`/`errorIcon`/`infoIcon`/
  `warningIcon`). Kept retro's `toastOptions` and restored the icon
  snippets on top.

**Gotcha hit and fixed at call sites (not in the component), per the Common
Gotchas table below:** retro's `button.svelte` only has `size`:
`md`/`sm`/`lg`/`icon` (stock shadcn had `xs`/`icon-xs`/`icon-sm`/`icon-lg`
too). Three call sites used `size="icon-xs"`
(`src/routes/projects/PackageManager.svelte`,
`src/routes/projects/PortsManager.svelte`,
`src/routes/models/+page.svelte`) and were changed to `size="icon"`, the
only icon size retro offers.

An unresolved side effect worth noting for Phase 3: `app.css` added
`--destructive-hover`/`--color-destructive-hover` because
`button.svelte`'s `destructive` variant references `hover:bg-destructive-hover`,
a token retro-ui-svelte itself never defines in any of its 16 theme files
(upstream gap, not this migration's bug).

Verified via `pnpm check` after every single component pull — 0 errors at
the end, including all 5 of Phase 0's originally-baseline errors (across
`command.svelte`, `command-input.svelte`, `dialog-content.svelte`,
`dialog-footer.svelte`), resolved as side effects of the re-applications
above rather than fixed directly.

### Implemented: Phase 2 — Update AGENTS.md

Root `AGENTS.md`'s "UI Components" section now lists all 13 components, notes
the Sunset Orange (Rounded) theme choice, and its install instruction uses the
retro-ui-svelte registry URL form
(`pnpm dlx shadcn-svelte@latest add https://retroui-svelte.netlify.app/r/<component-name>.json`)
with a documented fallback to the plain shadcn-svelte registry plus hand-restyle
conventions (`border-2`/`shadow-md`/`rounded-md`) for the `field`/`input-group`
case where retro-ui-svelte has no registry item. Also added two entries to the
root Common Gotchas table: the registry-URL requirement, and the
`--destructive-hover` upstream-gap token from Phase 1.

### Partially implemented: Phase 3 — Visual sweep of consuming files

No headless browser automation was available in the implementation
environment (Playwright/chromium install was declined), and this is a Tauri
desktop webview, not a pure web app — actually driving it needs a live
window. Did a static code-level pass instead: grepped all consuming files for
`overflow-hidden` containers (the highest-risk pattern, since retro's
thicker borders/hard shadows can clip inside a fixed-size clipped box) and
manually checked the ones outside `ui/` — `src/routes/+layout.svelte` (outer
app-shell wrapper, low risk), `src/lib/workspace/WorkspaceContent.svelte`,
`src/lib/activity/ActivityCenter.svelte` (`Button` sits outside the clipped
panel div, no overlap). Nothing else stood out from static reading.

**The actual visual walkthrough (light + dark mode, golden path + edge cases
per consuming file) is deferred to the user**, per their explicit choice when
asked how to handle this phase — they will run `pnpm tauri dev` and report
back anything that looks broken for follow-up fixes. Treat this phase as open
until that feedback comes back.

### Partially implemented: Phase 4 — Verify

- `pnpm check` (repo root) — clean, 0 errors, confirmed after Phase 1's last
  component pull.
- Manual run of the app: deferred to the user (see Phase 3) — they will run
  `pnpm tauri dev` and report back.
- `pnpm run test:light` (in `e2e/`) — **blocked by a pre-existing environment
  gap unrelated to this feature**: `tauri build --debug` fails because
  `src-tauri/binaries/` doesn't exist in this worktree at all (needs
  `mutagen-x86_64-pc-windows-msvc.exe`, the externalBin declared in
  `src-tauri/tauri.conf.json`). This migration touches only `src/`, never
  `src-tauri/`, so the binary's absence predates and is unrelated to this
  work. Re-run `pnpm run test:light` once the worktree has the Mutagen
  sidecar binary in place.

## Lifecycle / cross-cutting concerns

- **No Rust/backend impact.** This feature touches only `src/`; no
  `src-tauri/` files are expected to change, so `cargo check` is not part of
  Phase 4.
- **No i18n impact.** No new translation keys are introduced; Paraglide
  message files under `messages/` are not touched by this migration.
- **Registry coupling.** Every future "add a new shadcn-svelte component"
  workflow described in `AGENTS.md` must use the retro-ui-svelte registry URL
  going forward, not the plain shadcn-svelte registry, or new components will
  visually clash with the Sunset Orange theme established in Phase 0.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A component added via `pnpm dlx shadcn-svelte@latest add <name>` (no URL) looks visually inconsistent with the rest of the app | The plain command pulls from the default shadcn-svelte registry, not retro-ui-svelte | Always use `pnpm dlx shadcn-svelte@latest add https://retroui-svelte.netlify.app/r/<name>.json` per `AGENTS.md` (updated in Phase 2) |
| Re-pulling a component from the retro registry silently drops a prior project-specific customization | The CLI `add` overwrites the component file wholesale | Diff the component directory against git before running `add`, and re-apply the customization after, per Phase 1 step 1/3 |
| Layout overflow or clipped text after a component is re-skinned | Retro theme uses thicker borders and hard drop-shadows than default shadcn-svelte, increasing effective element size | Address in Phase 3's visual sweep of the specific consuming file, not by editing the component itself |
| `pnpm check` fails after re-pulling a component | retro-ui-svelte changed a prop name, slot, or export from the shadcn-svelte version | Fix at the specific call site listed under "The concrete model"; do not skip the per-component `pnpm check` in Phase 1 step 4, which is designed to catch this immediately |
