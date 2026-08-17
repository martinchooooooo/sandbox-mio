<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import Button from "$lib/components/ui/button/button.svelte";
    import AdvancedSection from "$lib/components/AdvancedSection.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { parseError, type AppErrorKind } from "$lib/errors";
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
    let probeErrorKind = $state<AppErrorKind | null>(null);

    const action = createActionRunner();
    const done = $derived(!probing && ready);

    $effect(() => ondone?.(done));

    async function probe() {
        try {
            ready = await invoke<boolean>("base_image_ready");
            probeError = "";
            probeErrorKind = null;
        } catch (e) {
            const err = parseError(e);
            probeError = err.message;
            probeErrorKind = err.kind;
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
    <h3 class="text-lg font-medium">{m.welcome_download_title()}</h3>
    <AdvancedSection label={m.welcome_technical_details_label()}>
        <p class="text-muted-foreground text-sm">
            {m.welcome_download_technical_details()}
        </p>
    </AdvancedSection>

    {#if probing}
        <p class="text-muted-foreground text-sm">{m.welcome_checking()}</p>
    {:else if done}
        <p class="text-sm font-medium">{m.welcome_download_already_done()}</p>
    {:else if probeErrorKind === "PodmanUnresponsive"}
        <!-- Named as a stall rather than a fault to fix: neither `setupPodman` nor `startPodman`
             helps a binary that never answers. -->
        <p class="text-destructive text-sm" data-error="podman-unresponsive">
            {m.welcome_podman_unresponsive_before()} <code>mio.log</code>{m.welcome_podman_unresponsive_after()}
        </p>
        <Button onclick={probe}>{m.welcome_try_again()}</Button>
    {:else if podmanRebootRequired()}
        <p class="text-destructive text-sm">
            {m.welcome_wsl_reboot_message()}
        </p>
        <Button onclick={download}>{m.welcome_try_again()}</Button>
    {:else}
        <Button loading={action.busy !== null} disabled={action.busy !== null} onclick={download}>
            {m.welcome_download_button()}
        </Button>
        {#if action.error || probeError}
            <p class="text-destructive text-sm">{action.error || probeError}</p>
        {/if}
    {/if}
</div>
