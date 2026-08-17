<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import GlobalNav from "$lib/components/GlobalNav.svelte";
    import ActivityCenter from "$lib/activity/ActivityCenter.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import type { Snippet } from "svelte";

    // 380px/48px cross-reference NAVIGATION_PANEL_EXPANDED_W/NAVIGATION_PANEL_RAIL_W
    // (src-tauri/src/workspace/mod.rs): the Terminal/AgentWeb overlay's native rect is computed
    // from those constants and stacks on top of the switcher/content column to the right of this
    // one, so a drift between the two renders it in the wrong place
    // (docs/features/consolidate-app-container/02_dev.md Resolved Decision 1).
    let {
        collapsed,
        showControls,
        onToggleCollapsed,
        children,
    }: {
        collapsed: boolean;
        // Whether this is the main-navigation-panel webview with a Terminal/AgentWeb pane or
        // Dashboard open — with neither, the collapse control is meaningless and this column is
        // just the ordinary full-width app shell (routed page + GlobalNav + ActivityCenter).
        showControls: boolean;
        onToggleCollapsed: () => void;
        children?: Snippet;
    } = $props();
</script>

<div
    class={[
        "@container flex flex-col",
        showControls ? (collapsed ? "w-12 shrink-0" : "w-[380px] shrink-0") : "flex-1",
    ].join(" ")}
>
    {#if showControls && collapsed}
        <!-- Thin-rail state: only the expand button fits. -->
        <div class="flex flex-col items-center bg-background p-1">
            <Button variant="ghost" size="sm" onclick={onToggleCollapsed} title={m.workspace_expand()}>
                »
            </Button>
        </div>
    {:else}
        <div class="flex items-start justify-between gap-2 pb-2">
            <GlobalNav />
            {#if showControls}
                <Button variant="ghost" size="sm" onclick={onToggleCollapsed} title={m.workspace_collapse()}>
                    «
                </Button>
            {/if}
        </div>
        <div class="flex flex-1 flex-col overflow-y-auto [scrollbar-gutter:stable]">
            {@render children?.()}
            <ActivityCenter />
        </div>
    {/if}
</div>
