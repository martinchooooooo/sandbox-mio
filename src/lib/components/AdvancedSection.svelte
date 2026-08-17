<script lang="ts">
    import * as Collapsible from "$lib/components/ui/collapsible";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { ArrowRight01Icon } from "@hugeicons/core-free-icons";
    import { m } from "$lib/paraglide/messages.js";
    import type { Snippet } from "svelte";

    // Shared disclosure convention (docs/features/desktop-app-feel/01_ideas.md, "Shared 'Advanced'
    // disclosure convention"): closed by default on every page visit, no persisted open state — a
    // single seam for the trigger's chevron/typography instead of four bespoke implementations.
    // Unlisted props (layout classes, a `data-*` e2e hook needed when a page has more than one
    // instance and its page object selects by attribute rather than the — still English-only —
    // trigger label) pass through to the outer root, same as any other block-level wrapper.
    let {
        label = m.settings_advanced(),
        children,
        ...restProps
    }: {
        label?: string;
        children?: Snippet;
        [key: string]: unknown;
    } = $props();

    let open = $state(false);
</script>

<Collapsible.Root {open} onOpenChange={(o) => (open = o)} {...restProps}>
    <Collapsible.Trigger class="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground">
        <HugeiconsIcon icon={ArrowRight01Icon} size={14} class={open ? "rotate-90" : ""} />
        {label}
    </Collapsible.Trigger>
    <Collapsible.Content class="pt-2">
        {@render children?.()}
    </Collapsible.Content>
</Collapsible.Root>
