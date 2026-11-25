<script lang="ts">
    import type { createActionRunner } from "$lib/actions.svelte";
    import type { ProviderId } from "$lib/providers";
    import NetPolicyEditor from "./NetPolicyEditor.svelte";
    import { ensureProxyStatus, proxyStatusFor } from "$lib/proxy/status.svelte";
    import { m } from "$lib/paraglide/messages.js";

    let {
        sandboxName,
        workspacePath,
        provider,
        action,
    }: {
        sandboxName: string;
        workspacePath: string;
        provider?: ProviderId;
        action: ReturnType<typeof createActionRunner>;
    } = $props();

    const statusEntry = $derived(proxyStatusFor(sandboxName));
    const status = $derived(statusEntry.value);
    const iron = $derived(status?.engine === "iron-proxy");

    $effect(() => {
        void sandboxName;
        ensureProxyStatus(sandboxName);
    });
</script>

<div class="flex flex-col gap-1.5" data-proxy-panel>
    <span class="text-muted-foreground text-xs shrink-0">{m.proxy_title()}</span>

    {#if status && !status.running}
        <span class="text-xs text-destructive" data-proxy-stopped>{m.proxy_stopped()}</span>
    {:else if status?.running}
        <span class="text-muted-foreground text-xs" data-proxy-network>
            {m.proxy_network({ network: status.network })}
        </span>
        {#if status.host}
            <span class="text-muted-foreground text-xs" data-proxy-host>
                Running on {status.host}. <!-- TODO(i18n) -->
            </span>
        {/if}
        {#if iron}
            <span class="text-muted-foreground text-xs" data-proxy-non-http-blocked>
                Traffic that ignores the proxy (raw sockets, git over SSH) is blocked: the sidecar is the sandbox's only way out. <!-- TODO(i18n) -->
            </span>
        {/if}
    {/if}

    {#if statusEntry.error}
        <span class="text-xs text-red-500">{statusEntry.error}</span>
    {/if}
</div>

<!-- A stopped iron-proxy sidecar keeps the editor: saving starts it (docs/features/podman-egress-boundary/02_dev.md, Resolved Decision 13). -->
{#if provider && iron && status?.exists}
    <NetPolicyEditor {sandboxName} />
{/if}
