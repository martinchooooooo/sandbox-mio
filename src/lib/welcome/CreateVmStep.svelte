<script lang="ts">
    import { onMount } from "svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import { parseError } from "$lib/errors";
    import {
        checkPodman,
        podmanUnavailable,
        podmanNeedsSetup,
        podmanRebootRequired,
        setupBusy,
        setupError,
        startBusy,
        startError,
        startPodman,
        setupPodman,
    } from "$lib/podman/setup.svelte";

    let { ondone }: { ondone?: (done: boolean) => void } = $props();

    let probing = $state(true);
    let probeError = $state("");

    // The probe runs in both variants, so a user who already has a machine sees the completed state
    // on first run too, not just on replay.
    const done = $derived(!probing && probeError === "" && !podmanUnavailable());

    $effect(() => ondone?.(done));

    onMount(async () => {
        try {
            await checkPodman();
        } catch (e) {
            probeError = parseError(e).message;
        } finally {
            probing = false;
        }
    });

    // Provisioning and starting are the same button here: which one applies is Podman's state,
    // not a choice the user should have to make.
    function run() {
        return podmanNeedsSetup() ? setupPodman() : startPodman();
    }

    const busy = $derived(setupBusy() || startBusy());
</script>

<div class="border-2 rounded-md shadow-md p-5 space-y-3" data-step="create-vm" data-done={done}>
    <!-- TODO(i18n) -->
    <h3 class="text-lg font-medium">Create virtual machine</h3>
    <!-- TODO(i18n) -->
    <p class="text-muted-foreground text-sm">
        Sandboxes run inside a Podman virtual machine. This sets it up on your computer.
    </p>

    {#if probing}
        <!-- TODO(i18n) -->
        <p class="text-muted-foreground text-sm">Checking…</p>
    {:else if probeError}
        <p class="text-destructive text-sm">{probeError}</p>
    {:else if done}
        <!-- TODO(i18n) -->
        <p class="text-sm font-medium">Already set up ✓</p>
    {:else if podmanRebootRequired()}
        <!-- TODO(i18n) -->
        <p class="text-destructive text-sm">
            Windows needs to restart to finish installing WSL2. Restart, then come back here.
        </p>
        <!-- TODO(i18n) -->
        <Button onclick={run}>Try again</Button>
    {:else}
        <!-- TODO(i18n) -->
        <Button loading={busy} disabled={busy} onclick={run}>
            {podmanNeedsSetup() ? "Create virtual machine" : "Start virtual machine"}
        </Button>
        {#if setupError() || startError()}
            <p class="text-destructive text-sm">{setupError() || startError()}</p>
        {/if}
    {/if}
</div>
