<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { listen } from "@tauri-apps/api/event";

    // The tab bar of the main workspace window (`main-switcher` child webview). Two tabs per
    // sandbox whose workspace is open — Terminal and Manage (docs/features/responsive-panel/
    // 02_dev.md Phase E) — the list mirrors the Rust registry via the workspace://state
    // broadcast; the DOM here only reflects intent, Rust owns which pane is shown.
    type AppKind = "terminal" | "dashboard";
    type AppPane = { label: string; sandbox: string; kind: AppKind; url: string };
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
                    {sandbox} — {pane.kind === "dashboard" ? "Manage" : "Terminal"}<!-- TODO(i18n) -->
                </button>
            {/each}
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
