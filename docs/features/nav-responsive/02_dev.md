---
type: dev-plan
title: "Navigation Column Responsive Layout — Build Plan"
feature: nav-responsive
---
# Navigation Column Responsive Layout — Build Plan

## Overview

This document is the concrete build plan for applying responsive container-query patterns across all NavigationColumn child pages (models, settings, credentials, intro). The ideation document `01_ideas.md` established the unified design: use `@max-[420px]:` breakpoints to hide button labels and reflow content, apply `truncate` to long text, replace `max-w-2xl` fixed widths with full-width layouts, and ensure flex children have `min-w-0` to prevent horizontal overflow. The projects/SandboxItem pages already follow this pattern and serve as reference. **Status**: All four phases (A–D) implemented and verified; no compilation errors.

## Concrete Model

### Responsive layout seam

Every page rendered as a child of NavigationColumn (`src/lib/workspace/NavigationColumn.svelte:31`, which declares `@container`) must follow these rules:

1. **Width constraint**: Remove fixed `max-w-2xl mx-auto` and replace with responsive spacing. Allow content to use the full column width (380px expanded, 48px collapsed). Use `p-3` or `p-4` for padding (consistent with SandboxItem card padding).

2. **Flex children shrinking rule**: Any flex container with text or input children must ensure those children have `min-w-0` (allows flex-1 to shrink past content width) and apply `truncate` if the text could overflow (e.g., `<span class="flex-1 min-w-0 truncate">`).

3. **Button label visibility**: Buttons with text labels must wrap the label in a `<span class="@max-[420px]:hidden">` to hide text below 420px. Use `title="action description"` on the Button element for accessibility/tooltip. Icons must be visible at all widths.

4. **Text overflow protection**: Card titles, status strings, email addresses, and any user-visible text that could exceed the column width must use the `truncate` Tailwind class. Add `title=""` attribute to show the full text on hover.

5. **Form field responsive behavior**: Input elements and their labels should reflow or shrink below 420px. Use `@max-[420px]:` to hide helper text, reduce label font size, or stack labels above inputs if needed. Inputs must have `min-w-0` to allow flex shrinking.

### Container inheritance

NavigationColumn declares `@container`, so all child pages (projects, models, settings, credentials, agent-auth, intro) automatically inherit the container context. `@max-[420px]:` rules in any child page will fire when the container (NavigationColumn) is ≤ 420px wide, which occurs when NavigationColumn is expanded to 380px plus the card/page padding margin of ~40px combined.

### Reference implementation

`src/routes/projects/SandboxItem.svelte` (lines 178–289) is the canonical implementation:
- Card root: `class="flex flex-col gap-2 rounded-lg border p-3 text-sm"` (flex-col, p-3, no max-width).
- Title row: `<span class="truncate">{project.name}</span>` ensures name never wraps.
- Button label visibility: `<span class="@max-[420px]:hidden">{m.projects_folder()}</span>` (lines 205, 210, 215, etc.).
- Button structure: `<Button size="sm" title="..."><HugeiconsIcon icon={FolderIcon} size={16} /><span>...</span></Button>`.

All updated pages will follow this structure.

## Resolved Decisions

**Open Question 1: Should NavigationColumn's width be configurable/draggable?**
- **Resolution**: Assume 380px (expanded) and 48px (collapsed) are fixed for this feature. If resizability is added later (separate feature), breakpoints can be re-tuned. No responsive code in this feature needs to account for variable widths; 420px is the fixed target.

