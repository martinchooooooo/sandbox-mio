<script lang="ts">
    import type { Snippet } from "svelte";
    import { setActLabel } from "./context.svelte";

    // Groups consecutive pages under one label: the pill above each title and a gap in the dots.
    let { label, children }: { label: string; children: Snippet } = $props();

    setActLabel(() => label);
</script>

{@render children()}
