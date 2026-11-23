<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { LoaderPinwheelIcon, RefreshIcon } from "@hugeicons/core-free-icons";
    import * as Card from "$lib/components/ui/card";
    import { m } from "$lib/paraglide/messages.js";
    import { roundGb } from "./recommend";
    import { modelsState, redetect } from "./model.svelte";
    import { PAGE } from "./copy";

    const st = modelsState();
    const specs = $derived(st.specs);
    // Only a failed re-detect has no job Model; a failed download or wake is shown on its card.
    const redetectFailed = $derived(st.error !== "" && !st.busy && st.jobModelId === null && specs !== null);
</script>

<Card.Root
    class="gap-3 p-5"
    data-specs
    data-total-ram={specs?.totalRamBytes}
    data-free-ram={specs?.freeRamBytes}
    data-cpu-cores={specs?.cpuCores}
>
    <div class="flex items-center justify-between gap-3">
        <Card.Title class="mb-0">{PAGE.yourComputer}</Card.Title>
        {#if specs}
            <Button
                size="icon"
                variant="ghost"
                data-redetect
                loading={st.busy === "redetect"}
                disabled={st.busy !== null}
                onclick={redetect}
                title={m.models_redetect()}
                aria-label={m.models_redetect()}
            >
                <HugeiconsIcon icon={RefreshIcon} class="size-3.5" />
            </Button>
        {/if}
    </div>
    {#if specs}
        <p class="m-0 text-sm text-muted-foreground" data-specs-ready>
            {PAGE.computer(roundGb(specs.totalRamBytes), roundGb(specs.freeRamBytes), specs.gpuNames.length > 0)}
        </p>
        {#if redetectFailed}<p class="m-0 text-xs text-destructive" data-redetect-error>{st.error}</p>{/if}
    {:else}
        <div role="status" aria-live="polite" class="flex items-center gap-1.5 text-sm text-muted-foreground">
            <HugeiconsIcon icon={LoaderPinwheelIcon} class="size-3.5 animate-spin" />
            <span>{m.models_detecting()}</span>
        </div>
    {/if}
</Card.Root>
