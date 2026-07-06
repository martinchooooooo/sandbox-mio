<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import StatusRow from "../../routes/projects/StatusRow.svelte";
    import PackageManager from "../../routes/projects/PackageManager.svelte";
    import AgentManager from "../../routes/projects/AgentManager.svelte";
    import PortsManager from "../../routes/projects/PortsManager.svelte";
    import type { Project } from "$lib/sandbox";
    import { LoaderPinwheelIcon } from "@hugeicons/core-free-icons";
    import { AGENTS, miseKey } from "$lib/agents/catalogue";
    import { LAUNCHABLE_APPS } from "$lib/workspace/apps";
    import { sandboxHasPane } from "$lib/workspace/state.svelte";
    import { packagesFor, ensurePackages } from "$lib/workspace/packages.svelte";

    // Rendered by `+layout.svelte` when no Terminal/AgentWeb overlay is active for the active
    // sandbox (docs/features/consolidate-app-container/02_dev.md) — no longer a separate embedded
    // pane with its own lifecycle.
    let { sandboxName }: { sandboxName: string } = $props();

    let workspacePath = $state("");
    const packages = $derived(packagesFor(sandboxName));
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
        // No optimistic client-side state here: the Dashboard hides itself once Rust confirms the
        // pane is active (`activeAppLabel()` becomes non-null via `workspace://state`), not before
        // — see the comment on `activeSandbox` in state.svelte.ts.
        await launcherAction.run(app.kind, async () => {
            await invoke(app.command, { name: sandboxName, workspacePath });
        });
    }

    onMount(async () => {
        const projects = await invoke<Project[]>("list_sandboxes");
        workspacePath = projects.find((p) => p.name === sandboxName)?.workspace_path ?? "";
    });

    $effect(() => {
        ensurePackages(sandboxName, workspacePath);
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
            {#if launcherAction.error}
                <p class="text-destructive -mt-4 text-xs">{launcherAction.error}</p>
            {/if}
            <PackageManager {sandboxName} {workspacePath} {action} />
            <AgentManager {sandboxName} {workspacePath} {action} />
            <PortsManager {sandboxName} {workspacePath} {action} />
        </div>
    {/if}
</main>
