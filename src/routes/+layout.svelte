<script lang="ts">
    import "../app.css";
    import { onMount } from "svelte";
    import { page } from "$app/state";
    import { invoke } from "@tauri-apps/api/core";
    import { toast } from "svelte-sonner";
    import { ModeWatcher } from "mode-watcher";
    import { Toaster } from "$lib/components/ui/sonner";
    import Button from "$lib/components/ui/button/button.svelte";
    import GlobalNav from "$lib/components/GlobalNav.svelte";
    import ActivityCenter from "$lib/activity/ActivityCenter.svelte";
    import { ensureActivity } from "$lib/activity/activity.svelte";
    import { ensureWorkspaceState, isCollapsed, hasTabs as hasTabsStore } from "$lib/workspace/state.svelte";
    import { ensureNetworkState } from "$lib/network/state.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import type { Snippet } from "svelte";

    let { children }: { children?: Snippet } = $props();

    // Start the global activity store once, above the router, so its subscription is established
    // before any route mounts and is never torn down for the life of the webview.
    ensureActivity();

    // The /workspace/* routes load inside other chrome child webviews (the switcher tab bar),
    // which draw their own chrome; suppress the panel nav and activity center there.
    const isWorkspaceChrome = $derived(page.url.pathname.startsWith("/workspace"));

    // This layout is the app's home: the `main-global` child webview, i.e. the left collapsible
    // panel of the main workspace window. Rust owns the native geometry; `collapsed` and
    // `hasTabs` mirror the shared workspace store (src/lib/workspace/state.svelte.ts) so the DOM
    // tracks what Rust actually applied (with no embedded tab the panel is full-bleed and the
    // collapse control is meaningless, so it is hidden).
    let isGlobalPanel = $state(false);
    const collapsed = $derived(isCollapsed());
    const hasTabs = $derived(hasTabsStore());

    async function toggleCollapsed() {
        try {
            // Rust recomputes the layout and re-bounds the sibling child webviews, then echoes
            // workspace://state; the shared store keeps the DOM authoritative.
            await invoke("set_panel_collapsed", { collapsed: !collapsed });
        } catch (e) {
            console.error(e);
        }
    }

    // Checked once, from the main-global webview only (every other webview would otherwise
    // duplicate the same check). Downloads and installs silently on accept, then relaunches —
    // the NSIS `currentUser` install mode needs no elevation for either step.
    async function checkForUpdate() {
        try {
            const { check } = await import("@tauri-apps/plugin-updater");
            const update = await check();
            if (!update) return;
            toast(m.update_available(), {
                action: {
                    label: m.update_restart_button(),
                    onClick: async () => {
                        try {
                            toast(m.update_downloading());
                            await update.downloadAndInstall();
                            const { relaunch } = await import("@tauri-apps/plugin-process");
                            await relaunch();
                        } catch (e) {
                            console.error(e);
                            toast.error(m.update_install_failed());
                        }
                    },
                },
            });
        } catch (e) {
            console.error(e);
        }
    }

    onMount(() => {
        (async () => {
            try {
                const { getCurrentWebview } = await import("@tauri-apps/api/webview");
                isGlobalPanel = getCurrentWebview().label === "main-global";
            } catch {
                // Plain-browser dev or another window (agent-auth): no workspace chrome.
            }
            await ensureWorkspaceState();
            await ensureNetworkState();
            if (isGlobalPanel) checkForUpdate();
        })();
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
    {@render children?.()}
    {#if !isWorkspaceChrome}
        <ActivityCenter />
    {/if}
{/if}
