<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { page } from "$app/state";
    import { toast } from "svelte-sonner";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import {
        Add01Icon,
        AiBrain01Icon,
        BookOpen01Icon,
        Home01Icon,
        Key01Icon,
        PanelLeftCloseIcon,
        PanelLeftOpenIcon,
        Settings01Icon,
    } from "@hugeicons/core-free-icons";
    import Button from "$lib/components/ui/button/button.svelte";
    import * as NavList from "$lib/components/ui/nav-list";
    import ActivityCenter from "$lib/activity/ActivityCenter.svelte";
    import { containerStatusFor } from "$lib/activity/activity.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { parseError } from "$lib/errors";
    import { statusDotClass, type Project } from "$lib/sandbox";
    import { deploymentFor, ensureSandboxRemoteState, sandboxesList } from "$lib/sandbox/state.svelte";
    import { activePane, isCollapsed, viewSandboxName } from "$lib/workspace/state.svelte";
    import { newSandbox, openSandbox, showPage } from "$lib/workspace/navigation.svelte";

    const collapsed = $derived(isCollapsed());
    const viewing = $derived(viewSandboxName());
    // A webview pane covers DOM tooltips that cross the rail's edge; the native chat has no webview.
    const webviewPaneShown = $derived(activePane()?.url != null);

    const pages = [
        { href: "/models", label: m.nav_models, icon: AiBrain01Icon },
        { href: "/credentials", label: m.nav_credentials, icon: Key01Icon },
        { href: "/settings", label: m.nav_settings, icon: Settings01Icon },
        { href: "/intro", label: m.nav_intro, icon: BookOpen01Icon },
    ];

    function pageActive(href: string): boolean {
        return viewing === null && page.url.pathname === href;
    }

    // Container events land between the store's 3 s polls, so the dot prefers the latest event.
    function statusOf(project: Project): string {
        return containerStatusFor(project.name) ?? project.status;
    }

    async function open(project: Project) {
        try {
            await openSandbox(project.name, project.workspace_path, { restore: true });
        } catch (e) {
            toast.error(parseError(e).message);
        }
    }

    async function toggleCollapsed() {
        try {
            // Rust re-bounds the pane webviews, then echoes workspace://state; the store keeps the
            // rail state authoritative.
            await invoke("set_panel_collapsed", { collapsed: !collapsed });
        } catch (e) {
            console.error(e);
        }
    }

    onMount(() => {
        ensureSandboxRemoteState();
    });
</script>

<!-- w-60/w-12 must equal NAVIGATION_PANEL_EXPANDED_W/NAVIGATION_PANEL_RAIL_W in
     src-tauri/src/workspace/mod.rs: Rust places the pane webviews at that x offset. -->
<aside
    data-sidebar
    data-collapsed={collapsed}
    class={[
        "flex h-full shrink-0 flex-col border-r-2 bg-sidebar text-sidebar-foreground",
        collapsed ? "w-12 px-1 py-2" : "w-60 p-2",
    ]}
>
    <div class={["flex items-center gap-2 pb-2", collapsed ? "justify-center" : "justify-between pl-2.5"]}>
        {#if !collapsed}
            <span class="truncate font-head text-lg">{m.app_name()}</span>
        {/if}
        <Button
            variant="ghost"
            size="icon"
            data-sidebar-toggle
            aria-label={collapsed ? m.workspace_expand() : m.workspace_collapse()}
            title={collapsed ? m.workspace_expand() : m.workspace_collapse()}
            onclick={toggleCollapsed}
        >
            <HugeiconsIcon icon={collapsed ? PanelLeftOpenIcon : PanelLeftCloseIcon} size={16} />
        </Button>
    </div>

    <NavList.Root
        aria-label={m.sidebar_label()}
        {collapsed}
        nativeTooltips={webviewPaneShown}
        class="flex-1"
    >
        <NavList.Item
            data-nav="/projects"
            label={m.nav_projects()}
            active={pageActive("/projects")}
            onclick={() => showPage("/projects")}
        >
            {#snippet icon()}
                <HugeiconsIcon icon={Home01Icon} size={16} />
            {/snippet}
        </NavList.Item>

        <NavList.Group label={m.sidebar_sandboxes()} class="min-h-0 flex-1 overflow-y-auto" data-sidebar-sandboxes>
            {#each sandboxesList() as project (project.id)}
                {@const dot = statusDotClass(statusOf(project), deploymentFor(project.name))}
                <NavList.Item
                    data-nav-sandbox={project.name}
                    label={project.name}
                    active={viewing === project.name}
                    onclick={() => open(project)}
                >
                    {#snippet icon()}
                        {#if collapsed}
                            <span class="relative flex size-6 items-center justify-center rounded-sm border-2 font-head text-xs uppercase">
                                {project.name.slice(0, 1)}
                                <span class={["absolute -bottom-1 -right-1 size-2 rounded-full", dot]}></span>
                            </span>
                        {:else}
                            <span class={["inline-block size-2 rounded-full", dot]}></span>
                        {/if}
                    {/snippet}
                </NavList.Item>
            {:else}
                {#if !collapsed}
                    <p class="text-muted-foreground px-2.5 text-xs">{m.sidebar_no_sandboxes()}</p>
                {/if}
            {/each}
            <NavList.Item data-new-sandbox label={m.sidebar_new_sandbox()} onclick={newSandbox}>
                {#snippet icon()}
                    <HugeiconsIcon icon={Add01Icon} size={16} />
                {/snippet}
            </NavList.Item>
        </NavList.Group>

        <NavList.Group class="border-t-2 pt-2">
            {#each pages as item (item.href)}
                <NavList.Item
                    data-nav={item.href}
                    label={item.label()}
                    active={pageActive(item.href)}
                    onclick={() => showPage(item.href)}
                >
                    {#snippet icon()}
                        <HugeiconsIcon icon={item.icon} size={16} />
                    {/snippet}
                </NavList.Item>
            {/each}
            <ActivityCenter placement="sidebar" />
        </NavList.Group>
    </NavList.Root>
</aside>
