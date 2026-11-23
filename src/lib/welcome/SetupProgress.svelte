<script lang="ts">
    import { onMount } from "svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Card from "$lib/components/ui/card";
    import { Progress } from "$lib/components/ui/progress";
    import AdvancedSection from "$lib/components/AdvancedSection.svelte";
    import RuntimeSteps from "$lib/models/RuntimeSteps.svelte";
    import WakeUpStatus from "$lib/models/WakeUpStatus.svelte";
    import { modelsState } from "$lib/models/model.svelte";
    import MachineSteps from "$lib/machine/MachineSteps.svelte";
    import { ensureMachineReadiness, machineReadiness } from "$lib/machine/readiness.svelte";
    import type { ProviderId } from "$lib/machine/types";
    import { podmanRebootRequired } from "$lib/podman/setup.svelte";
    import {
        imageRun,
        machineRun,
        modelRun,
        probeImage,
        startBackgroundSetup,
        retryModelSetup,
        runImageDownload,
        retryFirstSandbox,
        sandboxRun,
        skipModelSetup,
    } from "$lib/onboarding/setup.svelte";

    let {
        provider = "podman" as ProviderId,
        showModel = true,
        onleave,
    }: {
        provider?: ProviderId;
        /** False on a replay, which starts no Model and no first sandbox. */
        showModel?: boolean;
        /** Leaves onboarding for the sandbox list while a failed job stays unfinished. */
        onleave?: () => void;
    } = $props();

    // All three jobs live in the shared store: the background run may already be working when
    // this page mounts, and a second run from here would race it.
    const machine = $derived(machineRun());
    const image = $derived(imageRun());
    const model = $derived(modelRun());
    const sandbox = $derived(sandboxRun());
    const wake = $derived(modelsState().wake);
    const status = $derived(machineReadiness(provider));

    const machineDone = $derived(!status.probing && status.readiness.ready);
    const imageDone = $derived(!image.probing && image.ready);

    onMount(() => {
        ensureMachineReadiness(provider);
        if (!image.busy) probeImage();
    });

    // A retry continues into the next job, as the first background run did.
    async function retryMachine(forceProbe: boolean) {
        if (forceProbe) await ensureMachineReadiness(provider, true);
        void startBackgroundSetup(provider);
    }

    // The raw backend message belongs in the "For those who know" disclosure, never the visible text.
    const machineDetail = $derived(machine.error || machine.rebootMessage || status.error);
    const imageDetail = $derived(image.error || image.probeError);
</script>

