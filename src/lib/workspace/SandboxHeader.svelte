<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { revealItemInDir } from "@tauri-apps/plugin-opener";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { Cancel01Icon, Folder01Icon, PlayIcon, StopIcon } from "@hugeicons/core-free-icons";
    import Button from "$lib/components/ui/button/button.svelte";
    import { navItemBase } from "$lib/components/ui/section-nav/styles";
    import { m } from "$lib/paraglide/messages.js";
    import { cn } from "$lib/utils";
    import { createActionRunner } from "$lib/actions.svelte";
    import { containerStatusFor } from "$lib/activity/activity.svelte";
    import { statusDotClass, translateStatus } from "$lib/sandbox";
    import { deploymentFor, refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
    import { HOME_APP, LAUNCHABLE_APPS } from "$lib/workspace/apps";
    import { activeAppLabel, panesOf } from "$lib/workspace/state.svelte";
    import { activatePane, closePane, closeSandbox, showManage } from "$lib/workspace/navigation.svelte";
    import StatusRow from "../../routes/projects/StatusRow.svelte";

    let { sandbox }: { sandbox: string } = $props();

    // Missing for a sandbox created moments ago or a pane opened by command; the header then shows
    // only the name, tabs and close.
    const entry = $derived(sandboxesList().find((p) => p.name === sandbox));
    const deployment = $derived(deploymentFor(sandbox));
    const status = $derived(entry ? (containerStatusFor(sandbox) ?? entry.status) : undefined);
    const panes = $derived(panesOf(sandbox));
    const tabs = $derived(
        LAUNCHABLE_APPS.flatMap((app) => {
            const pane = panes.find((p) => p.kind === app.kind);
            return pane ? [{ app, pane }] : [];
        }),
    );
    const manageActive = $derived(activeAppLabel() === null);
    const action = createActionRunner();

    const tabClass = cn(navItemBase, "shrink-0 px-2.5");
    const activeTabClass = "border-border bg-primary text-primary-foreground font-semibold";

    async function start() {
        if (await action.run("start", () => invoke("start_sandbox", { name: sandbox }), {
            message: m.projects_status_starting(),
        })) {
            await refreshSandboxState();
        }
    }

    async function stop() {
        if (await action.run("stop", () => invoke("stop_sandbox", { name: sandbox }), {
            message: m.projects_status_stopping(),
        })) {
            await refreshSandboxState();
        }
    }

    async function openFolder() {
        if (entry) await action.run("folder", () => revealItemInDir(entry.workspace_path));
    }
</script>

<!-- h-16 = 64px must equal SWITCHER_H (src-tauri/src/workspace/mod.rs): Rust places the
     Terminal/Agents pane webviews that far from the top, so a drift leaves a strip or cuts into
     this header. The root stays a `div`: e2e page objects key on `div[data-sandbox]`. -->
<div
    data-sandbox={sandbox}
    data-sandbox-header
    class="@container/header flex h-16 shrink-0 items-center gap-3 border-b-2 bg-background px-4"
>
    <div class="flex w-44 min-w-0 shrink items-center gap-2">
        <span class={["inline-block size-2.5 shrink-0 rounded-full", statusDotClass(status, deployment)]}></span>
        <div class="min-w-0">
            <h1 class="truncate font-head text-base leading-tight" title={sandbox}>{sandbox}</h1>
            <div class="[&>div]:truncate">
                <StatusRow
                    busy={action.busy}
                    message={action.message}
                    error={action.error}
                    status={status === undefined ? "" : translateStatus(deployment ? deployment.status : status)}
                />
            </div>
        </div>
    </div>

    <div
        role="group"
        aria-label={m.sandbox_header_apps_label()}
        class="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto py-1"
    >
        <button
            type="button"
            data-app="dashboard"
            aria-current={manageActive ? "page" : undefined}
            class={cn(tabClass, manageActive ? activeTabClass : "hover:border-border")}
            onclick={() => showManage(sandbox)}
        >
            <HugeiconsIcon icon={HOME_APP.icon} size={16} />
            {HOME_APP.title()}
        </button>
        {#each tabs as { app, pane } (pane.label)}
            {@const active = pane.label === activeAppLabel()}
            <div class={cn(tabClass, "gap-0 pr-0", active ? activeTabClass : "hover:border-border")}>
                <button
                    type="button"
                    data-app={app.kind}
                    aria-current={active ? "page" : undefined}
                    class="flex h-full items-center gap-2"
                    onclick={() => activatePane(pane)}
                >
                    <HugeiconsIcon icon={app.icon} size={16} />
                    {app.title()}
                </button>
                <button
                    type="button"
                    data-close-tab={app.kind}
                    aria-label={m.workspace_close_tab_label({ title: app.title() })}
                    title={m.workspace_close_tab_label({ title: app.title() })}
                    class="flex h-full items-center px-2 opacity-70 hover:opacity-100"
                    onclick={() => closePane(pane)}
                >
                    <HugeiconsIcon icon={Cancel01Icon} size={12} />
                </button>
            </div>
        {/each}
    </div>

    <div class="flex shrink-0 items-center gap-1">
        {#if entry}
            {#if status === "running"}
                <Button variant="ghost" size="sm" onclick={stop} title={m.projects_stop()}
                    loading={action.busy === "stop"} disabled={action.busy !== null}>
                    <HugeiconsIcon icon={StopIcon} size={16} />
                    <span class="@max-[44rem]/header:hidden">{m.projects_stop()}</span>
                </Button>
            {:else}
                <Button variant="ghost" size="sm" onclick={start} title={m.projects_start()}
                    loading={action.busy === "start"} disabled={action.busy !== null}>
                    <HugeiconsIcon icon={PlayIcon} size={16} />
                    <span class="@max-[44rem]/header:hidden">{m.projects_start()}</span>
                </Button>
            {/if}
            <Button variant="ghost" size="sm" onclick={openFolder} title={entry.workspace_path}
                loading={action.busy === "folder"} disabled={action.busy !== null}>
                <HugeiconsIcon icon={Folder01Icon} size={16} />
                <span class="@max-[44rem]/header:hidden">{m.projects_folder()}</span>
            </Button>
        {/if}
        <Button
            variant="ghost"
            size="icon"
            data-close-sandbox
            aria-label={m.workspace_close_sandbox_label({ sandbox })}
            title={m.workspace_close_sandbox_label({ sandbox })}
            onclick={() => closeSandbox(sandbox)}
        >
            <HugeiconsIcon icon={Cancel01Icon} size={16} />
        </Button>
    </div>
</div>
