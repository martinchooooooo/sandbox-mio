<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { LoaderPinwheelIcon } from "@hugeicons/core-free-icons";
    import Button from "$lib/components/ui/button/button.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import { AGENTS, miseKey } from "$lib/agents/catalogue";
    import { LAUNCHABLE_APPS } from "$lib/workspace/apps";
    import { sandboxHasPane } from "$lib/workspace/state.svelte";
    import { packagesFor } from "$lib/workspace/packages.svelte";
    import AgentManager from "../../routes/projects/AgentManager.svelte";
    import StatusSummary from "./StatusSummary.svelte";
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

    // Same install predicate AgentManager.svelte computes, so the launcher and the agent list
    // agree on when "Agents" is available.
    const installedAgents = $derived(
        AGENTS.filter((a) => a.misePackages.every((p) => packages.includes(miseKey(p)))),
    );
    const launchableApps = $derived(
        LAUNCHABLE_APPS.filter((app) => app.available({ installedAgents })),
    );

    async function launch(app: (typeof LAUNCHABLE_APPS)[number]) {
        // No optimistic client-side state here: the Dashboard hides itself once Rust confirms the
        // pane is active (`activeAppLabel()` becomes non-null via `workspace://state`), not before
        // — see the comment on `activeSandbox` in state.svelte.ts.
        await launcherAction.run(app.kind, async () => {
            await invoke(app.command, { name: sandboxName, workspacePath });
        });
    }
</script>

<div class="stagger flex flex-col gap-4">
    <div class="flex flex-wrap items-center gap-2 text-xs">
        {#each launchableApps as app (app.kind)}
            {@const open = sandboxHasPane(sandboxName, app.kind)}
            {@const busy = launcherAction.busy === app.kind}
            <Button
                variant="outline"
                size="sm"
                loading={busy}
                disabled={launcherAction.busy !== null}
                data-app={app.kind}
                onclick={() => launch(app)}
            >
                <HugeiconsIcon
                    icon={busy ? LoaderPinwheelIcon : app.icon}
                    size={14}
                    class={busy ? "animate-spin" : ""}
                />
                {app.title()}
                <span class="text-muted-foreground">
                    · {open ? m.workspace_launcher_switch() : m.workspace_launcher_open()}
                </span>
            </Button>
        {/each}
    </div>
    {#if launcherAction.error}
        <p class="text-destructive -mt-2 text-xs">{launcherAction.error}</p>
    {/if}
    <StatusSummary {sandboxName} {showRemote} onOpen={onOpenSection} />
    <AgentManager {sandboxName} {workspacePath} {action} />
</div>