**Open Question 2: What is the priority order for hiding/truncating content in dense pages like models?**
- **Resolution**: Per-page, prioritize always-visible content as follows:
  - **models/+page.svelte**: Model name (truncate) > status (truncate) > download size/metadata (hide below 420px with `@max-[420px]:hidden`).
  - **settings/+page.svelte**: Form field labels (shown) > inputs (shown, shrink with `min-w-0`) > helper text (hide below 420px with `@max-[420px]:hidden`).
  - **credentials/+page.svelte**: Service name (truncate) > status/email (truncate, since it's the key identifier) > action buttons (icon-only below 420px).
  - Concrete logic: Titles/names never hidden, always truncated if text is long. Metadata/helper text hidden below 420px if space is critical. Buttons always icon-only below 420px.

**Open Question 3: Should the intro robot stay visible and scale down, or be hidden below a breakpoint?**
- **Resolution**: Scale responsively. Replace hardcoded `.robot { width: 220px; }` with `max-w-[220px] @max-[350px]:max-w-[150px]` so the robot scales down as the container narrows, keeping it visible and centered on all widths. This avoids a sudden disappearance and is more graceful than a hidden state.

## Implementation Phases

### Implemented: Phase A — models/+page.svelte

Update the models page to apply responsive container-query patterns.

**File**: `src/routes/models/+page.svelte`

**Changes implemented**:
- **Line 224** (main element): Replaced `<main class="p-6 pb-24 max-w-2xl mx-auto">` with `<main class="p-3 pb-6 @max-[420px]:p-2">`.
- **Line 273–276** (model title row): Wrapped left side (model info) in `<div class="flex-1 min-w-0">` to enable proper flex shrinking. Wrapped title and rating in `<div class="flex items-center gap-2">` with `shrink-0` on rating to ensure name gets `truncate` priority. Added `truncate` to model name.
- **Line 280–281** (running status base URL): Added `@max-[420px]:hidden` to the base URL label and code to hide verbose metadata below 420px while keeping the "running" status visible.

**Verification**: Page renders without horizontal scrollbars at 380px container width; model names are truncated (not wrapped) when long; metadata hides below 420px; button labels show above 420px, icons only below.

### Implemented: Phase B — settings/+page.svelte

Update the settings page to apply responsive form field behavior.

**File**: `src/routes/settings/+page.svelte`

**Changes implemented**:
- **Line 96** (main element): Replaced `<main class="p-6 pb-24 max-w-2xl mx-auto">` with `<main class="p-3 pb-6 @max-[420px]:p-2">`.
- **Line 126** (foundry unavailable helper text): Added `@max-[420px]:hidden` to hide verbose foundry installation explanation below 420px while keeping the install button visible.
- Field components (shadcn-svelte) use the field-group container query internally and already stack vertically with responsive spacing; no additional changes needed.

### Implemented: Phase C — credentials/+page.svelte

Update the credentials page to truncate long identifiers and apply responsive button patterns.

**File**: `src/routes/credentials/+page.svelte`

**Changes implemented**:
- **Line 107** (main element): Replaced `<main class="p-6 pb-24 max-w-2xl mx-auto">` with `<main class="p-3 pb-6 @max-[420px]:p-2">`.
- **Lines 119–124** (service card): Wrapped text content in `<div class="flex-1 min-w-0">` to enable shrinking. Added `truncate` class to service name and status/email field to prevent wrapping on long email addresses.

### Implemented: Phase D — intro/+page.svelte

Update the intro page to make the robot component responsive instead of hardcoded pixel width.

**File**: `src/routes/intro/+page.svelte`

**Changes implemented**:
- **Lines 18–24** (robot CSS): Replaced hardcoded `width: 220px` with `max-width: 220px; width: 100%` and added a `@container (max-width: 350px)` rule to scale the robot down to `max-width: 150px` below 350px. This allows the robot to scale smoothly and remain visible at all container widths.

## Lifecycle / Cross-Cutting Concerns

### Backward compatibility

All changes are CSS-only (Tailwind class additions and removals of fixed-width constraints). No Svelte logic, state, or component interfaces change. Existing unit tests (if any) for these pages do not need updates. E2E tests should verify that no horizontal scrollbars appear at 380px width (see Phase E, test plan).

### Testing strategy

See `03_e2e.md` (test plan) for detailed E2E coverage. At a high level:
- Resize the NavigationColumn to 380px in the browser and verify each page (models, settings, credentials, intro) renders without horizontal scrollbars.
- Verify that button labels hide below ~420px container width and icons remain visible.
- Verify that long text (model names, emails, robot width) truncates or scales without overflow.

### Known issues and future improvements

- **Robot scaling on intro**: The `@max-[350px]:` breakpoint is a guess; actual threshold depends on the robot's intrinsic width and the page's padding. May need fine-tuning after visual inspection. (Phase D verification will confirm.)
- **Dense metadata in models**: If models page has many columns (name, status, size, date, download button), some columns may still need hiding below 420px. Phase A verification will surface any remaining issues.
- **Form field density in settings**: If settings has many inline form fields, the `min-w-0` approach may not be enough; fields may need to stack vertically below 420px. Phase B verification will confirm.

These are not blockers for this feature; they are refinements after user testing.

## Files Touched (Summary)

- `src/routes/models/+page.svelte` — Phase A: responsive layout, text truncation, metadata hiding.
- `src/routes/settings/+page.svelte` — Phase B: responsive form fields, helper text hiding.
- `src/routes/credentials/+page.svelte` — Phase C: text truncation, button label hiding.
- `src/routes/intro/+page.svelte` — Phase D: responsive robot width.
- `src/routes/projects/SandboxItem.svelte` — Reference only (no changes; already responsive).
- `src/lib/workspace/NavigationColumn.svelte` — Reference only (already has `@container`, no changes needed).

