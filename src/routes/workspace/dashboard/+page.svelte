<script lang="ts">
    import { page } from "$app/state";
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import StatusRow from "../../projects/StatusRow.svelte";
    import PackageManager from "../../projects/PackageManager.svelte";
    import AgentManager from "../../projects/AgentManager.svelte";
    import PortsManager from "../../projects/PortsManager.svelte";
    import type { Project } from "$lib/sandbox";
    import { LoaderPinwheelIcon } from "@hugeicons/core-free-icons";
    import { AGENTS, miseKey } from "$lib/agents/catalogue";
    import { LAUNCHABLE_APPS } from "$lib/workspace/apps";
    import { sandboxHasPane } from "$lib/workspace/state.svelte";

    // Content pane for `AppKind::Dashboard` (docs/features/responsive-panel/02_dev.md Phase D):
    // an internal SvelteKit route embedded as a second content webview alongside a sandbox's
    // terminal pane (src-tauri/src/workspace/mod.rs, embed_dashboard). Reads which sandbox it
    // belongs to from the query string set at pane-creation time.
    const sandboxName = $derived(page.url.searchParams.get("sandbox") ?? "");

    let workspacePath = $state("");
    let packages = $state<string[]>([]);
    const action = createActionRunner();
    const launcherAction = createActionRunner();

    // Same install predicate AgentManager.svelte computes, so the launcher and the Agents section
    // agree on when "Agents" is available.
    const installedAgents = $derived(
        AGENTS.filter((a) => a.misePackages.every((p) => packages.includes(miseKey(p)))),
    );
    const launchableApps = $derived(
        LAUNCHABLE_APPS.filter((app) => app.available({ installedAgents })),
    );

    async function launch(app: (typeof LAUNCHABLE_APPS)[number]) {
        await launcherAction.run(app.kind, async () => {
            await invoke(app.command, { name: sandboxName, workspacePath });
        });
    }

    onMount(async () => {
        const projects = await invoke<Project[]>("list_sandboxes");
        workspacePath = projects.find((p) => p.name === sandboxName)?.workspace_path ?? "";
        packages = await invoke<string[]>("list_packages", { workspacePath });
    });
</script>

<main class="h-screen overflow-y-auto p-6">
    <h1 class="text-xl font-medium mb-4">{sandboxName}</h1>

    <StatusRow busy={action.busy} message={action.message} error={action.error} />

    {#if workspacePath}
        <div class="flex flex-col gap-6 max-w-2xl">
            <div class="flex flex-wrap items-center gap-2 border-b pb-4 text-xs">
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
            <PackageManager {sandboxName} {workspacePath} {action} />
            <AgentManager {sandboxName} {workspacePath} {action} />
            <PortsManager {sandboxName} {workspacePath} {action} />
        </div>
    {/if}
</main>
