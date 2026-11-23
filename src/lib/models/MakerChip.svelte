<script lang="ts">
    import { MAKERS } from "$lib/onboarding/chapters/agents-and-sandboxes/copy";

    let { maker }: { maker: string | null } = $props();
    const look = $derived(MAKERS.find((x) => x.name === maker));
</script>

{#if look}
    <span class="inline-flex items-center gap-1.5 text-xs text-muted-foreground" data-maker={look.name}>
        <span class="size-3 rounded-full border border-border" style:background-color={look.fill}></span>
        {look.name}
    </span>
{/if}
