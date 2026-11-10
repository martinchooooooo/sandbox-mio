<script lang="ts">
    import type { Snippet } from "svelte";
    import { setChapterLabel } from "./context.svelte";

    // Names the chapter its pages belong to; the book shows it in its header strip.
    let { title, children }: { title: string; children: Snippet } = $props();

    setChapterLabel(() => title);
</script>

{@render children()}
