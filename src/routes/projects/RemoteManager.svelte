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
    // the user could do to make it available.
    const canDeploy = $derived(capabilitiesFor(provider)?.remoteTargets ?? false);

    // The toggle is optimistic: Mutagen does not surface a "paused" status, so on/off is tracked
    // locally, defaulting on for a freshly listed deployment.
    let syncOn = $state(true);

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

<div class="flex flex-col gap-1.5">
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
        <!-- Every sandbox has a proxy sidecar, which has no remote counterpart yet
             (docs/features/proxy-remote-deploy/feature.md). -->
        <div class="flex items-center gap-2" data-remote-deploy-unavailable>
            <Button variant="ghost" size="sm" disabled>{m.projects_deploy()}</Button>
            <span class="text-muted-foreground text-xs">Not available yet. <!-- TODO(i18n) --></span>
        </div>
    {:else}
        <span class="text-muted-foreground text-xs">{m.projects_no_remote_target()}</span>
    {/if}
</div>
