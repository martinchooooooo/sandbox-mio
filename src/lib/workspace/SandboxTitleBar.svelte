<script lang="ts">
    import { openSandboxFolder } from "$lib/api/sandbox";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import {
        Cancel01Icon,
        Delete02Icon,
        Folder01Icon,
        MoreHorizontalIcon,
        PlayIcon,
        RefreshIcon,
        StopIcon,
    } from "@hugeicons/core-free-icons";
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Dialog from "$lib/components/ui/dialog/index.js";
    import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
    import { Text } from "$lib/components/ui/text";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import { statusDotClass, translateStatus } from "$lib/sandbox";
    import {
        lifecycleError,
        destroySandbox,
        lifecycleMessage,
        pendingOp,
        restartSandbox,
        startSandbox,
        stopSandbox,
    } from "$lib/sandbox/lifecycle.svelte";
    import { deploymentFor, liveStatus, refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
    import { forgetProxyStatus } from "$lib/proxy/status.svelte";
    import { forgetAgentAuth } from "$lib/agents/authStatus.svelte";
    import { forgetChat } from "$lib/agent-chat/chat.svelte";
    import { forgetSection } from "$lib/workspace/dashboard.svelte";
    import { closeSandbox } from "$lib/workspace/navigation.svelte";
    import HealthChip from "$lib/sandbox/HealthChip.svelte";
    import StatusRow from "$lib/sandbox/StatusRow.svelte";

    let { sandbox, dashboardBusy = false }: { sandbox: string; dashboardBusy?: boolean } = $props();

    // Missing for a sandbox created moments ago; the bar then shows only the name and close.
    const entry = $derived(sandboxesList().find((p) => p.name === sandbox));
    const deployment = $derived(deploymentFor(sandbox));
    const status = $derived(entry ? liveStatus(entry) : undefined);
    const action = createActionRunner();
    const pending = $derived(pendingOp(sandbox));
    const running = $derived(status === "running");
    const locked = $derived(action.busy !== null || pending !== undefined || dashboardBusy);

    let confirmingDelete = $state(false);

    async function onDestroy() {
        confirmingDelete = false;
        // TODO(i18n)
        const confirmForce = (message: string) => confirm(`${message}

Destroy anyway and discard them?`);
        if (await destroySandbox(sandbox, { confirmForce })) {
            // A recreated sandbox with the same name must not inherit the destroyed one's state.
            forgetSection(sandbox);
            forgetProxyStatus(sandbox);
            forgetAgentAuth(sandbox);
            forgetChat(sandbox);
            await refreshSandboxState();
            await closeSandbox(sandbox);
        }
    }

    async function openFolder() {
        await action.run("folder", () => openSandboxFolder(sandbox));
    }
</script>

<div data-sandbox-title-bar class="flex flex-wrap items-center gap-x-4 gap-y-2">
    <div class="flex min-w-0 items-center gap-3">
        <span class={["inline-block size-3 shrink-0 rounded-full", statusDotClass(status, deployment, pending)]}></span>
        <div class="min-w-0">
            <Text as="h1" class="truncate text-xl" title={sandbox}>{sandbox}</Text>
            <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
                <StatusRow
                    busy={pending ?? action.busy}
                    message={pending ? lifecycleMessage(pending) : action.message}
                    error={lifecycleError(sandbox) || action.error}
                    status={status === undefined ? "" : translateStatus(deployment ? deployment.status : status)}
                />
                {#if entry}
                    {#if running}
                        <Button variant="outline" size="sm" class="h-6 px-2 text-xs" data-sandbox-stop
                            onclick={() => stopSandbox(sandbox)} loading={pending === "stop"} disabled={locked}>
                            <HugeiconsIcon icon={StopIcon} size={12} />
                            {m.workspace_stop()}
                        </Button>
                    {:else}
                        <Button size="sm" class="h-6 px-2 text-xs" data-sandbox-start
                            onclick={() => startSandbox(sandbox)} loading={pending === "start"} disabled={locked}>
                            <HugeiconsIcon icon={PlayIcon} size={12} />
                            {m.workspace_start()}
                        </Button>
                    {/if}
                {/if}
            </div>
        </div>
        {#if running}
            <HealthChip {sandbox} restarting={pending !== undefined} onRestart={() => restartSandbox(sandbox)} />
        {/if}
    </div>

    <div class="ml-auto flex shrink-0 items-center gap-2">
        {#if entry}
            <Button variant="outline" size="sm" data-open-folder onclick={openFolder} title={entry.workspace_path}
                loading={action.busy === "folder"} disabled={locked}>
                <HugeiconsIcon icon={Folder01Icon} size={16} />
                {m.workspace_open_folder()}
            </Button>
        {/if}
        <DropdownMenu.Root>
            <DropdownMenu.Trigger>
                {#snippet child({ props })}
                    <Button {...props} variant="ghost" size="icon" data-sandbox-menu
                        aria-label={m.workspace_more_actions()} title={m.workspace_more_actions()}>
                        <HugeiconsIcon icon={MoreHorizontalIcon} size={16} />
                    </Button>
                {/snippet}
            </DropdownMenu.Trigger>
            <DropdownMenu.Content align="end">
                {#if entry && running}
                    <DropdownMenu.Item data-sandbox-restart disabled={locked} onSelect={() => restartSandbox(sandbox)}>
                        <HugeiconsIcon icon={RefreshIcon} size={16} />
                        {m.workspace_restart()}
                    </DropdownMenu.Item>
                {/if}
                <DropdownMenu.Item data-close-sandbox onSelect={() => closeSandbox(sandbox)}>
                    <HugeiconsIcon icon={Cancel01Icon} size={16} />
                    {m.workspace_close_sandbox_label({ sandbox })}
                </DropdownMenu.Item>
                {#if entry}
                    <DropdownMenu.Separator />
                    <DropdownMenu.Item variant="destructive" data-sandbox-destroy disabled={locked}
                        onSelect={() => (confirmingDelete = true)}>
                        <HugeiconsIcon icon={Delete02Icon} size={16} />
                        {m.workspace_delete_sandbox()}
                    </DropdownMenu.Item>
                {/if}
            </DropdownMenu.Content>
        </DropdownMenu.Root>
    </div>
</div>

<Dialog.Root bind:open={confirmingDelete}>
    <Dialog.Content data-destroy-dialog>
        <Dialog.Header>
            <Dialog.Title>{m.workspace_delete_title({ sandbox })}</Dialog.Title>
            <Dialog.Description>{m.workspace_delete_body()}</Dialog.Description>
        </Dialog.Header>
        <Dialog.Footer>
            <Button variant="outline" data-destroy-cancel onclick={() => (confirmingDelete = false)}>
                {m.projects_cancel()}
            </Button>
            <Button variant="destructive" data-destroy-confirm onclick={onDestroy} disabled={locked}>
                {m.workspace_delete_confirm()}
            </Button>
        </Dialog.Footer>
    </Dialog.Content>
</Dialog.Root>
