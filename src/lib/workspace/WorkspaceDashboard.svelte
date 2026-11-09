<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { revealItemInDir } from "@tauri-apps/plugin-opener";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import * as SectionNav from "$lib/components/ui/section-nav";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import { parseError } from "$lib/errors";
    import StatusRow from "../../routes/projects/StatusRow.svelte";
    import PackageManager from "../../routes/projects/PackageManager.svelte";
    import PortsManager from "../../routes/projects/PortsManager.svelte";
    import RemoteManager from "../../routes/projects/RemoteManager.svelte";
    import ProxyPanel from "../../routes/projects/ProxyPanel.svelte";
    import TrafficPanel from "../../routes/projects/TrafficPanel.svelte";
    import { Folder01Icon, Delete02Icon } from "@hugeicons/core-free-icons";
    import { clearActiveSandbox } from "$lib/workspace/state.svelte";
    import { ensurePackages } from "$lib/workspace/packages.svelte";
    import { ensureSandboxRemoteState, refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
    import { forgetProxyStatus } from "$lib/proxy/status.svelte";
    import { forgetAgentAuth } from "$lib/agents/authStatus.svelte";
    import { forgetSection, sectionFor, setSection } from "$lib/workspace/dashboard.svelte";
    import { DASHBOARD_SECTIONS, type DashboardSectionId } from "$lib/workspace/sections";
    import DashboardOverview from "$lib/workspace/DashboardOverview.svelte";

    // Rendered by `+layout.svelte` when no Terminal/AgentWeb overlay is active for the active
    // sandbox (docs/features/consolidate-app-container/02_dev.md) — no longer a separate embedded
    // pane with its own lifecycle.
    let { sandboxName }: { sandboxName: string } = $props();

    // Two primitive deriveds: the store's poll replaces every Project object, so effects must not
    // depend on the entry itself.
    const entry = $derived(sandboxesList().find((p) => p.name === sandboxName));
    const workspacePath = $derived(entry?.workspace_path ?? "");
    const provider = $derived(entry?.provider);
    const action = createActionRunner();

    const sections = $derived(DASHBOARD_SECTIONS.filter((s) => s.visible({ provider })));
    // A remembered section that is not visible yet (capabilities still loading) shows Overview
    // without losing the memory (docs/features/manage-dashboard-redesign/02_dev.md, Resolved Decision 16).
    const activeId = $derived.by(() => {
        const remembered = sectionFor(sandboxName);
        return sections.some((s) => s.id === remembered) ? remembered : "overview";
    });
    const showRemote = $derived(sections.some((s) => s.id === "remote"));

    function openSection(id: DashboardSectionId) {
        setSection(sandboxName, id);
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
            // A recreated sandbox with the same name must not inherit the destroyed one's state.
            forgetSection(sandboxName);
            forgetProxyStatus(sandboxName);
            forgetAgentAuth(sandboxName);
            await refreshSandboxState();
            await close();
        }
    }

    onMount(async () => {
        await ensureSandboxRemoteState();
        // A sandbox created moments ago is not in the store until its next poll.
        if (!entry) await refreshSandboxState();
    });

    $effect(() => {
        ensurePackages(sandboxName, workspacePath);
    });
</script>

<!-- Fade only: on first open the workspace column already slides in (+layout.svelte). -->
<main data-dashboard={sandboxName} class="animate-fade-in flex h-full flex-col gap-3 p-6">
    <div class="flex items-center justify-between gap-2">
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
        <SectionNav.Root value={activeId} onValueChange={(v) => openSection(v as DashboardSectionId)}
            class="flex-1">
            <SectionNav.List aria-label="Sections"><!-- TODO(i18n) -->
                {#each sections as section (section.id)}
                    {#if section.separated}
                        <SectionNav.Separator />
                    {/if}
                    <SectionNav.Item value={section.id} label={section.label()}>
                        {#snippet icon()}
                            <HugeiconsIcon icon={section.icon} size={16} />
                        {/snippet}
                    </SectionNav.Item>
                {/each}
            </SectionNav.List>

            {#each sections as section (section.id)}
                <SectionNav.Content value={section.id}>
                    <div class="flex max-w-2xl flex-col gap-4 pb-4 pr-1">
                        {#if section.id === "overview"}
                            <DashboardOverview {sandboxName} {workspacePath} {action} {showRemote}
                                onOpenSection={openSection} />
                        {:else if section.id === "tools"}
                            <PackageManager {sandboxName} {workspacePath} {action} />
                        {:else if section.id === "ports"}
                            <PortsManager {sandboxName} {workspacePath} {action} />
                        {:else if section.id === "network"}
                            <ProxyPanel {sandboxName} {workspacePath} {provider} {action} />
                        {:else if section.id === "traffic"}
                            <TrafficPanel {sandboxName} {workspacePath} {provider} {action} />
                        {:else if section.id === "remote" && provider}
                            <RemoteManager {sandboxName} {workspacePath} {provider} {action} />
                        {:else if section.id === "danger"}
                            <div class="flex flex-col items-start gap-2">
                                <span class="text-muted-foreground text-xs">{section.label()}</span>
                                <Button variant="destructive" size="sm" onclick={destroySandbox}
                                    title={m.projects_destroy()}
                                    loading={action.busy === "destroy"} disabled={action.busy !== null}>
                                    <HugeiconsIcon icon={Delete02Icon} size={16} />
                                    {m.projects_destroy()}
                                </Button>
                            </div>
                        {/if}
                    </div>
                </SectionNav.Content>
            {/each}
        </SectionNav.Root>
    {:else}
        <p class="text-muted-foreground text-xs">Loading…<!-- TODO(i18n) --></p>
    {/if}
</main>
