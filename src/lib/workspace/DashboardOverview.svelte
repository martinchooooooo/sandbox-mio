<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { LoaderPinwheelIcon } from "@hugeicons/core-free-icons";
    import Button from "$lib/components/ui/button/button.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import { AGENTS, miseKey } from "$lib/agents/catalogue";
    import { LAUNCHABLE_APPS, type AppDescriptor } from "$lib/workspace/apps";
    import { panesOf, sandboxHasPane } from "$lib/workspace/state.svelte";
    import { closePane, launchApp } from "$lib/workspace/navigation.svelte";
    import { packagesFor } from "$lib/workspace/packages.svelte";
    import { liveStatus, sandboxesList } from "$lib/sandbox/state.svelte";
    import AgentManager from "../../routes/projects/AgentManager.svelte";
    import StatusSummary from "./StatusSummary.svelte";
    import GithubAccess from "./GithubAccess.svelte";
    import type { DashboardSectionId } from "./sections";

    let {
        sandboxName,
        workspacePath,
        action,
        showRemote,
        onOpenSection,
    }: {
        sandboxName: string;
        workspacePath: string;
        action: ReturnType<typeof createActionRunner>;
        showRemote: boolean;
        onOpenSection: (id: DashboardSectionId) => void;
    } = $props();

    const launcherAction = createActionRunner();
    const packages = $derived(packagesFor(sandboxName));

    // Same install predicate AgentManager.svelte computes, so the cards and the agent list
    // agree on when "Agents" is available.
    const installedAgents = $derived(
        AGENTS.filter((a) => a.misePackages.every((p) => packages.includes(miseKey(p)))),
    );
    // Start Paseo before the user asks for it. Both inputs are booleans, so the effect re-runs only
    // when the sandbox starts or gains its first agent, e.g. right after an install.
    const entry = $derived(sandboxesList().find((p) => p.name === sandboxName));
    const running = $derived(entry ? liveStatus(entry) === "running" : false);
    const hasAgent = $derived(installedAgents.length > 0);
    $effect(() => {
        if (running && hasAgent) {
            // Fire and forget: the backend logs a failed pre-warm.
            invoke("prewarm_agent_web", { name: sandboxName }).catch(() => {});
        }
    });
    // An open pane stays usable so it can still be switched to and closed once its agent is gone.
    const apps = $derived(
        LAUNCHABLE_APPS.map((app) => ({
            app,
            usable: app.available({ installedAgents }) || sandboxHasPane(sandboxName, app.kind),
        })),
    );

    function launch(app: AppDescriptor) {
        return launcherAction.run(app.kind, () => launchApp(sandboxName, workspacePath, app));
    }

    function close(app: AppDescriptor) {
        const pane = panesOf(sandboxName).find((p) => p.kind === app.kind);
        if (pane) return closePane(pane);
    }
</script>

<div class="stagger flex flex-col gap-4">
    <div data-app-cards class="@container">
        <div class="grid grid-cols-1 gap-4 @xl:grid-cols-2 @2xl:grid-cols-3">
            {#each apps as { app, usable } (app.kind)}
                {@const open = sandboxHasPane(sandboxName, app.kind)}
                {@const busy = launcherAction.busy === app.kind}
                <div class="relative flex">
                    <button
                        type="button"
                        data-app={app.kind}
                        data-open={open}
                        disabled={!usable || launcherAction.busy !== null}
                        onclick={() => launch(app)}
                        class={[
                            "bg-card text-card-foreground flex w-full cursor-pointer flex-col items-start gap-3 rounded-md border-2 p-4 text-left shadow-md transition-all",
                            "hover:translate-y-1 hover:shadow-none focus-visible:translate-y-1 focus-visible:shadow-none focus-visible:outline-hidden",
                            "disabled:cursor-not-allowed disabled:opacity-60 disabled:shadow-none disabled:hover:translate-y-0",
                            open && "border-primary",
                        ]}
                    >
                        <span class="bg-muted flex size-12 items-center justify-center rounded-md border-2">
                            <HugeiconsIcon
                                icon={busy ? LoaderPinwheelIcon : app.icon}
                                size={28}
                                class={busy ? "animate-spin" : ""}
                            />
                        </span>
                        <span class="flex flex-col gap-1">
                            <span class="font-head text-base">{app.title()}</span>
                            <span class="text-muted-foreground text-sm">{app.description()}</span>
                        </span>
                        <span class={["mt-auto text-xs font-semibold", usable ? "text-foreground" : "text-muted-foreground"]}>
                            {#if !usable}
                                {app.unavailableHint()}
                            {:else if open}
                                {m.workspace_launcher_switch()} →
                            {:else}
                                {m.workspace_launcher_open()} →
                            {/if}
                        </span>
                    </button>
                    {#if open}
                        <Button
                            variant="outline"
                            size="sm"
                            class="absolute top-2 right-2 h-7 bg-card"
                            data-close-app={app.kind}
                            aria-label={m.workspace_close_tab_label({ title: app.title() })}
                            onclick={() => close(app)}
                        >
                            {m.workspace_launcher_close()}
                        </Button>
                    {/if}
                </div>
            {/each}
        </div>
    </div>
    {#if launcherAction.error}
        <p class="text-destructive -mt-2 text-xs">{launcherAction.error}</p>
    {/if}
    <StatusSummary {sandboxName} {showRemote} onOpen={onOpenSection} />
    <AgentManager {sandboxName} {workspacePath} {action} />
    <GithubAccess {sandboxName} />
</div>
