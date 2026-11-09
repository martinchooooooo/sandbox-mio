<script lang="ts">
    import { ensureProxyStatus, proxyStatusFor } from "$lib/proxy/status.svelte";
    import { openForwardsFor } from "$lib/network/state.svelte";
    import { deploymentFor } from "$lib/sandbox/state.svelte";
    import { translateStatus } from "$lib/sandbox";
    import { m } from "$lib/paraglide/messages.js";
    import type { DashboardSectionId } from "./sections";

    let {
        sandboxName,
        showRemote,
        onOpen,
    }: {
        sandboxName: string;
        showRemote: boolean;
        onOpen: (id: DashboardSectionId) => void;
    } = $props();

    $effect(() => {
        ensureProxyStatus(sandboxName);
    });

    const proxy = $derived(proxyStatusFor(sandboxName));
    const network = $derived.by(() => {
        if (proxy.error !== null) return m.dashboard_summary_network_unknown();
        const status = proxy.value;
        if (!status) return "…";
        if (!status.exists) return m.dashboard_summary_network_off();
        if (!status.running) return m.dashboard_summary_network_stopped();
        return status.healthy === false
            ? m.dashboard_summary_network_unhealthy()
            : m.dashboard_summary_network_recorded();
    });
    const portCount = $derived(
        openForwardsFor(sandboxName).filter((f) => f.kind.kind === "exposed").length,
    );
    const deployment = $derived(deploymentFor(sandboxName));
    const remote = $derived(
        deployment ? `${deployment.host} (${translateStatus(deployment.status)})` : m.dashboard_summary_local(),
    );
</script>

<p class="text-muted-foreground flex flex-wrap items-center gap-x-2 text-xs" data-status-summary>
    <button type="button" class="hover:underline" data-summary-part="network" onclick={() => onOpen("network")}>
        {m.dashboard_summary_network({ status: network })}
    </button>
    <span aria-hidden="true">·</span>
    <button type="button" class="hover:underline" data-summary-part="ports" onclick={() => onOpen("ports")}>
        {m.dashboard_summary_ports({ count: portCount })}
    </button>
    {#if showRemote}
        <span aria-hidden="true">·</span>
        <button type="button" class="hover:underline" data-summary-part="remote" onclick={() => onOpen("remote")}>
            {remote}
        </button>
    {/if}
</p>
