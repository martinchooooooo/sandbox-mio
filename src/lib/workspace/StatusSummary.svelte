<script lang="ts">
    import { ensureProxyStatus, proxyStatusFor } from "$lib/proxy/status.svelte";
    import { openForwardsFor } from "$lib/network/state.svelte";
    import { deploymentFor } from "$lib/sandbox/state.svelte";
    import { translateStatus } from "$lib/sandbox";
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
        if (proxy.error !== null) return "unknown"; // TODO(i18n)
        const status = proxy.value;
        if (!status) return "…";
        if (!status.exists) return "not recorded"; // TODO(i18n)
        if (!status.running) return "stopped"; // TODO(i18n)
        return status.healthy === false ? "unhealthy" : "recorded"; // TODO(i18n)
    });
    const portCount = $derived(
        openForwardsFor(sandboxName).filter((f) => f.kind.kind === "exposed").length,
    );
    const deployment = $derived(deploymentFor(sandboxName));
    const remote = $derived(
        deployment ? `${deployment.host} (${translateStatus(deployment.status)})` : "Local", // TODO(i18n)
    );
</script>

<p class="text-muted-foreground flex flex-wrap items-center gap-x-2 text-xs" data-status-summary>
    <button type="button" class="hover:underline" data-summary-part="network" onclick={() => onOpen("network")}>
        Network: {network}<!-- TODO(i18n) -->
    </button>
    <span aria-hidden="true">·</span>
    <button type="button" class="hover:underline" data-summary-part="ports" onclick={() => onOpen("ports")}>
        {portCount} {portCount === 1 ? "port" : "ports"} open<!-- TODO(i18n) -->
    </button>
    {#if showRemote}
        <span aria-hidden="true">·</span>
        <button type="button" class="hover:underline" data-summary-part="remote" onclick={() => onOpen("remote")}>
            {remote}
        </button>
    {/if}
</p>
