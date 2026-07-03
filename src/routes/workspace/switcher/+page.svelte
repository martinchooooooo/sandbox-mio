<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { listen } from "@tauri-apps/api/event";

    type AppKind = "terminal" | "dashboard" | "agentweb";
    type AppPane = {
        label: string;
        sandbox: string;
        workspace_path: string;
        kind: AppKind;
        url: string;
    };
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

    async function openAgents(sandbox: string, workspacePath: string) {
        try {
            await invoke("open_agent_web", { name: sandbox, workspacePath });
        } catch (e) {
            console.error(e);
        }
    }

    function paneTitle(kind: AppKind): string {
        // TODO(i18n)
        return kind === "dashboard" ? "Manage" : kind === "agentweb" ? "Agents" : "Terminal";
    }

    async function close(sandbox: string) {
        try {
            await invoke("close_workspace", { name: sandbox });
        } catch (e) {
            console.error(e);
        }
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

<div class="flex h-screen items-center gap-1 border-b bg-background px-2 text-xs">
    {#each bySandbox as [sandbox, panes] (sandbox)}
        <div class="flex items-center rounded">
            {#each panes as pane (pane.label)}
                <button
                    class={[
                        "px-3 py-1 rounded",
                        pane.label === activeApp
                            ? "bg-accent font-medium text-accent-foreground"
                            : "text-muted-foreground hover:text-foreground",
                    ].join(" ")}
                    onclick={() => activate(pane)}
                >
                    {sandbox} — {paneTitle(pane.kind)}
                </button>
            {/each}
            {#if !panes.some((p) => p.kind === "agentweb")}
                <button
                    class="px-3 py-1 rounded text-muted-foreground hover:text-foreground"
                    onclick={() => openAgents(sandbox, panes[0].workspace_path)}
                >
                    {sandbox} — Agents<!-- TODO(i18n) -->
                </button>
            {/if}
            <button
                class="pr-2 text-muted-foreground hover:text-foreground"
                aria-label={`close ${sandbox}`}
                onclick={() => close(sandbox)}
            >
                ×
            </button>
        </div>
    {/each}
</div>