<!-- TODO(i18n): every string on this page. -->
<div class="grid grid-cols-1 gap-4" data-setup-progress>
    <Card.Root class="gap-3 p-5" data-step="create-vm" data-done={machineDone}>
        <Card.Title>A pretend computer for your sandboxes</Card.Title>
        <div class="flex flex-col gap-3" aria-live="polite">
            {#if status.probing}
                <p class="text-muted-foreground text-sm">Checking…</p>
            {:else if machineDone}
                <p class="text-sm font-medium">Ready ✓</p>
            {:else if machine.rebootMessage || podmanRebootRequired()}
                <p class="text-destructive text-sm">Your computer needs a restart to finish this. Restart it, then open Sandbox mio again.</p>
            {:else if status.errorKind === "PodmanUnresponsive"}
                <p class="text-destructive text-sm" data-error="podman-unresponsive">It started but never answered. Trying again may clear it.</p>
                <Button onclick={() => retryMachine(true)}>Try again</Button>
            {:else if machine.error || status.error}
                <p class="text-destructive text-sm">That did not go through.</p>
                <Button onclick={() => retryMachine(Boolean(status.error))}>Try again</Button>
            {:else}
                <p class="text-muted-foreground text-sm">
                    {machine.busy ? "Getting it ready. This takes a few minutes." : "Not started yet."}
                </p>
                <Button loading={machine.busy} disabled={machine.busy} onclick={() => retryMachine(false)}>Get it ready</Button>
            {/if}
        </div>
        <AdvancedSection label="For those who know">
            <p class="text-muted-foreground text-sm">
                A virtual machine with Podman (WSL2 on Windows). Sandboxes are containers inside it.
            </p>
            {#if machineDetail}<p class="text-xs text-foreground/80">{machineDetail}</p>{/if}
            {#if !machineDone}<MachineSteps steps={status.steps} events={machine.events} />{/if}
        </AdvancedSection>
    </Card.Root>

    <Card.Root class="gap-3 p-5" data-step="download-necessities" data-done={imageDone}>
        <Card.Title>Everything a sandbox starts with</Card.Title>
        <div class="flex flex-col gap-3" aria-live="polite">
            {#if image.probing}
                <p class="text-muted-foreground text-sm">Checking…</p>
            {:else if imageDone}
                <p class="text-sm font-medium">Ready ✓</p>
            {:else if image.probeErrorKind === "PodmanUnresponsive"}
                <p class="text-destructive text-sm" data-error="podman-unresponsive">It started but never answered. Trying again may clear it.</p>
                <Button onclick={() => startBackgroundSetup(provider)}>Try again</Button>
            {:else if image.error || image.probeError}
                <p class="text-destructive text-sm">That did not go through.</p>
                <Button onclick={() => startBackgroundSetup(provider)}>Try again</Button>
            {:else}
                <p class="text-muted-foreground text-sm">
                    {image.busy ? "Packing it. This takes a few minutes the first time." : "Not started yet."}
                </p>
                <Button loading={image.busy} disabled={image.busy} onclick={runImageDownload}>Pack it now</Button>
            {/if}
        </div>
        <AdvancedSection label="For those who know">
            <p class="text-muted-foreground text-sm">The base container image every sandbox is created from.</p>
            {#if imageDetail}<p class="text-xs text-foreground/80">{imageDetail}</p>{/if}
        </AdvancedSection>
    </Card.Root>

    {#if showModel}
    <Card.Root class="gap-3 p-5" data-step="model" data-done={model.ready}>
        <Card.Title>The Model</Card.Title>
        <div class="flex flex-col gap-3" aria-live="polite">
            {#if model.ready}
                <p class="text-sm font-medium">Ready ✓</p>
            {:else if model.skipped}
                <p class="text-muted-foreground text-sm">Skipped. Your first sandbox cannot chat until you add a Model later.</p>
            {:else if model.error}
                <p class="text-destructive text-sm">The Model did not come through. You can try again, or go on without it.</p>
                <div class="flex gap-2">
                    <Button onclick={retryModelSetup}>Try again</Button>
                    <Button variant="outline" onclick={skipModelSetup} data-model-skip>Go on without it</Button>
                </div>
            {:else if model.downloading}
                <p class="text-muted-foreground text-sm">Downloading… {model.downloadPct ?? 0}%</p>
                <Progress value={model.downloadPct ?? 0} aria-label="Model download" />
                <Button variant="outline" onclick={skipModelSetup} data-model-skip>Go on without it</Button>
            {:else if model.starting}
                <WakeUpStatus />
                <Button variant="outline" onclick={skipModelSetup} data-model-skip>Go on without it</Button>
            {:else}
                <p class="text-muted-foreground text-sm">Waiting its turn…</p>
            {/if}
        </div>
        <AdvancedSection label="For those who know">
            {#if model.choice?.llama}
                <p class="text-muted-foreground text-sm">{model.choice.label} ({model.choice.llama.file}) served by llama.cpp.</p>
            {/if}
            {#if model.error}<p class="text-xs text-foreground/80">{model.error}</p>{/if}
            {#if wake.steps.length > 0}<RuntimeSteps steps={wake.steps} events={wake.events} />{/if}
        </AdvancedSection>
    </Card.Root>

    <Card.Root class="gap-3 p-5" data-step="first-sandbox" data-done={sandbox.ready}>
        <Card.Title>Preparing your first sandbox</Card.Title>
        <div class="flex flex-col gap-3" aria-live="polite">
            {#if sandbox.ready}
                <p class="text-sm font-medium">Ready ✓</p>
            {:else if sandbox.error}
                <p class="text-destructive text-sm">Your first sandbox did not come together.</p>
                <div class="flex gap-2">
                    <Button onclick={retryFirstSandbox} data-first-sandbox-retry>Try again</Button>
                    {#if onleave}<Button variant="outline" onclick={onleave} data-first-sandbox-skip>Go on without it</Button>{/if}
                </div>
            {:else if sandbox.busy}
                <p class="text-muted-foreground text-sm">Putting it together. This takes a few minutes.</p>
            {:else}
                <p class="text-muted-foreground text-sm">Waiting its turn…</p>
            {/if}
        </div>
        <AdvancedSection label="For those who know">
            <p class="text-muted-foreground text-sm">
                A sandbox named hello-world{(sandbox.ready ? sandbox.installed : !model.skipped) ? " with the pi agent installed, pointed at the Model once that is ready" : " with no agent, as you went on without a Model"}.
            </p>
            {#if sandbox.busy && sandbox.step}<p class="text-xs text-foreground/80">{sandbox.step}</p>{/if}
            {#if sandbox.error}<p class="text-xs text-foreground/80">{sandbox.error}</p>{/if}
        </AdvancedSection>
    </Card.Root>
    {/if}
</div>
