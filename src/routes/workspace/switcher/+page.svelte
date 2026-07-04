<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { listen } from "@tauri-apps/api/event";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { HOME_APP, LAUNCHABLE_APPS } from "$lib/workspace/apps";
    import type { AppKind, AppPane } from "$lib/workspace/state.svelte";

    type WorkspaceState = { apps: AppPane[]; active_app: string | null };

    let apps = $state<AppPane[]>([]);
    let activeApp = $state<string | null>(null);

    // Group panes by sandbox, preserving first-seen sandbox order.
    const bySandbox = $derived.by(() => {
        const groups = new Map<string, AppPane[]>();
        for (const pane of apps) {
            const group = groups.get(pane.sandbox);
            if (group) group.push(pane);
            else groups.set(pane.sandbox, [pane]);
        }
        return groups;
    });

    function applyState(s: WorkspaceState) {
        apps = s.apps;
        activeApp = s.active_app;
    }

    async function activate(pane: AppPane) {
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

    async function close(sandbox: string) {
        try {
            await invoke("close_workspace", { name: sandbox });
        } catch (e) {
            console.error(e);
        }
    }

    function paneFor(panes: AppPane[], kind: AppKind): AppPane | undefined {
        return panes.find((p) => p.kind === kind);
    }

    onMount(() => {
        let unlisten: (() => void) | undefined;
        (async () => {
            try {
                unlisten = await listen<WorkspaceState>("workspace://state", (e) =>
                    applyState(e.payload),
                );
                applyState(await invoke<WorkspaceState>("workspace_state"));
            } catch (e) {
                console.error(e);
            }
        })();
        return () => unlisten?.();
    });
</script>

<div class="flex h-screen items-start gap-3 overflow-x-auto border-b bg-background px-2 py-1 text-xs">
    {#each bySandbox as [sandbox, panes] (sandbox)}
        {@const dashboard = paneFor(panes, "dashboard")}
        <div class="flex flex-col gap-0.5" data-sandbox={sandbox}>
            <div class="flex items-center justify-between gap-2 px-1">
                <span class="truncate font-medium">{sandbox}</span>
                <button
                    class="text-muted-foreground hover:text-foreground"
                    data-close-sandbox
                    aria-label={m.workspace_close_sandbox_label({ sandbox })}
                    onclick={() => close(sandbox)}
                >
                    ×
                </button>
            </div>
            <div class="flex items-center gap-1">
                {#if dashboard}
                    <button
                        class={[
                            "flex items-center rounded px-2 py-1",
                            dashboard.label === activeApp
                                ? "bg-accent text-accent-foreground"
                                : "text-muted-foreground hover:text-foreground",
                        ].join(" ")}
                        data-app="dashboard"
                        aria-label={HOME_APP.title()}
                        title={HOME_APP.title()}
                        onclick={() => activate(dashboard)}
                    >
                        <HugeiconsIcon icon={HOME_APP.icon} size={14} />
                    </button>
                {/if}
                {#each LAUNCHABLE_APPS as app (app.kind)}
                    {@const pane = paneFor(panes, app.kind)}
                    {#if pane}
                        <div
                            class={[
                                "flex items-center rounded",
                                pane.label === activeApp
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
    {/each}
</div>
