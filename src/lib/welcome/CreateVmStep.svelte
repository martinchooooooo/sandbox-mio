<script lang="ts">
    import { onMount } from "svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import { parseError } from "$lib/errors";
    import MachineSteps from "$lib/machine/MachineSteps.svelte";
    import { fetchMachineReadiness, fetchMachineSetupSteps, prepareMachine } from "$lib/machine/readiness";
    import type { MachineReadiness, MachineSetupEvent, ProviderId, SetupStep } from "$lib/machine/types";

    // `provider` has no picker yet — Phase J (docs/features/microsandbox-provider/02_dev.md) adds
    // the frontend selector that will pass something other than the default here.
    let { provider = "podman" as ProviderId, ondone }: {
        provider?: ProviderId;
        ondone?: (done: boolean) => void;
    } = $props();

    let probing = $state(true);
    let probeError = $state("");
    let readiness = $state<MachineReadiness>({ ready: false, needsSetup: false });
    let steps = $state<SetupStep[]>([]);
    let events = $state<MachineSetupEvent[]>([]);
    let busy = $state(false);
    let error = $state("");
    let rebootMessage = $state("");

    // The probe runs in both variants, so a user who already has a machine sees the completed state
    // on first run too, not just on replay.
    const done = $derived(!probing && readiness.ready);

    $effect(() => ondone?.(done));

    async function probe() {
        try {
            [steps, readiness] = await Promise.all([
                fetchMachineSetupSteps(provider),
                fetchMachineReadiness(provider),
            ]);
            probeError = "";
        } catch (e) {
            probeError = parseError(e).message;
        } finally {
            probing = false;
        }
    }

    onMount(probe);

    async function run() {
        busy = true;
        error = "";
        rebootMessage = "";
        events = [];
        const result = await prepareMachine(provider, (e) => {
            events = [...events, e];
            if (e.kind === "rebootRequired") rebootMessage = e.message;
        });
        if (result.ok) {
            await probe();
        } else if (!result.rebootRequired) {
            error = result.message;
        }
        busy = false;
    }

    function actionLabel(): string {
        // TODO(i18n)
        if (provider === "microsandbox") return "Set up microsandbox";
        return readiness.needsSetup ? "Create virtual machine" : "Start virtual machine";
    }
</script>

<div class="border-2 rounded-md shadow-md p-5 space-y-3" data-step="create-vm" data-done={done}>
    <!-- TODO(i18n) -->
    <h3 class="text-lg font-medium">Create virtual machine</h3>
    <!-- TODO(i18n) -->
    <p class="text-muted-foreground text-sm">
        Sandboxes run inside a virtual machine. This sets it up on your computer.
    </p>

    {#if probing}
        <!-- TODO(i18n) -->
        <p class="text-muted-foreground text-sm">Checking…</p>
    {:else if probeError}
        <p class="text-destructive text-sm">{probeError}</p>
    {:else if done}
        <!-- TODO(i18n) -->
        <p class="text-sm font-medium">Already set up ✓</p>
    {:else if rebootMessage}
        <p class="text-destructive text-sm">{rebootMessage}</p>
        <!-- TODO(i18n) -->
        <Button onclick={run}>Try again</Button>
    {:else}
        <MachineSteps {steps} {events} />
        <Button loading={busy} disabled={busy} onclick={run}>{actionLabel()}</Button>
        {#if error}
            <p class="text-destructive text-sm">{error}</p>
        {/if}
    {/if}
</div>
