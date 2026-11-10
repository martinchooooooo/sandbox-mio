<script lang="ts">
    import type { Snippet } from "svelte";

    // A boxed aside inside a page: something to try, a warning, a tip.
    let { title, children }: { title?: string; children: Snippet } = $props();
</script>

<aside class="my-3 rounded-md border-2 bg-accent/20 px-4 py-3 shadow-sm [&_p:last-child]:mb-0">
    {#if title}
        <p class="!mb-1 font-head text-sm uppercase tracking-wide">{title}</p>
    {/if}
    {@render children()}
</aside>
