<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Text } from "$lib/components/ui/text";
    import { baseImageVersion } from "$lib/api/image";
    import { onMount } from "svelte";
    import { parseError } from "$lib/errors";
    import { m } from "$lib/paraglide/messages.js";
    import {
        checkPodman,
        podmanUnavailable,
        podmanNeedsSetup,
        podmanRebootRequired,
        setupBusy,
        setupError,
        startBusy,
        startError,
        startPodman as startPodmanMachine,
        setupPodman as provisionPodmanMachine,
    } from "$lib/podman/setup.svelte";
    import * as Card from "$lib/components/ui/card";
    import { Spinner } from "$lib/components/ui/spinner";
    import { dismissCreate, pendingCreates, retryCreate } from "$lib/sandbox/lifecycle.svelte";
    import { statusDotClass } from "$lib/sandbox";
    import SandboxItem from "./SandboxItem.svelte";
    import CreateSandboxItem from "./CreateSandboxItem.svelte";
    import {
        ensureSandboxRemoteState,
        sandboxesList,
        deploymentFor,
        sandboxLoadError,
        liveStatus,
    } from "$lib/sandbox/state.svelte";

    let error = $state("");
    let currentImageVersion = $state<number | undefined>(undefined);

    // The store's raw list, overlaid with the latest container status from the activity store's
    // event stream for responsiveness between the store's 3 s poll ticks.
    const projects = $derived(sandboxesList().map((p) => ({ ...p, status: liveStatus(p) })));

    // A create whose real entry is already listed (the instant before its placeholder is removed)
    // renders as the real card only, unless it failed: that one keeps its error visible.
    const placeholders = $derived(
        pendingCreates().filter((c) => c.error || !projects.some((p) => p.name === c.name)),
    );

    onMount(async () => {
        // Probe Podman before anything else: a stopped machine after a host restart should show the
        // "Start Podman" screen, not a wall of failing load errors.
        try {
            await checkPodman();
        } catch (e) {
            error = parseError(e).message;
            return;
        }
        if (podmanUnavailable()) return;
        ensureSandboxRemoteState();
        baseImageVersion().then((v) => (currentImageVersion = v)).catch(() => {});
    });

    async function startPodman() {
        if (await startPodmanMachine()) ensureSandboxRemoteState();
    }

    async function setupPodman() {
        if (await provisionPodmanMachine()) ensureSandboxRemoteState();
    }
</script>

<!-- Home. The grid's @min-[44rem] is a container query against the layout's route outlet, i.e. the
     content area beside the app sidebar, not the window. -->
<main data-home class="mx-auto w-full max-w-4xl p-6 pb-16">
    <Text as="h1">{m.projects_title()}</Text>
    <p class="text-muted-foreground mt-2 mb-6 max-w-2xl">{m.home_intro()}</p>

    <div>
        {#if error || sandboxLoadError()}
            <p class="text-destructive mt-4 text-sm">{error || sandboxLoadError()}</p>
        {/if}

        {#if podmanUnavailable() && podmanNeedsSetup()}
            <div class="mt-8 text-center space-y-4">
                <Text as="h2">{m.podman_setup_title()}</Text>
                <p class="text-muted-foreground">{m.podman_setup_description()}</p>
                {#if podmanRebootRequired()}
                    <p class="text-destructive text-sm">{m.podman_reboot_required()}</p>
                    <Button onclick={setupPodman}>{m.podman_reboot_retry()}</Button>
                {:else}
                    <Button loading={setupBusy()} disabled={setupBusy()} onclick={setupPodman}
                        >{m.podman_setup_button()}</Button
                    >
                    {#if setupError()}
                        <p class="text-destructive text-sm">{setupError()}</p>
                        <Button variant="outline" onclick={setupPodman}>{m.retry()}</Button>
                    {/if}
                {/if}
            </div>
        {:else if podmanUnavailable()}
            <div class="mt-8 text-center space-y-4">
                <p class="text-muted-foreground">{m.podman_not_running()}</p>
                <Button
                    loading={startBusy()}
                    disabled={startBusy()}
                    onclick={startPodman}
                >{m.podman_start_button()}</Button>
                {#if startError()}
                    <p class="text-destructive text-sm">{startError()}</p>
                    <Button variant="outline" onclick={startPodman}>{m.retry()}</Button>
                {/if}
            </div>
        {:else}
            <ul class="stagger grid gap-4 @min-[44rem]:grid-cols-2">
                {#each projects as project (project.id)}
                    <SandboxItem
                        {project}
                        {currentImageVersion}
                        deployment={deploymentFor(project.name)}
                    />
                {/each}
                {#each placeholders as create (create.name)}
                    <li data-sandbox={create.name} data-pending="create" data-create-placeholder>
                        <Card.Root class="h-full gap-3 p-4 text-sm">
                            <div class="flex min-w-0 items-center gap-2">
                                <span
                                    class={["inline-block size-2.5 shrink-0 rounded-full", create.error ? "bg-destructive" : statusDotClass(undefined, undefined, "create")]}
                                ></span>
                                <Card.Title class="mb-0 truncate">{create.name}</Card.Title>
                            </div>
                            {#if create.error}
                                <p role="status" class="text-xs text-destructive">{create.error}</p>
                                <div class="flex flex-wrap items-center gap-2">
                                    <Button size="sm" data-create-retry onclick={() => retryCreate(create.name)}>{m.retry()}</Button>
                                    <!-- TODO(i18n) -->
                                    <Button variant="ghost" size="sm" data-create-dismiss onclick={() => dismissCreate(create.name)}>Dismiss</Button>
                                </div>
                            {:else}
                                <div role="status" aria-live="polite" class="flex items-center gap-1.5 text-xs text-muted-foreground">
                                    <Spinner class="size-3.5" />
                                    <span data-create-step>{create.step}</span>
                                </div>
                            {/if}
                        </Card.Root>
                    </li>
                {/each}
                <CreateSandboxItem />
            </ul>
        {/if}
    </div>
</main>
