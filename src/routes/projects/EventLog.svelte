<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Channel, invoke } from "@tauri-apps/api/core";
    import { onMount, tick } from "svelte";
    import { parseError } from "$lib/errors";
    import { m } from "$lib/paraglide/messages.js";

    let {
        onStatusChange,
    }: {
        // Reports a container lifecycle event so the page can update the matching sandbox's status.
        onStatusChange: (name: string, status: string) => void;
    } = $props();

    let events = $state<string[]>([]);
    let showLog = $state(false);
    let listening = $state(false);
    let logEl = $state<HTMLDivElement>();

    const ACTION_STATUS: Record<string, string> = {
        start: "running",
        unpause: "running",
        stop: "exited",
        die: "exited",
        died: "exited",
        kill: "exited",
        pause: "paused",
        create: "created",
        destroy: "removed",
    };

    async function startListening() {
        if (listening) return;
        listening = true;
        const channel = new Channel<Record<string, unknown>>();
        channel.onmessage = async (ev) => {
            const type = String((ev.Type ?? ev.type) ?? "");
            const action = String((ev.Action ?? ev.action ?? ev.Status ?? ev.status) ?? "");
            const attrs = (ev.Actor as any)?.Attributes ?? (ev.actor as any)?.attributes ?? {};
            const name = String(attrs.name ?? (ev.Name as any) ?? (ev.name as any) ?? ev.id ?? "");
            const time = new Date().toLocaleTimeString();
            events.push(`[${time}] ${type} ${action}${name ? ` — ${name}` : ""}`);

            if (type === "container" && name && action in ACTION_STATUS) {
                onStatusChange(name, ACTION_STATUS[action]);
            }

            await tick();
            logEl?.scrollTo({ top: logEl.scrollHeight, behavior: "smooth" });
        };
        try {
            await invoke("listen_events", { onEvent: channel });
        } catch (e) {
            events.push(`Error: ${parseError(e).message}`);
            listening = false;
        }
    }

    function toggleLog() {
        showLog = !showLog;
    }

    onMount(startListening);
</script>

<div class="fixed bottom-0 left-0 right-0">
    {#if showLog}
        <div class="h-44 bg-background border-t flex flex-col">
            <div class="flex items-center justify-between px-4 py-1.5 border-b text-xs text-muted-foreground shrink-0">
                <span class="font-medium">
                    {m.event_log()}
                    <span class={listening ? "text-green-500" : "text-muted-foreground"}>
                        {listening ? m.event_log_live() : m.event_log_stopped()}
                    </span>
                </span>
                <button onclick={() => (showLog = false)} class="hover:text-foreground leading-none">✕</button>
            </div>
            <div bind:this={logEl} class="flex-1 overflow-y-auto px-4 py-2 font-mono text-xs space-y-0.5">
                {#if events.length === 0}
                    <p class="text-muted-foreground">{m.event_log_waiting()}</p>
                {:else}
                    {#each events as line}
                        <div>{line}</div>
                    {/each}
                {/if}
            </div>
        </div>
    {/if}

    <div class="flex justify-end px-4 py-2 bg-background border-t">
        <Button variant="outline" size="sm" onclick={toggleLog}>
            {showLog ? m.event_log_hide() : m.event_log()}
        </Button>
    </div>
</div>
