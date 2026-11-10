<script lang="ts">
    import type { Snippet } from "svelte";

    // A two-column list of terms and meanings; fill it with `GlossaryItem`s.
    let { children }: { children: Snippet } = $props();
</script>

<dl class="m-0 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-base">
    {@render children()}
</dl>
