<script lang="ts">
    import { onMount } from "svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Card from "$lib/components/ui/card";
    import AdvancedSection from "$lib/components/AdvancedSection.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { podmanRebootRequired } from "$lib/podman/setup.svelte";
    import { imageRun, probeImage, runImageDownload } from "$lib/onboarding/setup.svelte";

    let { ondone }: { ondone?: (done: boolean) => void } = $props();

    // The run lives in the shared store: the background setup may already be building the image
    // when this card mounts, and a second build from here would race it.
    const run = $derived(imageRun());
    const done = $derived(!run.probing && run.ready);

    $effect(() => ondone?.(done));

    onMount(() => {
        if (!run.busy) probeImage();
    });
</script>

<Card.Root class="gap-3 p-5" data-step="download-necessities" data-done={done}>
    <Card.Title>{m.welcome_download_title()}</Card.Title>
    <AdvancedSection label={m.welcome_technical_details_label()}>
        <p class="text-muted-foreground text-sm">
            {m.welcome_download_technical_details()}
        </p>
    </AdvancedSection>

    {#if run.probing}
        <p class="text-muted-foreground text-sm">{m.welcome_checking()}</p>
    {:else if done}
        <p class="text-sm font-medium">{m.welcome_download_already_done()}</p>
    {:else if run.probeErrorKind === "PodmanUnresponsive"}
        <!-- Named as a stall rather than a fault to fix: neither `setupPodman` nor `startPodman`
             helps a binary that never answers. -->
        <p class="text-destructive text-sm" data-error="podman-unresponsive">
            {m.welcome_podman_unresponsive_before()} <code>mio.log</code>{m.welcome_podman_unresponsive_after()}
        </p>
        <Button onclick={probeImage}>{m.welcome_try_again()}</Button>
    {:else if podmanRebootRequired()}
        <p class="text-destructive text-sm">
            {m.welcome_wsl_reboot_message()}
        </p>
        <Button onclick={runImageDownload}>{m.welcome_try_again()}</Button>
    {:else}
        <Button loading={run.busy} disabled={run.busy} onclick={runImageDownload}>
            {m.welcome_download_button()}
        </Button>
        {#if run.error || run.probeError}
            <p class="text-destructive text-sm">{run.error || run.probeError}</p>
        {/if}
    {/if}
</Card.Root>
