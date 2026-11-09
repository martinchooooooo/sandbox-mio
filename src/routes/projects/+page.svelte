<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { invoke } from "@tauri-apps/api/core";
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
    import SandboxItem from "./SandboxItem.svelte";
    import CreateSandboxItem from "./CreateSandboxItem.svelte";
    import { activityEvents, containerStatusFor } from "$lib/activity/activity.svelte";
    import type { Project } from "$lib/sandbox";
    import {
        ensureSandboxRemoteState,
        sandboxesList,
        deploymentFor,
        sandboxLoadError,
        refreshSandboxState,
    } from "$lib/sandbox/state.svelte";

    let error = $state("");
    let currentImageVersion = $state<number | undefined>(undefined);

    // The store's raw list, overlaid with the latest container status from the activity store's
    // event stream (src/lib/activity/activity.svelte.ts) for responsiveness between the store's 3 s
    // poll ticks. Reading activityEvents() makes this effect re-run on each container event.
    let projects = $state<Project[]>([]);
    $effect(() => {
        activityEvents();
        projects = sandboxesList().map((p) => {
            const status = containerStatusFor(p.name);
            return status && status !== p.status ? { ...p, status } : p;
        });
    });

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
        invoke<number>("base_image_version").then((v) => (currentImageVersion = v)).catch(() => {});
    });

    async function startPodman() {
        if (await startPodmanMachine()) ensureSandboxRemoteState();
    }

    async function setupPodman() {
        if (await provisionPodmanMachine()) ensureSandboxRemoteState();
    }
</script>

<main class="p-6 pb-52">
    <h1 class="text-center text-3xl mb-8">{m.projects_title()}</h1>

    <!-- @max-[420px]: a container query against NavigationColumn.svelte's `@container` ancestor
         (see the comment on SandboxItem.svelte's @max-[420px] usage) — a plain viewport media query
         would never fire once main-navigation-panel became the full app window (Phase C,
         docs/features/consolidate-app-container/02_dev.md). -->
    <div class="max-w-2xl mx-auto @max-[420px]:max-w-none @max-[420px]:mx-0">
        <!-- <Field.Set class="w-full">
            <Field.Group>
                <Field.Field>
                    <Field.Label>{m.projects_name_label()}</Field.Label>
                    <div class="flex w-full gap-2">
                        <Input placeholder={m.projects_name_placeholder()} bind:value={projectName} />
                        <Button
                            loading={action.busy === "create"}
                            disabled={action.busy !== null}
                            onclick={createProject}
                        >{m.projects_create()}</Button>
                    </div>
                </Field.Field>
            </Field.Group>
        </Field.Set> -->

        {#if error || sandboxLoadError()}
            <p class="text-destructive mt-4 text-sm">{error || sandboxLoadError()}</p>
        {/if}

        {#if podmanUnavailable() && podmanNeedsSetup()}
            <div class="mt-8 text-center space-y-4">
                <h2 class="text-lg font-medium">{m.podman_setup_title()}</h2>
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
            <ul class="stagger mt-6 space-y-2">
                {#each projects as project (project.id)}
                    <SandboxItem
                        {project}
                        {currentImageVersion}
                        deployment={deploymentFor(project.name)}
                        onChanged={refreshSandboxState}
                    />
                {/each}
                <CreateSandboxItem onCreated={refreshSandboxState} />
            </ul>
        {/if}
    </div>
</main>
