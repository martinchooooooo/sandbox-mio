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
        Cancel01Icon,
        Folder01Icon,
        Home01Icon,
        Key01Icon,
        PanelLeftCloseIcon,
        PanelLeftOpenIcon,
        Settings01Icon,
    } from "@hugeicons/core-free-icons";
    import Button from "$lib/components/ui/button/button.svelte";
    import * as NavList from "$lib/components/ui/nav-list";
    import ActivityCenter from "$lib/activity/ActivityCenter.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { parseError } from "$lib/errors";
    import { statusDotClass, type Project } from "$lib/sandbox";
    import { lifecycleMessage, pendingCreates, pendingOp } from "$lib/sandbox/lifecycle.svelte";
    import { deploymentFor, ensureSandboxRemoteState, liveStatus, sandboxesList } from "$lib/sandbox/state.svelte";
    import { HOME_APP, LAUNCHABLE_APPS } from "$lib/workspace/apps";
    import { activeAppLabel, activePane, isCollapsed, panesOf, viewSandboxName } from "$lib/workspace/state.svelte";
    import {
        activatePane,
        closePane,
        closeSandbox,
        newSandbox,
        openSandbox,
        showManage,
        showPage,
    } from "$lib/workspace/navigation.svelte";

    // A failed placeholder stays listed; a successful one is hidden once its real entry exists.
    const creating = $derived(
        pendingCreates().filter((c) => c.error || !sandboxesList().some((p) => p.name === c.name)),
    );

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

    const viewPanes = $derived(viewing ? panesOf(viewing) : []);
    // Apps are launched from the dashboard cards; the sidebar lists only the ones that are open.
    const apps = $derived(LAUNCHABLE_APPS.filter((app) => viewPanes.some((p) => p.kind === app.kind)));
    const dashboardActive = $derived(viewing !== null && activeAppLabel() === null);

    async function openFolder(sandbox: string) {
        try {
            await invoke("open_sandbox_folder", { name: sandbox });
        } catch (e) {
            toast.error(parseError(e).message);
        }
    }

    function pageActive(href: string): boolean {
        return viewing === null && page.url.pathname === href;
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
                {@const pending = pendingOp(project.name)}
                {@const hint = pending ? lifecycleMessage(pending) : undefined}
                {@const dot = statusDotClass(liveStatus(project), deploymentFor(project.name), pending)}
                {#snippet closeSandboxAction()}
                    <Button
                        variant="ghost"
                        size="icon"
                        class="size-6"
                        data-close-sandbox
                        aria-label={m.workspace_close_sandbox_label({ sandbox: project.name })}
                        title={m.workspace_close_sandbox_label({ sandbox: project.name })}
                        onclick={() => closeSandbox(project.name)}
                    >
                        <HugeiconsIcon icon={Cancel01Icon} size={12} />
                    </Button>
                {/snippet}
                <NavList.Item
                    data-nav-sandbox={project.name}
                    data-pending={pending}
                    label={project.name}
                    contains={viewing === project.name}
                    action={viewing === project.name ? closeSandboxAction : undefined}
                    onclick={() => open(project)}
                >
                    {#snippet icon()}
                        {#if collapsed}
                            <span class="relative flex size-6 items-center justify-center rounded-sm border-2 font-head text-xs uppercase">
                                {project.name.slice(0, 1)}
                                <span class={["absolute -bottom-1 -right-1 size-2 rounded-full", dot]} title={hint}></span>
                            </span>
                        {:else}
                            <span class={["inline-block size-2 rounded-full", dot]} title={hint}></span>
                        {/if}
                    {/snippet}
                </NavList.Item>
                {#if viewing === project.name}
                    <div
                        role="group"
                        aria-label={m.sandbox_header_apps_label()}
                        data-sandbox-apps={project.name}
                        class={[
                            "flex flex-col gap-1",
                            collapsed ? "bg-muted/50 rounded-md p-0.5" : "ml-3.5 border-l-2 pl-1.5",
                        ]}
                    >
                        <NavList.Item
                            data-app="dashboard"
                            class="h-8"
                            label={HOME_APP.title()}
                            active={dashboardActive}
                            onclick={() => showManage(project.name)}
                        >
                            {#snippet icon()}
                                <HugeiconsIcon icon={HOME_APP.icon} size={16} />
                            {/snippet}
                        </NavList.Item>
                        {#each apps as app (app.kind)}
                            {@const pane = viewPanes.find((p) => p.kind === app.kind)!}
                            {@const isActive = pane.label === activeAppLabel()}
                            {#snippet closeAppAction()}
                                <Button
                                    variant="ghost"
                                    size="icon"
                                    class="size-6"
                                    data-close-tab={app.kind}
                                    aria-label={m.workspace_close_tab_label({ title: app.title() })}
                                    title={m.workspace_close_tab_label({ title: app.title() })}
                                    onclick={() => closePane(pane)}
                                >
                                    <HugeiconsIcon icon={Cancel01Icon} size={12} />
                                </Button>
                            {/snippet}
                            <NavList.Item
                                data-app={app.kind}
                                class="h-8"
                                label={app.title()}
                                active={isActive}
                                action={closeAppAction}
                                onclick={() => activatePane(pane)}
                            >
                                {#snippet icon()}
                                    <HugeiconsIcon icon={app.icon} size={16} />
                                {/snippet}
                            </NavList.Item>
                        {/each}
                        <NavList.Item
                            data-open-folder
                            class="h-8"
                            label={m.workspace_open_folder()}
                            muted
                            onclick={() => openFolder(project.name)}
                        >
                            {#snippet icon()}
                                <HugeiconsIcon icon={Folder01Icon} size={16} />
                            {/snippet}
                        </NavList.Item>
                    </div>
                {/if}
            {/each}
            {#each creating as create (create.name)}
                {@const dot = create.error ? "bg-destructive" : statusDotClass(undefined, undefined, "create")}
                {@const hint = create.error || create.step}
                <!-- Not a destination yet: a click goes to Home, where the placeholder card shows progress. -->
                <NavList.Item
                    data-nav-sandbox={create.name}
                    data-pending="create"
                    label={create.name}
                    active={false}
                    onclick={() => showPage("/projects")}
                >
                    {#snippet icon()}
                        {#if collapsed}
                            <span class="relative flex size-6 items-center justify-center rounded-sm border-2 font-head text-xs uppercase">
                                {create.name.slice(0, 1)}
                                <span class={["absolute -bottom-1 -right-1 size-2 rounded-full", dot]} title={hint}></span>
                            </span>
                        {:else}
                            <span class={["inline-block size-2 rounded-full", dot]} title={hint}></span>
                        {/if}
                    {/snippet}
                </NavList.Item>
            {/each}
            {#if sandboxesList().length === 0 && creating.length === 0 && !collapsed}
                <p class="text-muted-foreground px-2.5 text-xs">{m.sidebar_no_sandboxes()}</p>
            {/if}
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
