<script lang="ts">
    import "../app.css";
    import { onMount } from "svelte";
    import { page } from "$app/state";
    import { invoke } from "@tauri-apps/api/core";
    import { ModeWatcher } from "mode-watcher";
    import { Toaster } from "$lib/components/ui/sonner";
    import Button from "$lib/components/ui/button/button.svelte";
    import GlobalNav from "$lib/components/GlobalNav.svelte";
    import ActivityCenter from "$lib/activity/ActivityCenter.svelte";
    import { ensureActivity } from "$lib/activity/activity.svelte";
    import { m } from "$lib/paraglide/messages.js";

    // Start the global activity store once, above the router, so its subscription is established
    // before any route mounts and is never torn down for the life of the webview.
    ensureActivity();

    // The /workspace/* routes load inside other chrome child webviews (the switcher tab bar),
    // which draw their own chrome; suppress the panel nav and activity center there.
    const isWorkspaceChrome = $derived(page.url.pathname.startsWith("/workspace"));

    // This layout is the app's home: the `main-global` child webview, i.e. the left collapsible
    // panel of the main workspace window. Rust owns the native geometry; `collapsed` and
    // `hasTabs` mirror the registry via the workspace://state broadcast so the DOM tracks what
    // Rust actually applied (with no embedded tab the panel is full-bleed and the collapse
    // control is meaningless, so it is hidden).
    let isGlobalPanel = $state(false);
    let collapsed = $state(false);
    let hasTabs = $state(false);

    type WorkspaceState = { apps: unknown[]; collapsed: boolean };
    function applyState(s: WorkspaceState) {
        collapsed = s.collapsed;
        hasTabs = s.apps.length > 0;
    }

    async function toggleCollapsed() {
        try {
            // Rust recomputes the layout and re-bounds the sibling child webviews, then echoes
            // workspace://state; the listener below keeps the DOM authoritative.
            await invoke("set_panel_collapsed", { collapsed: !collapsed });
        } catch (e) {
            console.error(e);
        }
    }

    onMount(() => {
        let unlisten: (() => void) | undefined;
        (async () => {
            try {
                const { getCurrentWebview } = await import("@tauri-apps/api/webview");
                isGlobalPanel = getCurrentWebview().label === "main-global";
                if (!isGlobalPanel) return;
                const { listen } = await import("@tauri-apps/api/event");
                unlisten = await listen<WorkspaceState>("workspace://state", (e) =>
                    applyState(e.payload),
                );
                applyState(await invoke<WorkspaceState>("workspace_state"));
            } catch {
                // Plain-browser dev or another window (agent-auth): no workspace chrome.
            }
        })();
        return () => unlisten?.();
    });
</script>

<ModeWatcher />
<Toaster />

{#if isGlobalPanel && collapsed && hasTabs}
    <!-- Thin-rail state: the panel is 48 logical px wide; only the expand button fits. -->
    <div class="flex h-screen flex-col items-center bg-background p-1">
        <Button variant="ghost" size="sm" onclick={toggleCollapsed} title={m.workspace_expand()}>
            »
        </Button>
    </div>
{:else}
    {#if !isWorkspaceChrome}
        <div class="flex items-center justify-between gap-2">
            <GlobalNav />
            {#if isGlobalPanel && hasTabs}
                <Button
                    variant="ghost"
                    size="sm"
                    onclick={toggleCollapsed}
                    title={m.workspace_collapse()}
                >
                    «
                </Button>
            {/if}
        </div>
    {/if}
    <slot />
    {#if !isWorkspaceChrome}
        <ActivityCenter />
    {/if}
{/if}
