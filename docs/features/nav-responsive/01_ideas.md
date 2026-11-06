---
type: ideas
title: "Navigation Column Responsive Layout"
feature: nav-responsive
---
# Navigation Column Responsive Layout

## Overview

This document covers a holistic responsive-layout redesign for the NavigationColumn (`src/lib/workspace/NavigationColumn.svelte`), which switches between a 48px collapsed rail and 380px expanded sidebar and renders arbitrary routed pages as children (projects, models, settings, credentials, agent-auth, intro). Currently, only the projects/SandboxItem pages follow responsive container-query patterns to fit content into 380px; other routes (models, settings, credentials, intro) use fixed `max-w-2xl` layouts with no responsive fallbacks, causing button text to overflow, titles to wrap/disappear, and content to force horizontal scrollbars when the column is 380px or narrower. The feature establishes a unified, column-wide responsive design system — button behavior (icon-only below threshold), text truncation rules, and breakpoint thresholds — so all pages adapt gracefully to the constrained width without horizontal scroll.

## Current State

**NavigationColumn container** (`src/lib/workspace/NavigationColumn.svelte:31`): Declares `@container` and switches between `w-12 shrink-0` (collapsed rail, 48px) and `w-[380px] shrink-0` (expanded). Children inherit the container context.

**Responsive pattern in projects**:
- `src/routes/projects/+page.svelte:170`: Sets `@max-[420px]:max-w-none @max-[420px]:mx-0` to remove fixed max-width below 420px.
- `src/routes/projects/SandboxItem.svelte:178–289`: Card layout with `@max-[420px]:hidden` on button text labels (lines 205, 210, 215, 220, 227, 233, 239, 243, 249); buttons show icons only below 420px. Text like project name uses `truncate` to prevent overflow.
- Comment in SandboxItem (lines 194–200) documents the 420px threshold as derived from `NAVIGATION_PANEL_EXPANDED_W = 380` plus padding.

**Non-responsive pages** (no container queries, overflow risk):
- `src/routes/models/+page.svelte:224, 270–336`: `max-w-2xl mx-auto` with `flex items-center gap-3` model rows; no `@max-[420px]:` breakpoints, no `truncate` on model names or status.
- `src/routes/settings/+page.svelte:96, 105–168`: `max-w-2xl mx-auto` form with flex gap-2/gap-3; no `min-w-0` on inputs to allow shrink-to-content, no `@max-[420px]:` fallbacks.
- `src/routes/credentials/+page.svelte:107, 112–140`: `max-w-2xl mx-auto` service cards with `flex items-center justify-between gap-2`; service status text (line 122, e.g., user email) not `truncate`d, will overflow < 380px.
- `src/routes/intro/+page.svelte:19–24`: Robot component with hardcoded `.robot { width: 220px; }`, no responsive fallback.

**Button primitives** (`src/lib/components/ui/button/button.svelte:17–26`):
- `size="icon-xs"`: 24×24px, `size-3` SVG.
- `size="icon-sm"`: 28×28px, `size-3.5` SVG.
- `size="sm"`: 28px height, gap-1, shows text + icon.
- Sized buttons already tested in SandboxItem; icon sizes (`size={16}`) via HugeiconsIcon are production-ready.

**Tailwind container queries**: Built-in to Tailwind v4 (via `@tailwindcss/vite`); `@container`, `@max-[Npx]:` etc. are available without extra config.

## Proposed Model

### Unified responsive breakpoint

