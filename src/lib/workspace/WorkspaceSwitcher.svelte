<script lang="ts">
    import { workspaceApps, type AppPane } from "$lib/workspace/state.svelte";
    import SandboxSwitcherGroup from "$lib/workspace/SandboxSwitcherGroup.svelte";

    // Group panes by sandbox, preserving first-seen sandbox order. Only sandboxes with an open
    // Terminal/AgentWeb pane get a switcher group — a sandbox visited only via Dashboard has no
    // Rust-tracked pane to group (docs/features/consolidate-app-container/02_dev.md Resolved
    // Decision 2), so it has no tab bar of its own; it is reached via ordinary page navigation.
    const bySandbox = $derived.by(() => {
        const groups = new Map<string, AppPane[]>();
        for (const pane of workspaceApps()) {
            const group = groups.get(pane.sandbox);
            if (group) group.push(pane);
            else groups.set(pane.sandbox, [pane]);
        }
        return groups;
    });
</script>

<!-- h-16 = 64px, must match SWITCHER_H (src-tauri/src/workspace/mod.rs) so the Terminal/AgentWeb
     overlay's native rect (positioned independently by Rust) lines up with this bar's actual
     height; a taller/shorter value here doesn't move the overlay, it just leaves the Dashboard's
     `flex-1` sibling squeezed or gapped. -->
<div class="flex h-16 shrink-0 items-start gap-3 overflow-x-auto border-b bg-background px-2 py-1 text-xs">
    {#each bySandbox as [sandbox, panes] (sandbox)}
        <SandboxSwitcherGroup {sandbox} {panes} />
    {/each}
</div>
