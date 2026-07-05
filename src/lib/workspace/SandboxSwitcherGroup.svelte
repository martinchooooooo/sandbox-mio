<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { HOME_APP, LAUNCHABLE_APPS } from "$lib/workspace/apps";
    import {
        activeAppLabel,
        activeSandboxName,
        setActiveSandbox,
        type AppKind,
        type AppPane,
    } from "$lib/workspace/state.svelte";

    let { sandbox, panes }: { sandbox: string; panes: AppPane[] } = $props();

    const homeActive = $derived(activeAppLabel() === null && activeSandboxName() === sandbox);

    // Dashboard visibility is derived (`activeAppLabel() === null && activeSandbox === sandbox`,
    // see +layout.svelte and the comment on `activeSandbox` in state.svelte.ts), so this only needs
    // to record the sandbox and ask Rust to hide whatever overlay is currently shown; the Dashboard
    // then appears once `workspace://state` confirms `active_app` is null.
    async function viewDashboard() {
        setActiveSandbox(sandbox);
        try {
            await invoke("hide_active_pane");
        } catch (e) {
            console.error(e);
        }
    }

    async function activate(pane: AppPane) {
        setActiveSandbox(pane.sandbox);
        try {
            await invoke("switch_pane", { name: pane.sandbox, kind: pane.kind });
        } catch (e) {
            console.error(e);
        }
    }

    async function closePane(pane: AppPane) {
        try {
            await invoke("close_pane", { name: pane.sandbox, kind: pane.kind });
        } catch (e) {
            console.error(e);
        }
    }

    async function close() {
        try {
            await invoke("close_workspace", { name: sandbox });
        } catch (e) {
            console.error(e);
        }
    }

    function paneFor(kind: AppKind): AppPane | undefined {
        return panes.find((p) => p.kind === kind);
    }
</script>

<div class="flex flex-col gap-0.5" data-sandbox={sandbox}>
    <div class="flex items-center justify-between gap-2 px-1">
        <span class="truncate font-medium">{sandbox}</span>
        <button
            class="text-muted-foreground hover:text-foreground"
            data-close-sandbox
            aria-label={m.workspace_close_sandbox_label({ sandbox })}
            onclick={close}
        >
            ×
        </button>
    </div>
    <div class="flex items-center gap-1">
        <button
            class={[
                "flex items-center rounded px-2 py-1",
                homeActive
                    ? "bg-accent text-accent-foreground"
                    : "text-muted-foreground hover:text-foreground",
            ].join(" ")}
            data-app="dashboard"
            aria-label={HOME_APP.title()}
            title={HOME_APP.title()}
            onclick={viewDashboard}
        >
            <HugeiconsIcon icon={HOME_APP.icon} size={14} />
        </button>
        {#each LAUNCHABLE_APPS as app (app.kind)}
            {@const pane = paneFor(app.kind)}
            {#if pane}
                <div
                    class={[
                        "flex items-center rounded",
                        pane.label === activeAppLabel()
                            ? "bg-accent text-accent-foreground"
                            : "text-muted-foreground hover:text-foreground",
                    ].join(" ")}
                >
                    <button
                        class="flex items-center gap-1 rounded-l px-2 py-1"
                        data-app={app.kind}
                        onclick={() => activate(pane)}
                    >
                        <HugeiconsIcon icon={app.icon} size={14} />
                        {app.title()}
                    </button>
                    <button
                        class="pr-1.5 hover:text-foreground"
                        data-close-tab={app.kind}
                        aria-label={m.workspace_close_tab_label({ title: app.title() })}
                        onclick={() => closePane(pane)}
                    >
                        ×
                    </button>
                </div>
            {/if}
        {/each}
    </div>
</div>
