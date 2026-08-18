<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { revealItemInDir } from "@tauri-apps/plugin-opener";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import { parseError } from "$lib/errors";
    import StatusRow from "../../routes/projects/StatusRow.svelte";
    import PackageManager from "../../routes/projects/PackageManager.svelte";
    import AgentManager from "../../routes/projects/AgentManager.svelte";
    import PortsManager from "../../routes/projects/PortsManager.svelte";
    import RemoteManager from "../../routes/projects/RemoteManager.svelte";
    import ProxyPanel from "../../routes/projects/ProxyPanel.svelte";
    import TrafficPanel from "../../routes/projects/TrafficPanel.svelte";
    import type { Project } from "$lib/sandbox";
    import type { ProviderId } from "$lib/providers";
    import { LoaderPinwheelIcon, Folder01Icon, Delete02Icon } from "@hugeicons/core-free-icons";
    import { AGENTS, miseKey } from "$lib/agents/catalogue";
    import { LAUNCHABLE_APPS } from "$lib/workspace/apps";
    import { sandboxHasPane, clearActiveSandbox } from "$lib/workspace/state.svelte";
    import { packagesFor, ensurePackages } from "$lib/workspace/packages.svelte";
    import { ensureSandboxRemoteState, refreshSandboxState } from "$lib/sandbox/state.svelte";

    // Rendered by `+layout.svelte` when no Terminal/AgentWeb overlay is active for the active
    // sandbox (docs/features/consolidate-app-container/02_dev.md) — no longer a separate embedded
    // pane with its own lifecycle.
    let { sandboxName }: { sandboxName: string } = $props();

    let workspacePath = $state("");
    let provider = $state<ProviderId | undefined>(undefined);
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

    // A sandbox with no open Terminal/AgentWeb pane has no `SandboxSwitcherGroup` (WorkspaceSwitcher.svelte
    // only groups by open panes), so its × close button doesn't exist there either — this is the
    // only remaining way to close such a sandbox. `close_workspace` is a no-op here (no panes to
    // tear down), so it's `clearActiveSandbox` that actually drops this Dashboard.
    async function close() {
        try {
            await invoke("close_workspace", { name: sandboxName });
        } catch (e) {
            console.error(e);
        }
        clearActiveSandbox();
    }

    async function openFolder() {
        await action.run("folder", () => revealItemInDir(workspacePath));
    }

    async function destroySandbox() {
        if (await action.run("destroy", async () => {
            try {
                await invoke("destroy_sandbox", { name: sandboxName, force: false });
            } catch (e) {
                const err = parseError(e);
                // The guard refuses to discard unsynced remote work; confirm, then force.
                if (err.kind === "UnsyncedChanges") {
                    if (!confirm(`${err.message}\n\nDestroy anyway and discard them?`)) return;
                    await invoke("destroy_sandbox", { name: sandboxName, force: true });
                } else {
                    throw e;
                }
            }
        })) {
            await refreshSandboxState();
            await close();
        }
    }

    onMount(async () => {
        const projects = await invoke<Project[]>("list_sandboxes");
        const proj = projects.find((p) => p.name === sandboxName);
        workspacePath = proj?.workspace_path ?? "";
        provider = proj?.provider;
        ensureSandboxRemoteState();
    });

    $effect(() => {
        ensurePackages(sandboxName, workspacePath);
    });
</script>

<main data-dashboard={sandboxName} class="h-screen overflow-y-auto p-6">
    <div class="mb-4 flex items-center justify-between gap-2">
        <h1 class="text-xl font-medium">{sandboxName}</h1>
        <span class="flex items-center gap-2">
            <Button variant="ghost" size="sm" onclick={openFolder} title={workspacePath}
                loading={action.busy === "folder"} disabled={action.busy !== null}>
                <HugeiconsIcon icon={Folder01Icon} size={16} />
                {m.projects_folder()}
            </Button>
            <button
                class="text-muted-foreground hover:text-foreground"
                data-close-sandbox
                aria-label={m.workspace_close_sandbox_label({ sandbox: sandboxName })}
                onclick={close}
            >
                ×
            </button>
        </span>
    </div>

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
            {#if provider}
                <RemoteManager {sandboxName} {workspacePath} {provider} {action} />
            {/if}
            <ProxyPanel {sandboxName} {workspacePath} {action} />
            <TrafficPanel {sandboxName} {workspacePath} {action} />

            <div class="flex justify-end border-t pt-4">
                <Button variant="destructive" size="sm" onclick={destroySandbox} title={m.projects_destroy()}
                    loading={action.busy === "destroy"} disabled={action.busy !== null}>
                    <HugeiconsIcon icon={Delete02Icon} size={16} />
                    {m.projects_destroy()}
                </Button>
            </div>
        </div>
    {/if}
</main>
