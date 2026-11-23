<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Item from "$lib/components/ui/item";
    import AdvancedSection from "$lib/components/AdvancedSection.svelte";
    import DownloadProgress from "./DownloadProgress.svelte";
    import ModelDetails from "./ModelDetails.svelte";
    import ModelFacts from "./ModelFacts.svelte";
    import { downloadAndWake, downloadModel, isDownloaded, modelsState, pick, type ModelInfo } from "./model.svelte";
    import { PAGE } from "./copy";
    import type { ModelDescriptor } from "./types";

    let { d, info }: { d: ModelDescriptor; info: ModelInfo } = $props();

    const st = modelsState();
    const pct = $derived(st.downloadPct[d.id]);
    const needsMore = $derived(info.fit.group === "needs-more");
</script>

<Item.Root variant="outline" class="items-start" data-model-id={d.id} data-fit={info.fit.verdict}>
    <Item.Content class="min-w-0 flex-1 basis-80">
        <ModelDetails {d} {info} isPick={pick()?.id === d.id} heading="h4" />
    </Item.Content>
    {#if !needsMore}
        <Item.Actions>
            <Button size="sm" data-start disabled={st.busy !== null} onclick={() => downloadAndWake(d)}>{PAGE.useThis}</Button>
        </Item.Actions>
    {/if}
    <Item.Footer class="flex-col items-stretch">
        {#if pct !== undefined}<DownloadProgress {pct} />{/if}
        {#if st.error && !st.busy && st.jobModelId === d.id}<span class="text-xs text-destructive">{PAGE.failed}</span>{/if}
        {#if isDownloaded(d)}<span class="text-xs text-emerald-600 dark:text-emerald-400" data-downloaded>{PAGE.downloaded}</span>{/if}
        <AdvancedSection label={PAGE.forThoseWhoKnow}>
            <div class="flex flex-col gap-2">
                <ModelFacts {d} />
                {#if needsMore && info.fit.verdict !== "engine" && d.llama && !isDownloaded(d)}
                    <Button variant="link" size="sm" class="self-start px-0" disabled={st.busy !== null} onclick={() => downloadModel(d)}>
                        {PAGE.downloadAnyway}
                    </Button>
                {/if}
            </div>
        </AdvancedSection>
    </Item.Footer>
</Item.Root>
