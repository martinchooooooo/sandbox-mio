<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
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
    import { Delete02Icon } from "@hugeicons/core-free-icons";
    import { closeSandbox } from "$lib/workspace/navigation.svelte";
    import { ensurePackages } from "$lib/workspace/packages.svelte";
    import { ensureSandboxRemoteState, refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
    import { forgetProxyStatus } from "$lib/proxy/status.svelte";
    import { forgetAgentAuth } from "$lib/agents/authStatus.svelte";
    import { forgetSection, sectionFor, setSection } from "$lib/workspace/dashboard.svelte";
    import { DASHBOARD_SECTIONS, type DashboardSectionId } from "$lib/workspace/sections";
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
            await closeSandbox(sandboxName);
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

<!-- No animation of its own: SandboxView.svelte already fades the whole sandbox view in. -->
<main data-dashboard={sandboxName} class="flex h-full flex-col gap-3 p-6">
    <StatusRow busy={action.busy} message={action.message} error={action.error} />

    {#if workspacePath}
        <SectionNav.Root value={activeId} onValueChange={(v) => openSection(v as DashboardSectionId)}
            class="flex-1">
            <SectionNav.List aria-label={m.dashboard_sections_label()}>
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
        <p class="text-muted-foreground text-xs">{m.dashboard_loading()}</p>
    {/if}
</main>
