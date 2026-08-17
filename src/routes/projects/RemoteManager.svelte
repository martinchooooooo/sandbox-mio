<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Switch } from "$lib/components/ui/switch";
    import { invoke } from "@tauri-apps/api/core";
    import { m } from "$lib/paraglide/messages.js";
    import type { createActionRunner } from "$lib/actions.svelte";
    import { translateStatus, syncColor } from "$lib/sandbox";
    import type { ProviderId } from "$lib/providers";
    import {
        deploymentFor,
        hostsList,
        capabilitiesFor,
        refreshSandboxState,
    } from "$lib/sandbox/state.svelte";

    let {
        sandboxName,
        workspacePath,
        provider,
        action,
    }: {
        sandboxName: string;
        workspacePath: string;
        provider: ProviderId;
        action: ReturnType<typeof createActionRunner>;
    } = $props();

    const deployment = $derived(deploymentFor(sandboxName));

    // Hidden rather than disabled for a provider with no remote-target concept: there is nothing
    // the user could do to make it available (`docs/features/microsandbox-provider/02_dev.md`,
    // Phase J).
    const canDeploy = $derived(capabilitiesFor(provider)?.remoteTargets ?? false);

    // Whether the host picker is open, and the host chosen within it.
    let picking = $state(false);
    let chosenHost = $state("");

    // The toggle is optimistic: Mutagen does not surface a "paused" status, so on/off is tracked
    // locally, defaulting on for a freshly listed deployment.
    let syncOn = $state(true);

    function openPicker() {
        picking = true;
        chosenHost = hostsList()[0] ?? "";
    }

    async function deploy() {
        if (!chosenHost) return;
        if (await action.run(
            "deploy",
            () => invoke("deploy_sandbox", { name: sandboxName, host: chosenHost }),
            { message: m.projects_status_deploying({ host: chosenHost }) }
        )) {
            picking = false;
            await refreshSandboxState();
        }
    }

    async function undeploy() {
        if (await action.run(
            "undeploy",
            () => invoke("undeploy_sandbox", { name: sandboxName }),
            { message: m.projects_status_undeploying() }
        )) {
            await refreshSandboxState();
        }
    }

    async function toggleSync(on: boolean) {
        syncOn = on;
        const success = await action.run(
            "sync",
            () => invoke(on ? "enable_sync" : "disable_sync", { name: sandboxName }),
            { message: on ? m.projects_status_enabling_sync() : m.projects_status_disabling_sync() }
        );
        if (success) {
            await refreshSandboxState();
        } else {
            syncOn = !on; // revert the optimistic flip
        }
    }
</script>

<div class="flex flex-col gap-1.5 border-t pt-2">
    <span class="text-muted-foreground text-xs shrink-0">{m.projects_remote_label()}</span>
    {#if deployment}
        <div class="flex items-center justify-between gap-2 text-xs">
            <span class="flex items-center gap-2">
                <span class={["inline-block h-2 w-2 rounded-full shrink-0", syncColor(deployment.status)].join(" ")}></span>
                {deployment.host} · {translateStatus(deployment.status)}
            </span>
            <span class="flex items-center gap-2">
                <label class="flex items-center gap-1">
                    <Switch checked={syncOn} onCheckedChange={toggleSync} disabled={action.busy !== null} />
                    {m.projects_sync()}
                </label>
                <Button variant="ghost" size="sm" onclick={undeploy} title={m.projects_undeploy()}
                    loading={action.busy === "undeploy"} disabled={action.busy !== null}>
                    {m.projects_undeploy()}
                </Button>
            </span>
        </div>
    {:else if canDeploy}
        {#if picking}
            <div class="flex items-center gap-2">
                {#if hostsList().length === 0}
                    <span class="text-muted-foreground text-xs flex-1">{m.projects_no_hosts()}</span>
                {:else}
                    <select bind:value={chosenHost} class="flex-1 rounded border bg-background px-2 py-1 text-xs">
                        {#each hostsList() as host}
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
        {:else}
            <Button variant="ghost" size="sm" onclick={openPicker} disabled={action.busy !== null}>
                {m.projects_deploy()}
            </Button>
        {/if}
    {:else}
        <span class="text-muted-foreground text-xs">{m.projects_no_remote_target()}</span>
    {/if}
</div>
