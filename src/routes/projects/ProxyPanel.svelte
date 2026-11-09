<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import Button from "$lib/components/ui/button/button.svelte";
    import { parseError } from "$lib/errors";
    import type { createActionRunner } from "$lib/actions.svelte";
    import type { ProviderId } from "$lib/providers";
    import NetPolicyEditor from "./NetPolicyEditor.svelte";
    import { ensureProxyStatus, proxyStatusFor, refreshProxyStatus } from "$lib/proxy/status.svelte";
    import { m } from "$lib/paraglide/messages.js";

    // Mirrors Rust `proxy::rules::ProxyRule` — snake_case, unrenamed, matching `.mio/proxy-rules.json`
    // on disk and `Project.workspace_path`'s existing snake_case IPC convention.
    interface ProxyRule {
        host: string;
        path_prefix: string;
        header: string;
        secret_id: string;
    }

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

    // Microsandbox owns secret injection and egress itself, so the Podman-only
    // `.mio/proxy-rules.json` editor is hidden for it (docs/features/microsandbox-network-policy/02_dev.md, Phase D).
    const msb = $derived(provider === "microsandbox");

    const statusEntry = $derived(proxyStatusFor(sandboxName));
    const status = $derived(statusEntry.value);
    let rules = $state<ProxyRule[]>([]);
    let panelError = $state<string | null>(null);

    let newHost = $state("");
    let newPathPrefix = $state("");
    let newHeader = $state("");
    let newSecretId = $state("");
    let secretValue = $state("");
    let busy = $state(false);

    async function loadRules() {
        if (msb) return;
        try {
            rules = await invoke<ProxyRule[]>("list_proxy_rules", { workspacePath });
        } catch (e) {
            panelError = parseError(e).message;
        }
    }

    async function saveRules(next: ProxyRule[]) {
        busy = true;
        panelError = null;
        try {
            await invoke("set_proxy_rules", { name: sandboxName, workspacePath, rules: next });
            rules = next;
        } catch (e) {
            panelError = parseError(e).message;
        } finally {
            busy = false;
        }
    }

    async function addRule() {
        const host = newHost.trim();
        const secretId = newSecretId.trim();
        if (!host || !newHeader.trim() || !secretId) return;
        await saveRules([
            ...rules,
            { host, path_prefix: newPathPrefix.trim(), header: newHeader.trim(), secret_id: secretId },
        ]);
        newHost = "";
        newPathPrefix = "";
        newHeader = "";
        newSecretId = "";
    }

    async function removeRule(index: number) {
        await saveRules(rules.filter((_, i) => i !== index));
    }

    async function saveSecret() {
        const id = newSecretId.trim();
        const value = secretValue.trim();
        if (!id || !value) return;
        busy = true;
        panelError = null;
        try {
            await invoke("set_proxy_secret", { secretId: id, value });
            secretValue = "";
        } catch (e) {
            panelError = parseError(e).message;
        } finally {
            busy = false;
        }
    }

    $effect(() => {
        void sandboxName;
        void workspacePath;
        ensureProxyStatus(sandboxName);
        loadRules();
    });
</script>

<div class="flex flex-col gap-1.5" data-proxy-panel>
    <span class="text-muted-foreground text-xs shrink-0">{m.proxy_title()}</span>

    {#if status && !status.exists}
        <span class="text-muted-foreground text-xs" data-proxy-disabled>
            {#if msb}
                {m.proxy_off_msb()}
            {:else}
                {m.proxy_off_podman()}
            {/if}
        </span>
    {:else if status && !status.running}
        <span class="text-xs text-destructive" data-proxy-stopped>{m.proxy_stopped()}</span>
    {:else if status?.running && msb}
        <span class="text-muted-foreground text-xs" data-proxy-msb-chain>
            {m.proxy_chained()}
        </span>
    {:else if status?.running}
        <span class="text-muted-foreground text-xs" data-proxy-network>
            {m.proxy_network({ network: status.network })}
        </span>

        <span class="text-muted-foreground mt-2 border-t pt-2 text-xs shrink-0" data-secrets-heading>
            Secrets<!-- TODO(i18n) -->
        </span>

        {#if rules.length > 0}
            <div class="flex flex-col gap-1">
                {#each rules as rule, i (rule.host + rule.path_prefix + rule.header)}
                    <div class="flex items-center gap-2 rounded bg-muted px-2 py-1 text-xs">
                        <span class="flex-1 truncate font-mono">
                            {rule.host}{rule.path_prefix} → {rule.header} ({rule.secret_id})
                        </span>
                        <Button variant="ghost" size="icon" disabled={busy}
                            onclick={() => removeRule(i)}>✕</Button>
                    </div>
                {/each}
            </div>
        {:else}
            <span class="text-muted-foreground text-xs">{m.proxy_no_rules()}</span>
        {/if}

        <div class="flex flex-wrap items-center gap-2">
            <input class="w-28 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.proxy_host_placeholder()} bind:value={newHost} disabled={busy} />
            <input class="w-24 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.proxy_path_prefix_placeholder()} bind:value={newPathPrefix} disabled={busy} />
            <input class="w-28 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.proxy_header_placeholder()} bind:value={newHeader} disabled={busy} />
            <input class="w-28 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.secret_id_placeholder()} bind:value={newSecretId} disabled={busy} />
            <Button size="sm" disabled={busy} onclick={addRule}>
                {m.rule_add()}
            </Button>
        </div>

        <div class="flex items-center gap-2">
            <span class="text-muted-foreground text-xs shrink-0">{m.secret_set_value()}</span>
            <input class="flex-1 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                type="password" placeholder={m.secret_value_placeholder()} bind:value={secretValue}
                disabled={busy} />
            <Button size="sm" disabled={busy || !newSecretId.trim() || !secretValue.trim()} onclick={saveSecret}>
                {m.settings_save()}
            </Button>
        </div>

        <span class="text-muted-foreground text-xs">
            {m.proxy_unmonitored()}
        </span>
    {/if}

    {#if panelError ?? statusEntry.error}
        <span class="text-xs text-red-500">{panelError ?? statusEntry.error}</span>
    {/if}
</div>

{#if msb && provider}
    <NetPolicyEditor {sandboxName} {workspacePath} {provider}
        viewerOn={status?.exists ?? false} onReapplied={() => refreshProxyStatus(sandboxName)} />
{/if}
