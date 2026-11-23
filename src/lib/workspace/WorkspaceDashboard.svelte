<script lang="ts">
    import { onMount } from "svelte";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import * as SectionNav from "$lib/components/ui/section-nav";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import StatusRow from "../../routes/projects/StatusRow.svelte";
    import PackageManager from "../../routes/projects/PackageManager.svelte";
    import PortsManager from "../../routes/projects/PortsManager.svelte";
    import SshAccessCard from "../../routes/projects/SshAccessCard.svelte";
    import RemoteManager from "../../routes/projects/RemoteManager.svelte";
    import ProxyPanel from "../../routes/projects/ProxyPanel.svelte";
    import TrafficPanel from "../../routes/projects/TrafficPanel.svelte";
    import { ensurePackages } from "$lib/workspace/packages.svelte";
    import { ensureSandboxRemoteState, refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
    import { sectionFor, setSection } from "$lib/workspace/dashboard.svelte";
    import { DASHBOARD_SECTIONS, type DashboardSectionId } from "$lib/workspace/sections";
    import SandboxTitleBar from "$lib/workspace/SandboxTitleBar.svelte";
    import DashboardOverview from "$lib/workspace/DashboardOverview.svelte";

    // Rendered by SandboxView.svelte when no pane is active for the sandbox in view
    // (docs/features/consolidate-app-container/02_dev.md) — not an embedded pane with its own lifecycle.
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

    onMount(async () => {
        await ensureSandboxRemoteState();
        // A sandbox created moments ago is not in the store until its next poll.
        if (!entry) await refreshSandboxState();
    });

    $effect(() => {
        ensurePackages(sandboxName, workspacePath);
    });
</script>

<!-- No animation of its own: SandboxView.svelte already fades the whole sandbox view in. -->
<main data-dashboard={sandboxName} class="flex h-full flex-col gap-4 p-6">
    <SandboxTitleBar sandbox={sandboxName} dashboardBusy={action.busy !== null} />
    <div data-dashboard-status>
        <StatusRow busy={action.busy} message={action.message} error={action.error} />
    </div>

    {#if workspacePath}
        <SectionNav.Root value={activeId} onValueChange={(v) => openSection(v as DashboardSectionId)}
            class="flex-1">
            <SectionNav.List aria-label={m.dashboard_sections_label()}>
                {#each sections as section (section.id)}
                    <SectionNav.Item value={section.id} label={section.label()}>
                        {#snippet icon()}
                            <HugeiconsIcon icon={section.icon} size={16} />
                        {/snippet}
                    </SectionNav.Item>
                {/each}
            </SectionNav.List>

            {#each sections as section (section.id)}
                <SectionNav.Content value={section.id}>
                    <div class="flex max-w-3xl flex-col gap-4 pb-4 pr-1">
                        {#if section.id === "overview"}
                            <DashboardOverview {sandboxName} {workspacePath} {action} {showRemote}
                                onOpenSection={openSection} />
                        {:else if section.id === "tools"}
                            <PackageManager {sandboxName} {workspacePath} {action} />
                        {:else if section.id === "ports"}
                            <PortsManager {sandboxName} {workspacePath} {action} />
                            <SshAccessCard {sandboxName} {workspacePath} {action} />
                        {:else if section.id === "network"}
                            <ProxyPanel {sandboxName} {workspacePath} {provider} {action} />
                        {:else if section.id === "traffic"}
                            <TrafficPanel {sandboxName} {workspacePath} {action} />
                        {:else if section.id === "remote" && provider}
                            <RemoteManager {sandboxName} {workspacePath} {provider} {action} />
                        {/if}
                    </div>
                </SectionNav.Content>
            {/each}
        </SectionNav.Root>
    {:else}
        <p class="text-muted-foreground text-xs">{m.dashboard_loading()}</p>
    {/if}
</main>
