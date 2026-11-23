<script lang="ts">
    import type { Snippet } from "svelte";
    import { GB } from "./catalogue";
    import { PAGE } from "./copy";
    import type { ModelDescriptor } from "./types";

    let { d, children }: { d: ModelDescriptor; children?: Snippet } = $props();
</script>

<dl class="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
    {@render children?.()}
    <dt class="text-muted-foreground">{PAGE.facts.params}</dt>
    <dd class="m-0">{d.paramsB}B</dd>
    {#if d.activeParamsB !== d.paramsB}
        <dt class="text-muted-foreground">{PAGE.facts.active}</dt>
        <dd class="m-0">{d.activeParamsB}B</dd>
    {/if}
    {#if d.llama}
        <dt class="text-muted-foreground">{PAGE.facts.file}</dt>
        <dd class="m-0 break-all">{d.llama.file}</dd>
    {/if}
    <dt class="text-muted-foreground">{PAGE.facts.context}</dt>
    <dd class="m-0">{d.contextWindow.toLocaleString()} tokens</dd>
    <dt class="text-muted-foreground">{PAGE.facts.memory}</dt>
    <dd class="m-0">{d.ramFloorBytes / GB} GB</dd>
</dl>
