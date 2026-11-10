<script lang="ts">
    import type { Snippet } from "svelte";

    let { term, children }: { term: string; children: Snippet } = $props();
</script>

<dt class="font-bold">{term}</dt>
<dd class="m-0">{@render children()}</dd>