Establish **380px as the hard constraint** (NavigationColumn's expanded width). Below this, pages enter **narrow/compact mode**, which hides non-essential button labels and text, removes fixed max-widths, and applies `truncate` to long content. Use the `@max-[380px]:` container query breakpoint consistently across all pages, with an optional `@max-[420px]:` alias for pages that need extra padding/margin breathing room.

**Rationale**: NavigationColumn is the container; its 380px width is the seam every child page must respect. No page gets a separate viewport media query (the window is much wider; the constraint is the column, not the screen). The 420px threshold (found in SandboxItem comments) accounts for card padding/margins and is already proven; stick with it unless a page needs tighter packing.

### Button visibility rule

Buttons follow **progressive disclosure**: above 420px, show text labels + icons (e.g., "Folder", "Deploy"). Below 420px, hide text and show icons only using `@max-[420px]:hidden` on the label span. Icons must be legible at `size={16}` (current standard); if a button's intent is unclear as an icon, use a tooltip/title attribute instead of relying on label text.

**Rationale**: Matches the proven SandboxItem pattern. Labels add clarity; icons are compact. The threshold 420px allows cards to remain readable (e.g., project name + status visible) while dropping verbose action labels.

### Text overflow rule

All text fields that could exceed available width must use `truncate` (Tailwind shorthand for `overflow: hidden; text-overflow: ellipsis; white-space: nowrap`). This applies to:
- Card titles/names (e.g., project name, model name, service name).
- Long email addresses, file paths, status strings.
- Form field labels and helper text in narrow columns.

**Rationale**: Prevents text from wrapping unexpectedly and causing cards to grow tall or push siblings down. A truncated title with an ellipsis is better UX than a title broken across multiple lines or an invisible title.

### No horizontal scrollbars

Remove all instances of fixed-width containers that force horizontal overflow. Replace `max-w-2xl mx-auto` with `w-full` (or remove the constraint entirely) and use `@max-[420px]:` to reflow margins/padding as needed. Flex children that contain text or inputs must have `min-w-0` to allow shrinking past their content width.

**Rationale**: Horizontal scroll is a last-resort; it breaks the narrow-column layout assumption. Narrow-column design always reflows or hides content; it does not scroll sideways.

### Per-page implementation seams

- **projects/SandboxItem.svelte** (already responsive): No change needed; documents the pattern for others to follow.
- **models/+page.svelte** (lines 224–336): Replace `max-w-2xl mx-auto` with responsive layout; add `@max-[420px]:hidden` to status/version text or use `truncate`; ensure grid/flex rows have `min-w-0` on text children.
- **settings/+page.svelte** (lines 96–168): Replace `max-w-2xl mx-auto`; add `@max-[420px]:` to hide helper text or labels; inputs must have `min-w-0` to shrink.
- **credentials/+page.svelte** (lines 107–140): Replace `max-w-2xl mx-auto`; apply `truncate` to service status text (email/URL); ensure card layout has proper flex shrinking.
- **intro/+page.svelte** (lines 19–24): Replace hardcoded `.robot { width: 220px; }` with responsive `max-w-[220px]` and `@max-[300px]:max-w-[150px]` or similar.

## Technology Choices with Tradeoffs

### Container Queries (`@container`, `@max-[Npx]:`) vs. Viewport Media Queries

**Chosen**: Container queries (`@max-[380px]:`, `@max-[420px]:` via Tailwind v4).

**Alternative: Viewport media queries** (`@max-sm:`, `@max-md:`, etc.).
- **Rejected**: NavigationColumn is 380px wide; the viewport is much wider. A viewport media query would never fire when the column is narrow but the window is full-width. Container queries directly inspect the column's actual width, so they fire reliably.

**Alternative: CSS Grid with implicit responsive columns** (e.g., `grid-cols-auto-fit`).
- **Rejected**: Useful for galleries of equal-width items but not for text-heavy pages (models, settings, credentials, projects) where content must reflow, not wrap to a second column. Navigation pages are mostly vertical lists; grid does not help here.

### Truncate vs. Wrap vs. Reflow

**Chosen**: Truncate (`truncate` class) for titles/names; conditional hide (`@max-[420px]:hidden`) for long-form text or button labels.

**Alternative: Allow text to wrap naturally**.
- **Rejected**: Causes unexpected card growth and visual clutter. A wrapped 3-line title is worse than a 1-line truncated title with an ellipsis. A title *should* fit on one line; if space doesn't allow, truncation signals "there's more, see the tooltip" rather than breaking layout.

**Alternative: Reflow text layout vertically below 420px** (e.g., show model name and status on separate rows).
- **Reason kept as secondary**: May be useful for very dense content (e.g., credentials card with long email), but truncate + `title=""` attribute is simpler and more consistent.

### Button Icon Size

**Chosen**: `size={16}` for HugeiconsIcon in button labels (current SandboxItem standard), `size="icon-sm"` (28×28px button) or `size="sm"` (28px height with text).

**Alternative: Smaller icons** (`size={12}` or `size={14}`) to save space.
- **Rejected**: Icon-only buttons at 28×28px are already compact; 14px icons inside 28px buttons are legible but start feeling cramped. 16px is the proven standard in SandboxItem.

**Alternative: Toggle to a completely icon-only button set** (no text ever) at 420px.
- **Reason kept as secondary**: Could save space further, but icon-only requires very clear iconography. The `@max-[420px]:hidden` approach (hide text, keep icon + title attribute) is more forgiving.

## Validation

**Proof of pattern**: The projects/SandboxItem page (deployed, in use) already demonstrates that:
- `@max-[420px]:hidden` on button text labels works without breaking icon visibility.
- Card titles with `truncate` + `title=""` remain readable.
- No horizontal scrollbars appear when the column is 380px.
- Icons at `size={16}` are legible inside 28px buttons.

**Outcome**: The pattern is production-proven for SandboxItem; extending it to other pages is a replication task, not a new experiment.

## Open Questions

1. **Should we make NavigationColumn's width configurable/draggable?** Currently hardcoded to 380px expanded. If resizable, the responsive breakpoints (currently tuned to 380px/420px) may need adjustment. For now, assume 380px/420px are fixed; if resizability is added later, breakpoints can be re-tuned. (Deferred to a separate resizability feature.)

2. **What is the priority order for hiding/truncating content in pages that are very dense?** For example, the models page lists model names, status, download size, and other metadata. Should we hide download size below 420px, or truncate model name? This depends on UX priority (what's the primary action — viewing the list or managing metadata?). The build plan (02_dev.md) will resolve this per page.

3. **Should the intro robot stay visible and scale down, or be hidden below a breakpoint?** A 220px hardcoded width is tight but may be acceptable at 380px. The build plan will decide whether to scale it responsively or hide it below a threshold.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Content still scrolls horizontally even after adding `@max-[420px]:` rules | Flex/grid child is wider than its container and lacks `min-w-0`. Flex children default `min-w: auto`, which prevents shrinking past content width. | Add `min-w-0` to the flex child (e.g., `<div class="flex-1 min-w-0"><span class="truncate">long text</span></div>`). |
| Button text label hides at `@max-[420px]:` but button icon is not visible or looks strange | Icon size too large for the button, or icon missing `title=""` attribute for accessibility. | Use `size={16}` for HugeiconsIcon; add `title="action description"` to the Button so hover/a11y shows intent. |
| Container query breakpoint `@max-[420px]:` never fires even though column is 380px | Page is not a child of the `@container` element (NavigationColumn). Verify parent has `@container` class. | Add `@container` to NavigationColumn and ensure all child pages inherit it. (Already in place.) |
| Text is truncated but the ellipsis is not visible (text color = background color) | Truncate works, but contrast issue hides the ellipsis. | Check Tailwind text/background color utilities; use `text-foreground` or `text-muted-foreground` per theme. |
| Form inputs shrink unexpectedly below 420px and become unusable | Input has a hardcoded min-width (e.g., `min-w-[200px]`) that conflicts with flex shrinking. | Remove hardcoded min-width or replace with a responsive `@max-[420px]:min-w-0` override. |
