<script lang="ts">
    import { onMount } from "svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import MachineSteps from "$lib/machine/MachineSteps.svelte";
    import { prepareMachine } from "$lib/machine/readiness";
    import { ensureMachineReadiness, machineReadiness } from "$lib/machine/readiness.svelte";
    import type { MachineSetupEvent, ProviderId } from "$lib/machine/types";

    // `provider` has no picker yet — Phase J (docs/features/microsandbox-provider/02_dev.md) adds
    // the frontend selector that will pass something other than the default here.
    let { provider = "podman" as ProviderId, ondone }: {
        provider?: ProviderId;
        ondone?: (done: boolean) => void;
    } = $props();

    let events = $state<MachineSetupEvent[]>([]);
    let busy = $state(false);
    let error = $state("");
    let rebootMessage = $state("");

    const status = $derived(machineReadiness(provider));
    // The probe runs in both variants, so a user who already has a machine sees the completed state
    // on first run too, not just on replay.
    const done = $derived(!status.probing && status.readiness.ready);

    $effect(() => ondone?.(done));

    onMount(() => ensureMachineReadiness(provider));

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
            await ensureMachineReadiness(provider, true);
        } else if (!result.rebootRequired) {
            error = result.message;
        }
        busy = false;
    }

    function actionLabel(): string {
        // TODO(i18n)
        if (provider === "microsandbox") return "Set up microsandbox";
        return status.readiness.needsSetup ? "Create virtual machine" : "Start virtual machine";
    }
</script>

<div class="border-2 rounded-md shadow-md p-5 space-y-3" data-step="create-vm" data-done={done}>
    <!-- TODO(i18n) -->
    <h3 class="text-lg font-medium">Create virtual machine</h3>
    <!-- TODO(i18n) -->
    <p class="text-muted-foreground text-sm">
        Sandboxes run inside a virtual machine. This sets it up on your computer.
    </p>

    {#if status.probing}
        <!-- TODO(i18n) -->
        <p class="text-muted-foreground text-sm">Checking…</p>
    {:else if status.errorKind === "PodmanUnresponsive"}
        <!-- Named as a stall rather than a fault to fix: the remedies the other failure branches
             offer do not apply to a binary that never answers. TODO(i18n) -->
        <p class="text-destructive text-sm" data-error="podman-unresponsive">
            Podman started but never answered. Retrying may clear it; if it does not, the details are
            in <code>mio.log</code>.
        </p>
        <!-- TODO(i18n) -->
        <Button onclick={() => ensureMachineReadiness(provider, true)}>Try again</Button>
    {:else if status.error}
        <p class="text-destructive text-sm">{status.error}</p>
        <!-- Without this the card is permanently dead after a failed probe, with an app restart as
             the only escape. TODO(i18n) -->
        <Button onclick={() => ensureMachineReadiness(provider, true)}>Try again</Button>
    {:else if done}
        <!-- TODO(i18n) -->
        <p class="text-sm font-medium">Already set up ✓</p>
    {:else if rebootMessage}
        <p class="text-destructive text-sm">{rebootMessage}</p>
        <!-- TODO(i18n) -->
        <Button onclick={run}>Try again</Button>
    {:else}
        <MachineSteps steps={status.steps} {events} />
        <Button loading={busy} disabled={busy} onclick={run}>{actionLabel()}</Button>
        {#if error}
            <p class="text-destructive text-sm">{error}</p>
        {/if}
    {/if}
</div>
