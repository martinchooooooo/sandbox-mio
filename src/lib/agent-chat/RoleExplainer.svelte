<script lang="ts">
    import InfoIcon from "@lucide/svelte/icons/info";
    import * as HoverCard from "$lib/components/ui/hover-card/index.js";
    import { Text } from "$lib/components/ui/text/index.js";
    import type { Role } from "./anatomy";
    import { ROLE_COPY } from "./anatomy-copy";

    let { role }: { role: Role } = $props();
    const copy = $derived(ROLE_COPY[role]);
    const id = $props.id();

    // Hover and keyboard focus show the card; a click pins it open, which is the only way a touch
    // pointer can, because the primitive ignores touch hover.
    let open = $state(false);
    let pinned = false;
    let trigger = $state<HTMLButtonElement>();
</script>

<!-- The card is portalled out of reach of assistive tech, so the button carries the body as its description. -->
<span id="{id}-desc" class="sr-only">{copy.body}</span>
<!-- A pinned card re-opens itself, because the primitive closes on pointer leave. -->
<HoverCard.Root
    bind:open
    onOpenChange={(next: boolean) => {
        if (!next && pinned) open = true;
    }}
>
    <HoverCard.Trigger>
        {#snippet child({ props })}
            <!-- TODO(i18n): the label -->
            <button
                {...props}
                bind:this={trigger}
                type="button"
                class="inline-flex size-6 cursor-pointer items-center justify-center rounded-full text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
                aria-label="{copy.title}: what does this mean?"
                aria-describedby="{id}-desc"
                data-anatomy-info={role}
                onclick={() => {
                    pinned = !pinned;
                    open = pinned;
                }}
            >
                <InfoIcon class="size-3.5" />
            </button>
        {/snippet}
    </HoverCard.Trigger>
    <HoverCard.Content
        side="bottom"
        align="start"
        class="flex flex-col gap-1"
        onEscapeKeydown={() => (pinned = false)}
        onInteractOutside={(e) => {
            // The trigger's own click toggles the pin; closing here as well would undo it.
            if (!trigger?.contains(e.target as Node)) pinned = false;
        }}
        data-anatomy-explainer
        data-explainer-role={role}
    >
        <Text as="p" class="font-head text-lg">{copy.title}</Text>
        <p class="m-0 text-sm text-muted-foreground">{copy.body}</p>
    </HoverCard.Content>
</HoverCard.Root>
