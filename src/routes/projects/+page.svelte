<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { onMount, onDestroy } from "svelte";
    import { parseError } from "$lib/errors";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import SandboxItem from "./SandboxItem.svelte";
    import CreateSandboxItem from "./CreateSandboxItem.svelte";
    import { activityEvents, containerStatusFor } from "$lib/activity/activity.svelte";
    import type { Project, Deployment } from "$lib/sandbox";

    let projects = $state<Project[]>([]);
    let error = $state("");
    let currentImageVersion = $state<number | undefined>(undefined);

    // Deployment state, overlaid onto the local sandbox list by name. A sandbox with an entry is
    // deployed to `deployments[name].host`; one without is local-only. Polled, since sync status
    // changes outside our event stream.
    let hosts = $state<string[]>([]);
    let deployments = $state<Record<string, Deployment>>({});

    // Startup gate: set when `check_podman_ready` reports the machine/socket is down. While true the
    // sandbox list is replaced by the "Start Podman" screen and the deployment poll is not started
    // (polling a down Podman floods the backend with failing invocations).
    let podmanUnavailable = $state(false);

    // First-run gate: set when the default Podman machine has never been created (`machine_needs_init`
    // returns true). Takes priority over `podmanUnavailable`'s plain "Start Podman" screen, since
    // starting a machine that was never initialized cannot succeed.
    let podmanNeedsSetup = $state(false);

    // Set when `provision_podman` reports a fresh WSL2 install needs a Windows restart before it can
    // continue. Shown instead of a generic setup failure so the user knows to reboot, not retry blindly.
    let podmanRebootRequired = $state(false);

    const startAction = createActionRunner();

    // Not `createActionRunner`-driven: that helper only keeps the error message, but this flow
    // needs the structured `kind` to tell a reboot requirement apart from a generic failure.
    let setupBusy = $state(false);
    let setupError = $state("");

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
    }

    onMount(async () => {
        // Probe Podman before anything else: a stopped machine after a host restart should show the
        // "Start Podman" screen, not a wall of failing load errors.
        try {
            await invoke("check_podman_ready");
        } catch (e) {
            if (parseError(e).kind === "PodmanUnavailable") {
                // Distinguish "never initialized" (first run) from "just stopped": starting a
                // machine that was never created cannot succeed, so route to the setup flow instead.
                try {
                    podmanNeedsSetup = await invoke<boolean>("machine_needs_init");
                } catch {
                    podmanNeedsSetup = false;
                }
                podmanUnavailable = true;
                return; // skip the load sequence; the deployment poll stays unstarted
            }
            error = parseError(e).message;
            return;
        }
        loadAll();
        pollId = setInterval(loadDeployments, 3000);
    });

    // Drive the startup sequence from the "Start Podman" button. On success, clear the gate and run
    // the normal load + poll; on failure `startAction.error` surfaces a Retry button.
    async function startPodman() {
        const ok = await startAction.run(
            "start",
            () => invoke("start_podman_machine"),
            { message: m.podman_starting() }
        );
        if (ok) {
            podmanUnavailable = false;
            loadAll();
            pollId = setInterval(loadDeployments, 3000);
        }
    }

    // Drive first-run provisioning (WSL2 + `podman machine init` + start) from the setup screen.
    // A `PodmanRebootRequired` error keeps the screen up with a reboot prompt instead of a bare retry.
    async function setupPodman() {
        podmanRebootRequired = false;
        setupError = "";
        setupBusy = true;
        try {
            await invoke("provision_podman");
            podmanNeedsSetup = false;
            podmanUnavailable = false;
            loadAll();
            pollId = setInterval(loadDeployments, 3000);
        } catch (e) {
            const err = parseError(e);
            podmanRebootRequired = err.kind === "PodmanRebootRequired";
            setupError = err.message;
        } finally {
            setupBusy = false;
        }
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

        {#if podmanUnavailable && podmanNeedsSetup}
            <div class="mt-8 text-center space-y-4">
                <h2 class="text-lg font-medium">{m.podman_setup_title()}</h2>
                <p class="text-muted-foreground">{m.podman_setup_description()}</p>
                {#if podmanRebootRequired}
                    <p class="text-destructive text-sm">{m.podman_reboot_required()}</p>
                    <Button onclick={setupPodman}>{m.podman_reboot_retry()}</Button>
                {:else}
                    <Button loading={setupBusy} disabled={setupBusy} onclick={setupPodman}
                        >{m.podman_setup_button()}</Button
                    >
                    {#if setupError}
                        <p class="text-destructive text-sm">{setupError}</p>
                        <Button variant="outline" onclick={setupPodman}>{m.retry()}</Button>
                    {/if}
                {/if}
            </div>
        {:else if podmanUnavailable}
            <div class="mt-8 text-center space-y-4">
                <p class="text-muted-foreground">{m.podman_not_running()}</p>
                <Button
                    loading={startAction.busy === "start"}
                    disabled={startAction.busy !== null}
                    onclick={startPodman}
                >{m.podman_start_button()}</Button>
                {#if startAction.error}
                    <p class="text-destructive text-sm">{startAction.error}</p>
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
                        onChanged={reload}
                    />
                {/each}
                <CreateSandboxItem {projects} onCreated={reload} />
            </ul>
        {/if}
    </div>
</main>
