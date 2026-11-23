<script lang="ts">
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { LoaderPinwheelIcon, RefreshIcon } from "@hugeicons/core-free-icons";
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Card from "$lib/components/ui/card";
    import AdvancedSection from "$lib/components/AdvancedSection.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { GB } from "./catalogue";
    import { roundGb } from "./recommend";
    import { modelsState, redetect } from "./model.svelte";
    import { PAGE } from "./copy";

    const st = modelsState();
    const specs = $derived(st.specs);
    const gb = (bytes: number) => bytes / GB;
</script>

<Card.Root class="gap-3 p-5" data-specs>
    <Card.Title class="mb-0">{PAGE.yourComputer}</Card.Title>
    {#if specs}
        <p class="m-0 text-sm text-muted-foreground">
            {PAGE.computer(roundGb(specs.totalRamBytes), roundGb(specs.freeRamBytes), specs.gpuNames.length > 0)}
        </p>
        <AdvancedSection label={PAGE.forThoseWhoKnow} data-computer-details>
            <div class="flex items-start justify-between gap-3">
                <ul class="m-0 list-none space-y-0.5 p-0 text-sm text-muted-foreground" data-specs-ready>
                    <li>{m.models_cpu_cores({ cores: specs.cpuCores })}</li>
                    <li>{m.models_ram({ free: gb(specs.freeRamBytes).toFixed(1), total: gb(specs.totalRamBytes).toFixed(1) })}</li>
                    <li>
                        {m.models_gpu()}: {specs.gpuNames.length > 0 ? specs.gpuNames.join(", ") : m.models_no_gpu()}
                        {#if specs.hasNpu}· {m.models_npu()}{/if}
                    </li>
                </ul>
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
            </div>
        </AdvancedSection>
    {:else}
        <div role="status" aria-live="polite" class="flex items-center gap-1.5 text-sm text-muted-foreground">
            <HugeiconsIcon icon={LoaderPinwheelIcon} class="size-3.5 animate-spin" />
            <span>{m.models_detecting()}</span>
        </div>
    {/if}
</Card.Root>
