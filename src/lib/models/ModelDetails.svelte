<script lang="ts">
    import { Text } from "$lib/components/ui/text";
    import MakerChip from "./MakerChip.svelte";
    import ModelAvatar from "./ModelAvatar.svelte";
    import ModelExplainer from "./ModelExplainer.svelte";
    import ModelScales from "./ModelScales.svelte";
    import { FIT_LABELS, GOOD_FOR_LABELS, MODEL_SUMMARIES, PAGE, fitSentence } from "./copy";
    import type { ModelInfo } from "./model.svelte";
    import type { ModelDescriptor } from "./types";

    let {
        d,
        info,
        isPick,
        heading = "h3",
    }: { d: ModelDescriptor; info: ModelInfo; isPick: boolean; heading?: "h3" | "h4" } = $props();

    const { fit } = $derived(info);
    const tone = $derived(
        fit.verdict === "fits"
            ? "text-emerald-600 dark:text-emerald-400"
            : fit.verdict === "tight"
              ? "text-amber-600 dark:text-amber-400"
              : "text-destructive",
    );
</script>

<div class="flex items-start gap-3">
    <ModelAvatar />
    <div class="flex min-w-0 flex-1 flex-col gap-2">
        <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
            <Text as={heading} class="truncate">{d.label}</Text>
            <MakerChip maker={d.maker} />
            {#if isPick}
                <span class="rounded-md border-2 border-border bg-secondary px-1.5 text-xs" data-pick>{PAGE.pickBadge}</span>
                <ModelExplainer key="pick" />
            {/if}
        </div>
        <p class="m-0 text-sm text-muted-foreground" data-summary>{MODEL_SUMMARIES[d.id] ?? ""}</p>
        {#if d.goodFor.length > 0}
            <ul class="m-0 flex list-none flex-wrap gap-1.5 p-0">
                {#each d.goodFor as tag (tag)}
                    <li class="rounded-md border border-border px-1.5 text-xs">{GOOD_FOR_LABELS[tag]}</li>
                {/each}
            </ul>
        {/if}
        <ModelScales scales={info.scales} />
        <p class="m-0 flex flex-wrap items-center gap-x-2 text-xs">
            <span class={["font-medium", tone]} data-rating>{FIT_LABELS[fit.verdict]}</span>
            {#if fit.verdict !== "fits"}<span class="text-muted-foreground">{fitSentence(fit)}</span>{/if}
            {#if fit.group === "needs-more"}<ModelExplainer key="cantRun" />{/if}
        </p>
    </div>
</div>
