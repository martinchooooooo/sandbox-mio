<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { listen } from "@tauri-apps/api/event";

    // The tab bar of the main workspace window (`main-switcher` child webview). One tab per
    // embedded sandbox pane; the list mirrors the Rust registry via the workspace://state
    // broadcast — the DOM here only reflects intent, Rust owns which pane is shown.
    type AppPane = { label: string; sandbox: string; kind: string; url: string };
    type WorkspaceState = { apps: AppPane[]; active_app: string | null };

    let apps = $state<AppPane[]>([]);
    let activeApp = $state<string | null>(null);

    function applyState(s: WorkspaceState) {
        apps = s.apps;
        activeApp = s.active_app;
    }

    async function activate(pane: AppPane) {
        try {
            await invoke("switch_app", { name: pane.sandbox });
        } catch (e) {
            console.error(e);
        }
    }

    async function close(pane: AppPane) {
        try {
            await invoke("close_workspace", { name: pane.sandbox });
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
    {#each apps as pane (pane.label)}
        <div
            class={[
                "flex items-center rounded",
                pane.label === activeApp
                    ? "bg-accent font-medium text-accent-foreground"
                    : "text-muted-foreground hover:text-foreground",
            ].join(" ")}
        >
            <button class="px-3 py-1" onclick={() => activate(pane)}>
                {pane.sandbox}
            </button>
            <button
                class="pr-2 text-muted-foreground hover:text-foreground"
                aria-label={`close ${pane.sandbox}`}
                onclick={() => close(pane)}
            >
                ×
            </button>
        </div>
    {/each}
</div>
