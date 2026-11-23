<script lang="ts">
    import type { ModelScales } from "./describe";
    import ModelExplainer from "./ModelExplainer.svelte";
    import { PAGE } from "./copy";

    let { scales }: { scales: ModelScales } = $props();
</script>

<ul class="m-0 flex list-none flex-wrap gap-x-4 gap-y-1 p-0 text-xs">
    <li class="flex items-center">
        <span class="text-muted-foreground">{PAGE.size}:&nbsp;</span>
        <span class="font-medium">{scales.size}</span>
        <ModelExplainer key="size" />
    </li>
    {#if scales.speed}
        <li class="flex items-center">
            <span class="text-muted-foreground">{PAGE.speed}:&nbsp;</span>
            <span class="font-medium">{scales.speed}</span>
            <ModelExplainer key="speed" />
        </li>
    {/if}
    <li class="flex items-center">
        <span class="font-medium">{PAGE.download(scales.downloadGb)}</span>
        <ModelExplainer key="download" />
    </li>
    <li class="flex items-center">
        <span class="font-medium">{PAGE.memory(scales.memoryGb)}</span>
        <ModelExplainer key="memory" />
    </li>
</ul>
