<script lang="ts">
    import { Text } from "$lib/components/ui/text";
    import ModelCard from "./ModelCard.svelte";
    import { currentEntry, entries, infoOf } from "./model.svelte";
    import { PAGE, groupTitle } from "./copy";
    import type { FitGroup } from "./types";

    const GROUPS: FitGroup[] = ["runs-well", "runs-slowly", "needs-more"];

    const others = $derived.by(() => {
        const current = currentEntry();
        return entries()
            .filter((d) => d.id !== current?.id)
            .flatMap((d) => {
                const info = infoOf(d);
                return info ? [{ d, info }] : [];
            });
    });
</script>

<section class="flex flex-col gap-4">
    <Text as="h2">{PAGE.otherModels}</Text>
    {#each GROUPS as group (group)}
        {@const cards = others.filter((c) => c.info.fit.group === group)}
        {#if cards.length > 0}
            <div class="flex flex-col gap-3" data-fit-group={group}>
                <Text as="h3" class="text-muted-foreground">{groupTitle(group)}</Text>
                {#each cards as { d, info } (d.id)}
                    <ModelCard {d} {info} />
                {/each}
            </div>
        {/if}
    {/each}
</section>
