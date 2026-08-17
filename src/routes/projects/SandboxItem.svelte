<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { DashboardSquare01Icon, PlayIcon, StopIcon } from "@hugeicons/core-free-icons";
    import StatusRow from "./StatusRow.svelte";
    import { setActiveSandbox } from "$lib/workspace/state.svelte";
    import {
        translateStatus,
        syncColor,
        type Project,
        type Deployment,
    } from "$lib/sandbox";

    let {
        project,
        deployment,
        currentImageVersion,
        onChanged,
    }: {
        project: Project;
        deployment: Deployment | undefined;
        currentImageVersion: number | undefined;
        // Tells the parent to reload its sandbox and deployment lists after a mutating action.
        onChanged: () => void;
    } = $props();

    let dismissedVersion = $state<number | undefined>(undefined);

    const imageOutdated = $derived(
        currentImageVersion !== undefined &&
        project.image_version !== undefined &&
        project.image_version < currentImageVersion &&
        (dismissedVersion === undefined || dismissedVersion < currentImageVersion)
    );

    const action = createActionRunner();

    async function start() {
        if (await action.run(
            "start",
            () => invoke("start_sandbox", { name: project.name }),
            { message: m.projects_status_starting() }
        )) {
            onChanged();
        }
    }

    async function stop() {
        if (await action.run(
            "stop",
            () => invoke("stop_sandbox", { name: project.name }),
            { message: m.projects_status_stopping() }
        )) {
            onChanged();
        }
    }

    // Primary action: navigate the merged webview's client-side view to this sandbox's Dashboard
    // (docs/features/consolidate-app-container/02_dev.md); `watch_workspace_ports` no longer embeds
    // a pane, it only (idempotently) starts `.mio/ports.json` watching. `hide_active_pane` clears
    // whatever Terminal/AgentWeb overlay another sandbox may have left active, so this sandbox's
    // Dashboard actually becomes visible (its visibility is derived from `active_app` being null).
    async function openWorkspace() {
        setActiveSandbox(project.name);
        await action.run(
            "workspace",
            async () => {
                await Promise.all([
                    invoke("watch_workspace_ports", { name: project.name, workspacePath: project.workspace_path }),
                    invoke("hide_active_pane"),
                ]);
            },
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

<li data-sandbox={project.name} class="flex flex-col gap-2 rounded-lg border p-3 text-sm">
    <div class="flex justify-between items-center gap-2">
        <span class="flex items-center gap-2 font-medium min-w-0">
            <span class={[
                "inline-block h-2 w-2 rounded-full shrink-0",
                deployment ? syncColor(deployment.status) :
                project.status === "running" ? "bg-green-500" :
                project.status === "paused"  ? "bg-yellow-400" :
                project.status === "exited"  ? "bg-red-400" :
                "bg-muted-foreground/40"
            ].join(" ")}></span>
            <span class="truncate">{project.name}</span>
            {#if deployment}
                <span class="text-muted-foreground text-xs shrink-0">→ {deployment.host}</span>
            {/if}
        </span>
        <!-- @max-[420px]: a *container* query (not a viewport media query) against
             NavigationColumn.svelte's `@container` ancestor, whose width is
             NAVIGATION_PANEL_EXPANDED_W = 380 (src-tauri/src/workspace/mod.rs) once a workspace is
             open: main-navigation-panel is the full app window since Phase C
             (docs/features/consolidate-app-container/02_dev.md), so a plain viewport media query
             here would never fire — labels drop to icons only when the nav *column*, not the
             window, narrows past 420px. -->
        <span class="flex items-center gap-2 shrink-0">
            {#if project.status === "running"}
                <Button variant="ghost" size="sm" onclick={stop} title={m.projects_stop()}
                    loading={action.busy === "stop"} disabled={action.busy !== null}>
                    <HugeiconsIcon icon={StopIcon} size={16} />
                    <span class="@max-[420px]:hidden">{m.projects_stop()}</span>
                </Button>
            {:else}
                <Button variant="ghost" size="sm" onclick={start} title={m.projects_start()}
                    loading={action.busy === "start"} disabled={action.busy !== null}>
                    <HugeiconsIcon icon={PlayIcon} size={16} />
                    <span class="@max-[420px]:hidden">{m.projects_start()}</span>
                </Button>
            {/if}
            <Button variant="ghost" size="sm" onclick={openWorkspace} title={m.projects_workspace()}
                loading={action.busy === "workspace"} disabled={action.busy !== null}>
                <HugeiconsIcon icon={DashboardSquare01Icon} size={16} />
                <span class="@max-[420px]:hidden">{m.projects_workspace()}</span>
            </Button>
        </span>
    </div>

    <StatusRow
        busy={action.busy}
        message={action.message}
        error={action.error}
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
</li>
