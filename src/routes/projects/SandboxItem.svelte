<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Card from "$lib/components/ui/card";
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { m } from "$lib/paraglide/messages.js";
    import {
        lifecycleError,
        lifecycleMessage,
        pendingOp,
        startSandbox,
        stopSandbox,
    } from "$lib/sandbox/lifecycle.svelte";
    import { createActionRunner } from "$lib/actions.svelte";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { DashboardSquare01Icon, PlayIcon, StopIcon } from "@hugeicons/core-free-icons";
    import StatusRow from "$lib/sandbox/StatusRow.svelte";
    import { openSandbox } from "$lib/workspace/navigation.svelte";
    import {
        translateStatus,
        statusDotClass,
        type Project,
        type Deployment,
    } from "$lib/sandbox";

    let {
        project,
        deployment,
        currentImageVersion,
    }: {
        project: Project;
        deployment: Deployment | undefined;
        currentImageVersion: number | undefined;
    } = $props();

    let dismissedVersion = $state<number | undefined>(undefined);

    const imageOutdated = $derived(
        currentImageVersion !== undefined &&
        project.image_version !== undefined &&
        project.image_version < currentImageVersion &&
        (dismissedVersion === undefined || dismissedVersion < currentImageVersion)
    );

    const action = createActionRunner();

    const pending = $derived(pendingOp(project.name));

    // The card always lands on Manage; the sidebar restores the last view instead
    // (docs/features/unified-sidebar-nav/02_dev.md, Resolved Decision 2).
    async function openWorkspace() {
        await action.run(
            "workspace",
            () => openSandbox(project.name, project.workspace_path, { restore: false }),
            { message: m.projects_status_opening_workspace() },
        );
    }

    async function dismissUpdate() {
        if (currentImageVersion === undefined) return;
        await invoke("dismiss_image_update", {
            workspacePath: project.workspace_path,
            version: currentImageVersion,
        });
        dismissedVersion = currentImageVersion;
    }

    onMount(async () => {
        dismissedVersion = await invoke<number | undefined>("get_dismissed_image_version", {
            workspacePath: project.workspace_path,
        }) ?? undefined;
    });
</script>

<li data-sandbox={project.name} data-pending={pending}>
<Card.Root class="h-full gap-3 p-4 text-sm">
    <div class="flex min-w-0 items-center gap-2">
        <span class={["inline-block size-2.5 shrink-0 rounded-full", statusDotClass(project.status, deployment, pending)]}></span>
        <Card.Title class="mb-0 truncate">{project.name}</Card.Title>
        {#if deployment}
            <span class="text-muted-foreground text-xs shrink-0">→ {deployment.host}</span>
        {/if}
    </div>

    <!-- Label text doubles as the e2e selector (`button=workspace`, `button=start`), so labels stay
         visible: Home cards always have room since pages use the full content area. -->
    <div class="flex flex-wrap items-center gap-2">
        <Button size="sm" onclick={openWorkspace} title={m.projects_workspace()}
            loading={action.busy === "workspace"} disabled={action.busy !== null || pending !== undefined}>
            <HugeiconsIcon icon={DashboardSquare01Icon} size={16} />
            <span>{m.projects_workspace()}</span>
        </Button>
        {#if project.status === "running"}
            <Button variant="ghost" size="sm" onclick={() => stopSandbox(project.name)} title={m.projects_stop()}
                loading={pending === "stop"} disabled={action.busy !== null || pending !== undefined}>
                <HugeiconsIcon icon={StopIcon} size={16} />
                <span>{m.projects_stop()}</span>
            </Button>
        {:else}
            <Button variant="ghost" size="sm" onclick={() => startSandbox(project.name)} title={m.projects_start()}
                loading={pending === "start"} disabled={action.busy !== null || pending !== undefined}>
                <HugeiconsIcon icon={PlayIcon} size={16} />
                <span>{m.projects_start()}</span>
            </Button>
        {/if}
    </div>

    <StatusRow
        busy={pending ?? action.busy}
        message={pending ? lifecycleMessage(pending) : action.message}
        error={lifecycleError(project.name) || action.error}
        status={translateStatus(deployment ? deployment.status : project.status)}
    />

    {#if imageOutdated}
        <div class="flex items-center gap-2">
            <p class="text-xs text-amber-600 dark:text-amber-400 flex-1">{m.projects_image_outdated()}</p>
            <button class="text-xs text-muted-foreground hover:text-foreground shrink-0" onclick={dismissUpdate}>
                {m.projects_image_dismiss()}
            </button>
        </div>
    {/if}
</Card.Root>
</li>
