<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Switch } from "$lib/components/ui/switch";
    import { invoke } from "@tauri-apps/api/core";
    import { revealItemInDir } from "@tauri-apps/plugin-opener";
    import { onMount } from "svelte";
    import { parseError } from "$lib/errors";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import {
        Folder01Icon,
        DashboardSquare01Icon,
        CloudUploadIcon,
        CloudDownloadIcon,
        PlayIcon,
        StopIcon,
        Delete02Icon,
    } from "@hugeicons/core-free-icons";
    import StatusRow from "./StatusRow.svelte";
    import { isSandboxOpen } from "$lib/workspace/state.svelte";
    import {
        translateStatus,
        syncColor,
        type Project,
        type Deployment,
    } from "$lib/sandbox";

    let {
        project,
        deployment,
        hosts,
        currentImageVersion,
        onChanged,
    }: {
        project: Project;
        deployment: Deployment | undefined;
        hosts: string[];
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

    // Whether this sandbox's host picker is open, and the host chosen within it.
    let picking = $state(false);
    let chosenHost = $state("");

    // The toggle is optimistic: Mutagen does not surface a "paused" status, so on/off is tracked
    // locally, defaulting on for a freshly listed deployment.
    let syncOn = $state(true);

    // Once this sandbox's workspace panes exist, package/agent management lives in the dashboard
    // pane (docs/features/responsive-panel/02_dev.md Phase D), so the row narrows to
    // Folder/Sync/Workspace/Destroy.
    const workspaceOpen = $derived(isSandboxOpen(project.name));

    function openPicker() {
        picking = true;
        chosenHost = hosts[0] ?? "";
    }

    async function deploy() {
        if (!chosenHost) return;
        if (await action.run(
            "deploy",
            () => invoke("deploy_sandbox", { name: project.name, host: chosenHost }),
            { message: m.projects_status_deploying({ host: chosenHost }) }
        )) {
            picking = false;
            onChanged();
        }
    }

    async function undeploy() {
        if (await action.run(
            "undeploy",
            () => invoke("undeploy_sandbox", { name: project.name }),
            { message: m.projects_status_undeploying() }
        )) {
            onChanged();
        }
    }

    async function destroyProject() {
        if (await action.run("destroy", async () => {
            try {
                await invoke("destroy_sandbox", { name: project.name, force: false });
            } catch (e) {
                const err = parseError(e);
                // The guard refuses to discard unsynced remote work; confirm, then force.
                if (err.kind === "UnsyncedChanges") {
                    if (!confirm(`${err.message}\n\nDestroy anyway and discard them?`)) return;
                    await invoke("destroy_sandbox", { name: project.name, force: true });
                } else {
                    throw e;
                }
            }
        })) {
            onChanged();
        }
    }

    async function toggleSync(on: boolean) {
        syncOn = on;
        const success = await action.run(
            "sync",
            () => invoke(on ? "enable_sync" : "disable_sync", { name: project.name }),
            { message: on ? m.projects_status_enabling_sync() : m.projects_status_disabling_sync() }
        );
        if (success) {
            onChanged();
        } else {
            syncOn = !on; // revert the optimistic flip
        }
    }

    async function start() {
        await action.run(
            "start",
            () => invoke("start_sandbox", { name: project.name }),
            { message: m.projects_status_starting() }
        );
    }

    async function stop() {
        await action.run(
            "stop",
            () => invoke("stop_sandbox", { name: project.name }),
            { message: m.projects_status_stopping() }
        );
    }

    // Primary action: the embedded multi-webview workspace.
    // Once this sandbox's panes already exist, just bring its last-active pane forward instead of
    // round-tripping through `open_workspace`'s own exists-guard (Resolved Decision 1 in
    // docs/features/responsive-panel/02_dev.md).
    async function openWorkspace() {
        await action.run("workspace", async () => {
            if (workspaceOpen) {
                await invoke("switch_app", { name: project.name });
            } else {
                await invoke("open_workspace", {
                    name: project.name,
                    workspacePath: project.workspace_path,
                });
            }
        }, { message: m.projects_status_opening_workspace() });
    }

    async function openFolder() {
        await action.run("folder", () => revealItemInDir(project.workspace_path));
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
        <!-- max-[420px]: matches main-navigation-panel's real rendered width once a workspace is open
             (NAVIGATION_PANEL_EXPANDED_W = 380, src-tauri/src/workspace/mod.rs:32); labels drop to icons. -->
        <span class="flex items-center gap-2 shrink-0">
            <Button variant="ghost" size="sm" onclick={openFolder} title={project.workspace_path}
                loading={action.busy === "folder"} disabled={action.busy !== null}>
                <HugeiconsIcon icon={Folder01Icon} size={16} />
                <span class="max-[420px]:hidden">{m.projects_folder()}</span>
            </Button>
            {#if deployment}
                <label class="flex items-center gap-1 text-xs">
                    <Switch checked={syncOn} onCheckedChange={toggleSync} disabled={action.busy !== null} />
                    <span class="max-[420px]:hidden">{m.projects_sync()}</span>
                </label>
                <Button variant="ghost" size="sm" onclick={openWorkspace} title={m.projects_workspace()}
                    loading={action.busy === "workspace"} disabled={action.busy !== null}>
                    <HugeiconsIcon icon={DashboardSquare01Icon} size={16} />
                    <span class="max-[420px]:hidden">{m.projects_workspace()}</span>
                </Button>
                <Button variant="ghost" size="sm" onclick={undeploy} title={m.projects_undeploy()}
                    loading={action.busy === "undeploy"} disabled={action.busy !== null}>
                    <HugeiconsIcon icon={CloudDownloadIcon} size={16} />
                    <span class="max-[420px]:hidden">{m.projects_undeploy()}</span>
                </Button>
            {:else}
                {#if project.status === "running"}
                    <Button variant="ghost" size="sm" onclick={stop} title={m.projects_stop()}
                        loading={action.busy === "stop"} disabled={action.busy !== null}>
                        <HugeiconsIcon icon={StopIcon} size={16} />
                        <span class="max-[420px]:hidden">{m.projects_stop()}</span>
                    </Button>
                {:else}
                    <Button variant="ghost" size="sm" onclick={start} title={m.projects_start()}
                        loading={action.busy === "start"} disabled={action.busy !== null}>
                        <HugeiconsIcon icon={PlayIcon} size={16} />
                        <span class="max-[420px]:hidden">{m.projects_start()}</span>
                    </Button>
                {/if}
                <Button variant="ghost" size="sm" onclick={openWorkspace} title={m.projects_workspace()}
                    loading={action.busy === "workspace"} disabled={action.busy !== null}>
                    <HugeiconsIcon icon={DashboardSquare01Icon} size={16} />
                    <span class="max-[420px]:hidden">{m.projects_workspace()}</span>
                </Button>
                <Button variant="ghost" size="sm" onclick={openPicker} title={m.projects_deploy()} disabled={action.busy !== null}>
                    <HugeiconsIcon icon={CloudUploadIcon} size={16} />
                    <span class="max-[420px]:hidden">{m.projects_deploy()}</span>
                </Button>
            {/if}
            <Button variant="destructive" size="sm" onclick={destroyProject} title={m.projects_destroy()}
                loading={action.busy === "destroy"} disabled={action.busy !== null}>
                <HugeiconsIcon icon={Delete02Icon} size={16} />
                <span class="max-[420px]:hidden">{m.projects_destroy()}</span>
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

    {#if picking}
        <div class="flex items-center gap-2">
            {#if hosts.length === 0}
                <span class="text-muted-foreground text-xs flex-1">{m.projects_no_hosts()}</span>
            {:else}
                <select bind:value={chosenHost} class="flex-1 rounded border bg-background px-2 py-1 text-xs">
                    {#each hosts as host}
                        <option value={host}>{host}</option>
                    {/each}
                </select>
                <Button size="sm" loading={action.busy === "deploy"} disabled={action.busy !== null} onclick={deploy}>
                    {m.projects_deploy_confirm()}
                </Button>
            {/if}
            <Button size="sm" variant="outline" disabled={action.busy !== null} onclick={() => (picking = false)}>
                {m.projects_cancel()}
            </Button>
        </div>
    {/if}
</li>
