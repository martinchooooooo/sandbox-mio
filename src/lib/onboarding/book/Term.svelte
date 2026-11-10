<script lang="ts">
    import type { Snippet } from "svelte";

    // A key term, marked with a highlighter stroke the first time a page introduces it.
    let { children }: { children: Snippet } = $props();
</script>

<b class="bg-[linear-gradient(transparent_60%,var(--muted)_60%)] font-bold">{@render children()}</b>
