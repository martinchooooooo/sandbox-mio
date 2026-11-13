<script lang="ts">
    import "../app.css";
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { page } from "$app/state";
    import { toast } from "svelte-sonner";
    import { ModeWatcher } from "mode-watcher";
    import { Toaster } from "$lib/components/ui/sonner";
    import { ensureActivity } from "$lib/activity/activity.svelte";
    import { ensureWorkspaceState, viewSandboxName } from "$lib/workspace/state.svelte";
    import AppSidebar from "$lib/workspace/AppSidebar.svelte";
    import SandboxView from "$lib/workspace/SandboxView.svelte";
    import { ensureNetworkState } from "$lib/network/state.svelte";
    import { ensureOnboardingState, onboardingCompleted } from "$lib/settings/onboarding.svelte";
    import { reprovisionInstalledAgents } from "$lib/agents/reprovision";
    import type { InferenceStatus } from "$lib/models/types";
    import { m } from "$lib/paraglide/messages.js";
    import type { Snippet } from "svelte";

    let { children }: { children?: Snippet } = $props();

    // Start the global activity store once, above the router, so its subscription is established
    // before any route mounts and is never torn down for the life of the webview.
    ensureActivity();

    // Rust positions native panes beside the sidebar and below the sandbox header. The routed page
    // unmounts while a sandbox is in view (docs/features/unified-sidebar-nav/02_dev.md, Resolved
    // Decision 9).
    let isNavigationPanel = $state(false);
    const viewSandbox = $derived(isNavigationPanel ? viewSandboxName() : null);
    // The agent login popup is its own window on this layout; it gets no app chrome.
    const chromeless = $derived(onboardingCompleted() === false || page.url.pathname.startsWith("/agent-auth"));

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
     flashing chrome that then hides. Both branches keep `@container` on the route outlet, which
     pages resolve their container queries against. -->
<div class="flex h-screen overflow-hidden">
    {#if !chromeless}
        <AppSidebar />
        {#if viewSandbox !== null}
            {#key viewSandbox}
                <SandboxView sandbox={viewSandbox} />
            {/key}
        {:else}
            <div data-route-outlet class="@container min-w-0 flex-1 overflow-y-auto [scrollbar-gutter:stable]">
                {@render children?.()}
            </div>
        {/if}
    {:else}
        <div data-route-outlet class="@container flex-1 overflow-y-auto">
            {@render children?.()}
        </div>
    {/if}
</div>
