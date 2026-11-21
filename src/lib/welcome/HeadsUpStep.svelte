<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Text } from "$lib/components/ui/text";
    import AdvancedSection from "$lib/components/AdvancedSection.svelte";
    import { modelOffer, specsProbed } from "$lib/onboarding/setup.svelte";

    let {
        onconfirm,
        ondecline,
    }: {
        onconfirm: () => void;
        /** Goes on without a Model; never blocks onboarding. */
        ondecline: () => void;
    } = $props();

    const offer = $derived(modelOffer());
    // The low-RAM consent depends on the probe, so confirming waits for it.
    const checking = $derived(!specsProbed());
    const sizeGb = $derived(offer?.descriptor.llama ? (offer.descriptor.llama.sizeBytes / 1024 ** 3).toFixed(1) : null);
</script>

<!-- The one place outside the tour that says "Brain", introduced as "Brain (AKA Model)".
     TODO(i18n): every string on this screen. -->
<div class="max-w-lg space-y-5 text-center" data-welcome-heads-up data-low-ram={offer?.lowRam ? "" : undefined}>
    <Text as="h1">Let's get your first Brain running</Text>
    {#if offer && offer.lowRam}
        <p class="text-muted-foreground">
            Your computer is a little short on memory for a Brain (AKA Model) this size. A small one can still work,
            just more slowly. It is a {sizeGb} GB download. You can read the tour while it arrives.
        </p>
        <div class="flex flex-col items-center gap-2">
            <Button onclick={onconfirm} data-welcome-begin data-welcome-try-anyway>Try anyway</Button>
            <Button variant="link" size="sm" onclick={ondecline} data-welcome-decline>Set up without a Model</Button>
        </div>
    {:else}
        <p class="text-muted-foreground">
            Sandbox mio will prepare a computer-brain (Brain, AKA Model) and a safe place to run it.
            {#if sizeGb}It is a {sizeGb} GB download.{/if}
            You can read the tour while it happens.
        </p>
        {#if checking}<p class="text-muted-foreground text-sm" aria-live="polite" data-welcome-checking>Checking your computer…</p>{/if}
        <div class="flex flex-col items-center gap-2">
            <Button onclick={onconfirm} disabled={checking} data-welcome-begin>Start the tour and get set up</Button>
            <Button variant="link" size="sm" onclick={ondecline} data-welcome-decline>Set up without a Model</Button>
        </div>
    {/if}
    <AdvancedSection label="For those who know">
        <p class="text-left text-sm text-muted-foreground">
            {#if offer?.descriptor.llama}
                Model: {offer.descriptor.label} ({offer.descriptor.llama.file}), served by llama.cpp on this computer.
            {/if}
            Sandboxes run in Podman containers inside a virtual machine, which is set up at the same time.
        </p>
    </AdvancedSection>
</div>
