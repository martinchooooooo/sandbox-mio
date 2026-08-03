<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import Button from "$lib/components/ui/button/button.svelte";
    import { parseError } from "$lib/errors";
    import { createActionRunner } from "$lib/actions.svelte";
    import {
        checkPodman,
        podmanUnavailable,
        podmanNeedsSetup,
        podmanRebootRequired,
        setupError,
        startError,
        startPodman,
        setupPodman,
    } from "$lib/podman/setup.svelte";

    let { ondone }: { ondone?: (done: boolean) => void } = $props();

    let probing = $state(true);
    let ready = $state(false);
    let probeError = $state("");

    const action = createActionRunner();
    const done = $derived(!probing && ready);

    $effect(() => ondone?.(done));

    async function probe() {
        try {
            ready = await invoke<boolean>("base_image_ready");
            probeError = "";
        } catch (e) {
            probeError = parseError(e).message;
        } finally {
            probing = false;
        }
    }

    onMount(probe);

    // Provisioning the machine is chained in rather than exposed as a prerequisite click: the image
    // cannot build without it, and one busy state covers the whole chain (02_dev.md, Resolved Decisions).
    async function download() {
        const ok = await action.run("build", async () => {
            await checkPodman();
            if (podmanUnavailable()) {
                const started = podmanNeedsSetup() ? await setupPodman() : await startPodman();
                // Rethrown in `AppError` shape so the runner surfaces the store's message; the
                // reboot case is rendered from the store, which keeps the structured kind.
                if (!started) throw { kind: "Backend", message: setupError() || startError() };
            }
            await invoke("build_base_image");
        });
        if (ok) await probe();
    }
</script>

<div class="border-2 rounded-md shadow-md p-5 space-y-3" data-step="download-necessities" data-done={done}>
    <!-- TODO(i18n) -->
    <h3 class="text-lg font-medium">Download necessities</h3>
    <!-- TODO(i18n) -->
    <p class="text-muted-foreground text-sm">
        Builds the base image every sandbox starts from. This takes a few minutes the first time.
    </p>

    {#if probing}
        <!-- TODO(i18n) -->
        <p class="text-muted-foreground text-sm">Checking…</p>
    {:else if done}
        <!-- TODO(i18n) -->
        <p class="text-sm font-medium">Already downloaded ✓</p>
    {:else if podmanRebootRequired()}
        <!-- TODO(i18n) -->
        <p class="text-destructive text-sm">
            Windows needs to restart to finish installing WSL2. Restart, then come back here.
        </p>
        <!-- TODO(i18n) -->
        <Button onclick={download}>Try again</Button>
    {:else}
        <!-- TODO(i18n) -->
        <Button loading={action.busy !== null} disabled={action.busy !== null} onclick={download}>
            Download necessities
        </Button>
        {#if action.error || probeError}
            <p class="text-destructive text-sm">{action.error || probeError}</p>
        {/if}
    {/if}
</div>
