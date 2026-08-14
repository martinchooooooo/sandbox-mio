<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { onMount, onDestroy } from "svelte";
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
    import type { Project, Deployment } from "$lib/sandbox";
    import { allCapabilities, type ProviderCapabilities, type ProviderId } from "$lib/providers";

    let capabilities = $state<Record<ProviderId, ProviderCapabilities> | undefined>(undefined);

    let projects = $state<Project[]>([]);
    let error = $state("");
    let currentImageVersion = $state<number | undefined>(undefined);

    // Deployment state, overlaid onto the local sandbox list by name. A sandbox with an entry is
    // deployed to `deployments[name].host`; one without is local-only. Polled, since sync status
    // changes outside our event stream.
    let hosts = $state<string[]>([]);
    let deployments = $state<Record<string, Deployment>>({});

    async function loadProjects() {
        try {
            projects = await invoke<Project[]>("list_sandboxes");
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function loadDeployments() {
        try {
            const list = await invoke<Deployment[]>("sandbox_deployments");
            deployments = Object.fromEntries(list.map((d) => [d.name, d]));
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function loadHosts() {
        try {
            hosts = await invoke<string[]>("list_sandbox_hosts");
        } catch (e) {
            error = parseError(e).message;
        }
    }

    // The global activity store now owns the listen_events container stream (it superseded the
    // route-local EventLog). Re-apply the latest container status to each project whenever a new
    // container event arrives; reading activityEvents() makes this effect re-run on each event.
    $effect(() => {
        activityEvents();
        for (let i = 0; i < projects.length; i++) {
            const status = containerStatusFor(projects[i].name);
            if (status && projects[i].status !== status) {
                projects[i] = { ...projects[i], status };
            }
        }
    });

    // After a sandbox mutates its deploy/lifecycle state, refresh both overlays it depends on.
    function reload() {
        return Promise.all([loadDeployments(), loadProjects()]);
    }

    let pollId: ReturnType<typeof setInterval>;

    // The normal load sequence, run only once Podman is confirmed ready. Sync status drifts outside
    // the container event stream, so the caller follows this with the deployment poll.
    function loadAll() {
        loadProjects();
        loadHosts();
        loadDeployments();
        invoke<number>("base_image_version").then((v) => (currentImageVersion = v)).catch(() => {});
        allCapabilities().then((c) => (capabilities = c)).catch(() => {});
    }

    // Only started once Podman is confirmed ready: polling a down Podman floods the backend with
    // failing invocations.
    function startLoadAndPoll() {
        loadAll();
        pollId = setInterval(loadDeployments, 3000);
    }

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
        startLoadAndPoll();
    });

    async function startPodman() {
        if (await startPodmanMachine()) startLoadAndPoll();
    }

    async function setupPodman() {
        if (await provisionPodmanMachine()) startLoadAndPoll();
    }

    onDestroy(() => clearInterval(pollId));
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

        {#if error}
            <p class="text-destructive mt-4 text-sm">{error}</p>
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
            <ul class="mt-6 space-y-2">
                {#each projects as project (project.id)}
                    <SandboxItem
                        {project}
                        {hosts}
                        {currentImageVersion}
                        deployment={deployments[project.name]}
                        capabilities={capabilities?.[project.provider]}
                        onChanged={reload}
                    />
                {/each}
                <CreateSandboxItem {projects} onCreated={reload} />
            </ul>
        {/if}
    </div>
</main>
