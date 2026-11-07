<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import Button from "$lib/components/ui/button/button.svelte";
    import { parseError } from "$lib/errors";
    import { ensureTrafficState, flowsFor, watchFlows, type FlowEntry } from "$lib/proxy/traffic.svelte";
    import type { createActionRunner } from "$lib/actions.svelte";
    import { m } from "$lib/paraglide/messages.js";

    // Mirrors Rust `proxy::types::ProxyStatus` (camelCase per its own `serde(rename_all)`).
    interface ProxyStatus {
        running: boolean;
        proxyIp: string;
        network: string;
        provider: "podman" | "microsandbox";
        /** The microsandbox sidecar's SOCKS5 port; null for Podman. */
        hostPort: number | null;
        /** Whether a running microsandbox sidecar answers; null for Podman or a stopped sidecar. */
        healthy: boolean | null;
    }

    // `workspacePath`/`action` are declared to match `ProxyPanel.svelte`'s prop shape (and the
    // dashboard's call site); this panel reads neither.
    let {
        sandboxName,
    }: {
        sandboxName: string;
        workspacePath: string;
        action: ReturnType<typeof createActionRunner>;
    } = $props();

    const STATUS_CLASSES = ["2xx", "3xx", "4xx", "5xx", "error"];

    let status = $state<ProxyStatus | null>(null);
    let panelError = $state<string | null>(null);
    let query = $state("");
    let methodFilter = $state<string | null>(null);
    let statusFilter = $state<string | null>(null);
    let expandedId = $state<number | null>(null);
    let watching = "";

    // Newest first: this is a live log, and the store appends.
    const flows = $derived(flowsFor(sandboxName).filter(matches).slice().reverse());
    const methods = $derived([...new Set(flowsFor(sandboxName).map((f) => f.method))].sort());

    function statusClass(flow: FlowEntry): string {
        if (flow.error !== null) return "error";
        return flow.status === null ? "error" : `${Math.floor(flow.status / 100)}xx`;
    }

    function matches(flow: FlowEntry): boolean {
        const needle = query.trim().toLowerCase();
        if (needle && !`${flow.host}${flow.path}`.toLowerCase().includes(needle)) return false;
        if (methodFilter && flow.method !== methodFilter) return false;
        if (statusFilter && statusClass(flow) !== statusFilter) return false;
        return true;
    }

    function toggle<T>(current: T | null, value: T): T | null {
        return current === value ? null : value;
    }

    function timestamp(ts: number): string {
        return new Date(ts * 1000).toISOString();
    }

    async function refresh() {
        try {
            status = await invoke<ProxyStatus>("proxy_status", { name: sandboxName });
        } catch (e) {
            panelError = parseError(e).message;
            return;
        }
        // The backend call is idempotent; `watching` only avoids re-invoking on every effect run.
        if (!status.running || watching === sandboxName) return;
        watching = sandboxName;
        try {
            await ensureTrafficState();
            await watchFlows(sandboxName);
        } catch (e) {
            watching = "";
            panelError = parseError(e).message;
        }
    }

    $effect(() => {
        void sandboxName;
        refresh();
    });
</script>

{#if status?.running}
    <div class="flex flex-col gap-1.5 border-t pt-2" data-traffic-panel>
        <span class="text-muted-foreground text-xs shrink-0">{m.traffic_title()}</span>
        {#if status.healthy === false}
            <span class="text-xs text-destructive" data-traffic-unhealthy>
                {m.traffic_unhealthy()}
            </span>
        {/if}
        {#if status.provider === "microsandbox"}
            <span class="text-muted-foreground text-xs" data-traffic-scope>
                {m.traffic_scope_msb()}
            </span>
        {/if}

        <div class="flex flex-wrap items-center gap-1">
            <input
                class="w-40 rounded border bg-background px-2 py-1 text-xs"
                placeholder={m.traffic_filter_placeholder()}
                bind:value={query}
            />
            {#each methods as method (method)}
                <Button
                    variant={methodFilter === method ? "default" : "outline"}
                    size="sm"
                    data-method-filter={method}
                    onclick={() => (methodFilter = toggle(methodFilter, method))}
                >
                    {method}
                </Button>
            {/each}
            {#each STATUS_CLASSES as klass (klass)}
                <Button
                    variant={statusFilter === klass ? "default" : "outline"}
                    size="sm"
                    data-status-filter={klass}
                    onclick={() => (statusFilter = toggle(statusFilter, klass))}
                >
                    {klass === "error" ? m.status_error() : klass}
                </Button>
            {/each}
        </div>

        {#if flows.length > 0}
            <div class="flex flex-col gap-1">
                {#each flows as flow (flow.id)}
                    <div class="rounded bg-muted px-2 py-1 text-xs" data-flow-row={flow.id}>
                        <button
                            type="button"
                            class="flex w-full items-center gap-2 text-left"
                            onclick={() => (expandedId = toggle(expandedId, flow.id))}
                        >
                            <span class="w-12 shrink-0 font-mono">{flow.method}</span>
                            <span class="shrink-0 font-mono">{flow.host}</span>
                            <span class="flex-1 truncate font-mono text-muted-foreground" title={flow.path}>
                                {flow.path}
                            </span>
                            {#if flow.secretInjected}
                                <span class="shrink-0 rounded border px-1" title={m.traffic_secret_badge_title()}>
                                    {m.traffic_secret_badge()}
                                </span>
                            {/if}
                            <span class="w-10 shrink-0 text-right font-mono" data-flow-status>
                                {flow.status ?? m.status_error()}
                            </span>
                            <span class="w-14 shrink-0 text-right font-mono text-muted-foreground">
                                {flow.durationMs === null ? "" : `${Math.round(flow.durationMs)} ms`}
                            </span>
                        </button>
                        {#if expandedId === flow.id}
                            <div class="mt-1 flex flex-col gap-0.5 border-t pt-1 font-mono break-all">
                                <span>{flow.path}</span>
                                <span class="text-muted-foreground">{timestamp(flow.ts)}</span>
                                <span class="text-muted-foreground">
                                    {m.traffic_bytes({ sent: flow.reqBytes ?? 0, received: flow.respBytes ?? 0 })}
                                </span>
                                {#if flow.error}
                                    <span class="text-red-500">{flow.error}</span>
                                {/if}
                            </div>
                        {/if}
                    </div>
                {/each}
            </div>
        {:else}
            <span class="text-muted-foreground text-xs" data-traffic-empty>
                {m.traffic_empty()}
            </span>
        {/if}

        {#if panelError}
            <span class="text-xs text-red-500">{panelError}</span>
        {/if}
    </div>
{/if}
