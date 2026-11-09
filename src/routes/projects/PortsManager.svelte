<script lang="ts">
    import { openUrl } from "@tauri-apps/plugin-opener";
    import Button from "$lib/components/ui/button/button.svelte";
    import { parseError } from "$lib/errors";
    import { openForwardsFor, exposePort, unexposePort, type ForwardInfo } from "$lib/network/state.svelte";
    import type { createActionRunner } from "$lib/actions.svelte";

    // `.mio/ports.json` watching is started backend-side by `watch_workspace_ports`
    // (src-tauri/src/commands/panes.rs)
    // as soon as this sandbox's workspace pane opens, so this component only needs to read the
    // live store — no mount-time invoke here. `workspacePath` is still needed for `exposePort`,
    // which bootstraps that same file if it doesn't exist yet.
    let {
        sandboxName,
        workspacePath,
        action,
    }: {
        sandboxName: string;
        workspacePath: string;
        action: ReturnType<typeof createActionRunner>;
    } = $props();

    // The store (src/lib/network/state.svelte.ts) is kept live by `network://forwards-changed`, so
    // this list updates as soon as the backend's own mutation completes, without a manual refetch.
    // Only user-exposed ports belong in this section: every open sandbox also carries the internal
    // `terminal`/`agent-web` bridges, which the user neither opened here nor can manage here, so
    // they're filtered out (otherwise "No ports open." would never show for a fresh sandbox).
    const forwards = $derived(openForwardsFor(sandboxName).filter((f) => f.kind.kind === "exposed"));
    let newPort = $state("");
    let portBusy = $state(false);
    let portError = $state<string | null>(null);

    function kindLabel(fwd: ForwardInfo): string {
        if (fwd.label) return fwd.label;
        switch (fwd.kind.kind) {
            case "terminal": return "Terminal";
            case "agent-web": return "Agents";
            case "exposed": return fwd.autoDetected ? `Auto :${fwd.kind.targetPort}` : `Exposed :${fwd.kind.targetPort}`;
        }
    }

    // Ports declared via `.mio/ports.json` (identified by having a `label`) or discovered by the
    // listening-socket scan (`autoDetected`) are managed by the backend poll loop, not the UI:
    // they come back on the next tick as long as the file entry/listening socket still exists, so
    // they don't get a manual close button.
    function isManaged(fwd: ForwardInfo): boolean {
        return fwd.kind.kind === "exposed" && (fwd.label !== null || fwd.autoDetected);
    }

    async function addExposedPort() {
        const port = Number(newPort);
        if (!Number.isInteger(port) || port < 1 || port > 65535) return;
        portBusy = true;
        portError = null;
        try {
            await exposePort(sandboxName, workspacePath, port);
            newPort = "";
        } catch (e) {
            portError = parseError(e).message;
        } finally {
            portBusy = false;
        }
    }

    async function removeExposedPort(targetPort: number) {
        portBusy = true;
        try {
            await unexposePort(sandboxName, targetPort);
        } finally {
            portBusy = false;
        }
    }
</script>

<div class="flex flex-col gap-1.5">
    <span class="text-muted-foreground text-xs shrink-0">Ports</span>
    {#if forwards.length > 0}
        <div class="flex flex-col gap-1">
            {#each forwards as fwd (fwd.forwardName)}
                <div class="flex items-center gap-2 rounded bg-muted px-2 py-1 text-xs">
                    <span class="w-24 shrink-0 font-mono">{kindLabel(fwd)}</span>
                    <!-- `data-port-url` is the e2e hook (e2e/test/pageobjects/workspace-dashboard.page.ts):
                         the ✕ beside it is a type="button" too, so only this marker tells them apart. -->
                    <button
                        type="button"
                        data-port-url
                        onclick={() => openUrl(`http://127.0.0.1:${fwd.hostPort}/`)}
                        class="flex-1 truncate text-left text-blue-600 hover:underline dark:text-blue-400"
                    >
                        http://127.0.0.1:{fwd.hostPort}/
                    </button>
                    {#if fwd.kind.kind === "exposed" && !isManaged(fwd)}
                        <Button variant="ghost" size="icon" disabled={portBusy} title="Stop exposing"
                            onclick={() => removeExposedPort(fwd.kind.kind === "exposed" ? fwd.kind.targetPort : 0)}>
                            ✕
                        </Button>
                    {/if}
                </div>
            {/each}
        </div>
    {:else}
        <span class="text-muted-foreground text-xs">No ports open.</span>
    {/if}
    <div class="flex items-center gap-2">
        <input
            type="number"
            min="1"
            max="65535"
            placeholder="Expose a port (e.g. 3000)"
            bind:value={newPort}
            class="flex-1 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
            disabled={portBusy}
            onkeydown={(e) => e.key === "Enter" && addExposedPort()}
        />
        <Button size="sm" disabled={portBusy || !newPort} onclick={addExposedPort}>
            Expose
        </Button>
    </div>
    {#if portError}
        <span class="text-xs text-red-500">{portError}</span>
    {/if}
</div>
