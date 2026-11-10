<script lang="ts">
    import "../app.css";
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { toast } from "svelte-sonner";
    import { ModeWatcher } from "mode-watcher";
    import { Toaster } from "$lib/components/ui/sonner";
    import { ensureActivity } from "$lib/activity/activity.svelte";
    import {
        ensureWorkspaceState,
        isCollapsed,
        hasTabs as hasTabsStore,
        activeSandboxName,
        activeAppLabel,
    } from "$lib/workspace/state.svelte";
    import NavigationColumn from "$lib/workspace/NavigationColumn.svelte";
    import WorkspaceContent from "$lib/workspace/WorkspaceContent.svelte";
    import { ensureNetworkState } from "$lib/network/state.svelte";
    import { ensureOnboardingState, onboardingCompleted } from "$lib/settings/onboarding.svelte";
    import { reprovisionInstalledAgents } from "$lib/agents/reprovision";
    import type { InferenceStatus } from "$lib/models/types";
    import { m } from "$lib/paraglide/messages.js";
    import { rise } from "$lib/motion";
    import type { Snippet } from "svelte";

    let { children }: { children?: Snippet } = $props();

    // Start the global activity store once, above the router, so its subscription is established
    // before any route mounts and is never torn down for the life of the webview.
    ensureActivity();

    // This layout is the app's home: the `main-navigation-panel` child webview, i.e. the merged
    // NavigationPanel/Switcher/Dashboard document (docs/features/consolidate-app-container/02_dev.md).
    // Rust owns the native geometry; `collapsed`/`hasTabs`/`activeAppLabel` mirror the shared
    // workspace store (src/lib/workspace/state.svelte.ts) so the DOM tracks what Rust actually
    // applied (with no embedded tab the panel is full-bleed and the collapse control is
    // meaningless, so it is hidden). Dashboard visibility is *derived* from `activeAppLabel()`
    // being null (no Terminal/AgentWeb overlay confirmed active) rather than tracked as its own
    // client-side "current view" — see the comment on `activeSandbox` in state.svelte.ts for why
    // that distinction matters (a prior version hid the Dashboard optimistically before the
    // `invoke` that opens a pane resolved, leaving nothing shown if it was slow or failed).
    let isNavigationPanel = $state(false);
    const collapsed = $derived(isCollapsed());
    const hasTabs = $derived(hasTabsStore());
    const dashboardSandbox = $derived(activeAppLabel() === null ? activeSandboxName() : null);
    // Whether the switcher/content column exists at all: either a Terminal/AgentWeb pane is open
    // somewhere, or the user has navigated to a sandbox's Dashboard. With neither, the nav column
    // is the whole window, same as before any workspace existed.
    const hasWorkspaceContent = $derived(hasTabs || dashboardSandbox !== null);
    const showWorkspaceChrome = $derived(isNavigationPanel && hasWorkspaceContent);
    // The navigation column stays docked while the closing workspace fades out; snapping it to
    // full width first would split the window with the outgoing content.
    let workspaceLeaving = $state(false);
    const dockNavigation = $derived(showWorkspaceChrome || workspaceLeaving);
    // Held while the column fades out, so it leaves with its content rather than going blank first.
    let lastWorkspace = { hasTabs: false, dashboardSandbox: null as string | null };
    const workspace = $derived.by(() => {
        if (showWorkspaceChrome) lastWorkspace = { hasTabs, dashboardSandbox };
        return lastWorkspace;
    });

    async function toggleCollapsed() {
        try {
            // Rust recomputes the layout and re-bounds the sibling child webviews, then echoes
            // workspace://state; the shared store keeps the DOM authoritative.
            await invoke("set_panel_collapsed", { collapsed: !collapsed });
        } catch (e) {
            console.error(e);
        }
    }

    // Checked once, from the main-navigation-panel webview only (every other webview would otherwise
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

    // Once per app run, from the main-navigation-panel webview only, like `checkForUpdate`.
    async function reprovisionAtStartup() {
        let runningModelId: string | null = null;
        try {
            const status = await invoke<InferenceStatus>("inference_status");
            runningModelId = status.running ? status.modelId : null;
        } catch {
            runningModelId = null;
        }
        await reprovisionInstalledAgents(runningModelId);
    }

    onMount(() => {
        (async () => {
            try {
                const { getCurrentWebview } = await import("@tauri-apps/api/webview");
                isNavigationPanel = getCurrentWebview().label === "main-navigation-panel";
            } catch {
                // Plain-browser dev or another window (agent-auth): no workspace chrome.
            }
            await ensureWorkspaceState();
            await ensureNetworkState();
            await ensureOnboardingState();
            if (isNavigationPanel) {
                checkForUpdate();
                reprovisionAtStartup();
            }
        })();
    });
</script>

<ModeWatcher />
<!-- During first-run onboarding the bottom-right corner holds the book's Next button, and a failure
     toast stays until dismissed, so toasts move out of its way. -->
<Toaster position={onboardingCompleted() === false ? "top-left" : "bottom-right"} />

<!-- Gated on `!== false` so the still-loading state matches the eventual `false` case rather than
     flashing chrome that then hides. NavigationColumn *wraps* the routed page, so the chromeless
     branch has to re-render `children` itself — and keeps `@container`, which components below
     resolve their container queries against. -->
<div class="flex h-screen overflow-hidden">
    {#if onboardingCompleted() !== false}
        <NavigationColumn {collapsed} showControls={dockNavigation} onToggleCollapsed={toggleCollapsed}>
            {@render children?.()}
        </NavigationColumn>
        {#if showWorkspaceChrome}
            <div
                class="flex min-w-0 flex-1"
                in:rise={{ x: 16, y: 0 }}
                out:rise={{ x: 16, y: 0 }}
                onintrostart={() => (workspaceLeaving = false)}
                onoutrostart={() => (workspaceLeaving = true)}
                onoutroend={() => (workspaceLeaving = false)}
            >
                <WorkspaceContent hasTabs={workspace.hasTabs} dashboardSandbox={workspace.dashboardSandbox} />
            </div>
        {/if}
    {:else}
        <div data-route-outlet class="@container flex-1 overflow-y-auto">
            {@render children?.()}
        </div>
    {/if}
</div>
