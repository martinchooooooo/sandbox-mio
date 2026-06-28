<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Switch } from "$lib/components/ui/switch";
    import { invoke } from "@tauri-apps/api/core";
    import { revealItemInDir } from "@tauri-apps/plugin-opener";
    import { onMount } from "svelte";
    import { parseError } from "$lib/errors";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import StatusRow from "./StatusRow.svelte";
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

    let packages = $state<string[]>([]);
    let packageInput = $state("");

    // Whether this sandbox's host picker is open, and the host chosen within it.
    let picking = $state(false);
    let chosenHost = $state("");

    // The toggle is optimistic: Mutagen does not surface a "paused" status, so on/off is tracked
    // locally, defaulting on for a freshly listed deployment.
    let syncOn = $state(true);

    // Reads from the host manifest, so no polling needed — only install/uninstall change it.
    async function loadPackages() {
        await action.run("load", async () => {
            packages = await invoke<string[]>("list_packages", {
                workspacePath: project.workspace_path,
            });
        });
    }

    async function installPackage() {
        const pkg = packageInput.trim();
        if (!pkg) return;
        if (await action.run(
            "install",
            () => invoke("install_package", { name: project.name, package: pkg }),
            { message: m.projects_status_installing({ name: pkg }) }
        )) {
            packageInput = "";
            await loadPackages();
        }
    }

    async function uninstallPackage(pkg: string) {
        if (await action.run(
            "uninstall",
            () => invoke("uninstall_package", { name: project.name, package: pkg }),
            { message: m.projects_status_uninstalling({ name: pkg }) }
        )) {
            await loadPackages();
        }
    }

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

    async function openTerminal() {
        await action.run("terminal", async () => {
            await invoke("open_terminal", {
                name: project.name,
                workspacePath: project.workspace_path,
            });
        }, { message: m.projects_status_opening_terminal() });
    }

    async function openWorkspace() {
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
        await loadPackages();
        dismissedVersion = await invoke<number | undefined>("get_dismissed_image_version", {
            workspacePath: project.workspace_path,
        }) ?? undefined;
    });
</script>

<li class="flex flex-col gap-2 rounded-lg border p-3 text-sm">
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
        <span class="flex items-center gap-2 shrink-0">
            <Button variant="ghost" size="sm" onclick={openWorkspace} title={project.workspace_path}
                loading={action.busy === "folder"} disabled={action.busy !== null}>
                {m.projects_folder()}
            </Button>
            {#if deployment}
                <label class="flex items-center gap-1 text-xs">
                    <Switch checked={syncOn} onCheckedChange={toggleSync} disabled={action.busy !== null} />
                    {m.projects_sync()}
                </label>
                <Button variant="ghost" size="sm" onclick={openTerminal}
                    loading={action.busy === "terminal"} disabled={action.busy !== null}>
                    {m.projects_terminal()}
                </Button>
                <Button variant="ghost" size="sm" onclick={undeploy}
                    loading={action.busy === "undeploy"} disabled={action.busy !== null}>
                    {m.projects_undeploy()}
                </Button>
            {:else}
                {#if project.status === "running"}
                    <Button variant="ghost" size="sm" onclick={stop}
                        loading={action.busy === "stop"} disabled={action.busy !== null}>
                        {m.projects_stop()}
                    </Button>
                {:else}
                    <Button variant="ghost" size="sm" onclick={start}
                        loading={action.busy === "start"} disabled={action.busy !== null}>
                        {m.projects_start()}
                    </Button>
                {/if}
                <Button variant="ghost" size="sm" onclick={openTerminal}
                    loading={action.busy === "terminal"} disabled={action.busy !== null}>
                    {m.projects_terminal()}
                </Button>
                <Button variant="ghost" size="sm" onclick={openPicker} disabled={action.busy !== null}>
                    {m.projects_deploy()}
                </Button>
            {/if}
            <Button variant="destructive" size="sm" onclick={destroyProject}
                loading={action.busy === "destroy"} disabled={action.busy !== null}>
                {m.projects_destroy()}
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

    <div class="flex flex-col gap-1.5 border-t pt-2">
        <div class="flex items-center gap-2">
            <span class="text-muted-foreground text-xs shrink-0">{m.projects_packages()}</span>
            <input
                class="flex-1 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.projects_package_placeholder()}
                bind:value={packageInput}
                disabled={action.busy !== null}
                onkeydown={(e) => e.key === "Enter" && installPackage()}
            />
            <Button size="sm" loading={action.busy === "install"} disabled={action.busy !== null} onclick={installPackage}>
                {m.projects_install()}
            </Button>
        </div>
        {#if packages.length > 0}
            <!-- <span> not <li>: nested <li> breaks WebView2's `li*=name` WebDriver lookup. -->
            <div class="flex flex-wrap gap-1.5">
                {#each packages as pkg}
                    <span class="flex items-center gap-1 rounded bg-muted px-2 py-0.5 text-xs">
                        <span class="font-mono">{pkg}</span>
                        <Button variant="ghost" size="icon-xs"
                            loading={action.busy === "uninstall"}
                            disabled={action.busy !== null}
                            title={m.projects_uninstall()}
                            onclick={() => uninstallPackage(pkg)}>✕</Button>
                    </span>
                {/each}
            </div>
        {:else}
            <span class="text-muted-foreground text-xs">{m.projects_no_packages()}</span>
        {/if}
    </div>
</li>
