<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Field from "$lib/components/ui/field";
    import { Input } from "$lib/components/ui/input";
    import { invoke } from "@tauri-apps/api/core";
    import { onMount, onDestroy } from "svelte";
    import { parseError } from "$lib/errors";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import SandboxItem from "./SandboxItem.svelte";
    import StatusRow from "./StatusRow.svelte";
    import { activityEvents, containerStatusFor } from "$lib/activity/activity.svelte";
    import type { Project, Deployment } from "$lib/sandbox";

    let projectName = $state("");
    let projects = $state<Project[]>([]);
    let error = $state("");
    let currentImageVersion = $state<number | undefined>(undefined);

    // Deployment state, overlaid onto the local sandbox list by name. A sandbox with an entry is
    // deployed to `deployments[name].host`; one without is local-only. Polled, since sync status
    // changes outside our event stream.
    let hosts = $state<string[]>([]);
    let deployments = $state<Record<string, Deployment>>({});

    const action = createActionRunner();

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

    async function createProject() {
        const name = projectName.trim();
        if (!name) return;
        if (await action.run(
            "create",
            () => invoke("create_sandbox", { name }),
            { message: m.projects_status_creating({ name }) }
        )) {
            projectName = "";
            await loadProjects();
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

    onMount(() => {
        loadProjects();
        loadHosts();
        loadDeployments();
        invoke<number>("base_image_version").then((v) => (currentImageVersion = v)).catch(() => {});
        // Sync status drifts outside the container event stream, so poll the deployment overlay.
        pollId = setInterval(loadDeployments, 3000);
    });

    onDestroy(() => clearInterval(pollId));
</script>

<main class="p-6 pb-52">
    <Button href="/" variant="outline" class="mb-6">{m.back()}</Button>

    <h1 class="text-center text-3xl mb-8">{m.projects_title()}</h1>

    <div class="max-w-2xl mx-auto">
        <Field.Set class="w-full">
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
        </Field.Set>

        {#if error}
            <p class="text-destructive mt-4 text-sm">{error}</p>
        {/if}

        <StatusRow busy={action.busy} message={action.message} error={action.error} />

        {#if projects.length > 0}
            <ul class="mt-6 space-y-2">
                {#each projects as project (project.id)}
                    <SandboxItem
                        {project}
                        {hosts}
                        {currentImageVersion}
                        deployment={deployments[project.name]}
                        onChanged={reload}
                    />
                {/each}
            </ul>
        {/if}
    </div>
</main>
