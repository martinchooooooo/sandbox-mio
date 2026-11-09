<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import Button from "$lib/components/ui/button/button.svelte";
    import { parseError } from "$lib/errors";
    import type { ProviderId } from "$lib/providers";
    import { Switch } from "$lib/components/ui/switch";
    import { workspaceApps } from "$lib/workspace/state.svelte";
    import { m } from "$lib/paraglide/messages.js";

    // Mirrors Rust `net_policy::NetPolicy` (camelCase, `.mio/net-policy.json` on disk).
    type Port = number | string;
    interface NetRule {
        action: "allow" | "deny";
        host?: string;
        cidr?: string;
        group?: "public" | "private" | "host";
        protocol?: "tcp" | "udp";
        ports?: Port[];
    }
    interface SecretBinding {
        secretId: string;
        envVar: string;
        hosts: string[];
        headers: string[];
        query: boolean;
        body: boolean;
    }
    type ViolationAction = "block" | "block-and-log" | "block-and-terminate";
    interface NetPolicy {
        version: number;
        egress: "open" | "allowlist";
        rules: NetRule[];
        strictHostnames: boolean;
        secrets: SecretBinding[];
        violationAction: ViolationAction;
        denyMessage?: string;
        interactive?: boolean;
    }

    let {
        sandboxName,
        workspacePath,
        provider,
        viewerOn,
        onReapplied,
    }: {
        sandboxName: string;
        workspacePath: string;
        provider: ProviderId;
        viewerOn: boolean;
        onReapplied: () => void;
    } = $props();

    // Mirrors Rust `provider::types::SecretViolation`.
    interface SecretViolation {
        ts: string;
        action: string;
        envVar: string;
        host: string;
        method: string;
        path: string;
        location: string;
    }
    const VIOLATION_POLL_MS = 5000;
    let violations = $state<SecretViolation[]>([]);

    $effect(() => {
        const name = sandboxName;
        const poll = async () => {
            try {
                violations = await invoke<SecretViolation[]>("net_policy_violations", { name });
            } catch {
                /* the list is advisory; a failed poll keeps the last one */
            }
        };
        poll();
        const timer = setInterval(poll, VIOLATION_POLL_MS);
        return () => clearInterval(timer);
    });

    // Re-apply recreates the VM (docs/features/microsandbox-network-policy/02_dev.md, Phase G).
    let wantViewer = $state(false);
    let confirming = $state(false);
    $effect(() => {
        wantViewer = viewerOn;
    });
    const openPanes = $derived(workspaceApps().filter((a) => a.sandbox === sandboxName).map((a) => a.label));
    const viewerChange = $derived(wantViewer !== viewerOn);

    async function reapply() {
        busy = true;
        editorError = null;
        try {
            await invoke("reapply_sandbox_network", { name: sandboxName, viewer: wantViewer });
            confirming = false;
            await refresh();
            onReapplied();
        } catch (e) {
            editorError = parseError(e).message;
        } finally {
            busy = false;
        }
    }

    // Hosts known to echo request headers back, which would reflect a substituted secret into the
    // guest (docs/features/microsandbox-network-policy/02_dev.md, Secret handling invariants).
    const ECHO_HOSTS = ["httpbin.org", "postman-echo.com"];

    let policy = $state<NetPolicy | null>(null);
    let drift = $state(false);
    let dirty = $state(false);
    let busy = $state(false);
    let editorError = $state<string | null>(null);

    let newAction = $state<"allow" | "deny">("allow");
    let newTarget = $state("");
    let newPorts = $state("443");

    let newSecretId = $state("");
    let newEnvVar = $state("");
    let newSecretHosts = $state("");
    let secretValue = $state("");

    async function refresh() {
        try {
            policy = await invoke<NetPolicy>("get_net_policy", { workspacePath });
            drift = await invoke<boolean>("net_policy_drift", { name: sandboxName, workspacePath });
            dirty = false;
        } catch (e) {
            editorError = parseError(e).message;
        }
    }

    function mutate(fn: (p: NetPolicy) => void) {
        if (!policy) return;
        fn(policy);
        dirty = true;
    }

    async function save() {
        if (!policy) return;
        busy = true;
        editorError = null;
        try {
            await invoke("set_net_policy", { workspacePath, policy, provider });
            await refresh();
        } catch (e) {
            editorError = parseError(e).message;
        } finally {
            busy = false;
        }
    }

    function targetLabel(rule: NetRule): string {
        return rule.host ?? rule.cidr ?? (rule.group ? `@${rule.group}` : "?");
    }

    function portsLabel(rule: NetRule): string {
        const proto = rule.protocol ?? "any";
        return rule.ports?.length ? `${proto}:${rule.ports.join(",")}` : proto;
    }

    function parsePorts(text: string): Port[] | undefined {
        const parts = text.split(",").map((p) => p.trim()).filter(Boolean);
        if (parts.length === 0) return undefined;
        return parts.map((p) => (/^\d+$/.test(p) ? Number(p) : p));
    }

    function addRule() {
        const target = newTarget.trim();
        if (!target) return;
        const ports = parsePorts(newPorts);
        const rule: NetRule = { action: newAction };
        if (["public", "private", "host"].includes(target)) rule.group = target as NetRule["group"];
        else if (/^[0-9a-fA-F:.]+(\/\d+)?$/.test(target) && /[\d:]/.test(target[0])) rule.cidr = target;
        else rule.host = target;
        if (ports) {
            rule.protocol = "tcp";
            rule.ports = ports;
        }
        mutate((p) => p.rules.push(rule));
        newTarget = "";
    }

    function moveRule(index: number, delta: number) {
        mutate((p) => {
            const next = index + delta;
            if (next < 0 || next >= p.rules.length) return;
            const [rule] = p.rules.splice(index, 1);
            p.rules.splice(next, 0, rule);
        });
    }

    function addBinding() {
        const secretId = newSecretId.trim();
        const envVar = newEnvVar.trim();
        const hosts = newSecretHosts.split(",").map((h) => h.trim()).filter(Boolean);
        if (!secretId || !envVar || hosts.length === 0) return;
        mutate((p) =>
            p.secrets.push({ secretId, envVar, hosts, headers: ["authorization"], query: false, body: false }),
        );
        newEnvVar = "";
        newSecretHosts = "";
    }

    async function saveSecretValue() {
        const id = newSecretId.trim();
        const value = secretValue.trim();
        if (!id || !value) return;
        busy = true;
        editorError = null;
        try {
            await invoke("set_proxy_secret", { secretId: id, value });
            secretValue = "";
        } catch (e) {
            editorError = parseError(e).message;
        } finally {
            busy = false;
        }
    }

    const echoWarning = $derived.by(() => {
        if (!policy || policy.secrets.length === 0) return null;
        const allowed = policy.rules.filter((r) => r.action === "allow").map((r) => r.host);
        const reachable = policy.egress === "open" ? ECHO_HOSTS : ECHO_HOSTS.filter((h) => allowed.includes(h));
        return reachable.length ? reachable.join(", ") : null;
    });

    $effect(() => {
        void sandboxName;
        void workspacePath;
        refresh();
    });
</script>

<div class="flex flex-col gap-1.5 border-t pt-2" data-net-policy-editor>
    <span class="text-muted-foreground text-xs shrink-0">{m.netpolicy_title()}</span>

    {#if drift}
        <span class="rounded bg-amber-100 px-2 py-1 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-200" data-net-policy-drift>
            {m.netpolicy_drift()}
        </span>
    {/if}

    {#if policy}
        <div class="flex items-center gap-2" data-net-policy-mode>
            <span class="text-muted-foreground text-xs">{m.netpolicy_mode_label()}</span>
            {#each [["open", m.egress_open()], ["allowlist", m.egress_allowlist()]] as const as [value, label] (value)}
                <Button
                    data-net-policy-mode-option={value}
                    aria-pressed={policy.egress === value}
                    variant={policy.egress === value ? "default" : "outline"}
                    size="sm"
                    disabled={busy}
                    onclick={() => mutate((p) => (p.egress = value))}
                >{label}</Button>
            {/each}
        </div>

        <span class="text-muted-foreground text-xs">
            {#if policy.egress === "allowlist"}
                {m.netpolicy_mode_allowlist_hint()}
            {:else}
                {m.netpolicy_mode_open_hint()}
            {/if}
        </span>

        {#if policy.rules.length > 0}
            <div class="flex flex-col gap-1" data-net-policy-rules>
                {#each policy.rules as rule, i (i)}
                    <div class="flex items-center gap-2 rounded bg-muted px-2 py-1 text-xs" data-net-policy-rule>
                        <span class={rule.action === "deny" ? "text-destructive" : ""}>{rule.action}</span>
                        <span class="flex-1 truncate font-mono">{targetLabel(rule)}</span>
                        <span class="text-muted-foreground font-mono">{portsLabel(rule)}</span>
                        <Button variant="ghost" size="icon" disabled={busy || i === 0} onclick={() => moveRule(i, -1)}>↑</Button>
                        <Button variant="ghost" size="icon" disabled={busy || i === policy.rules.length - 1} onclick={() => moveRule(i, 1)}>↓</Button>
                        <Button variant="ghost" size="icon" disabled={busy}
                            onclick={() => mutate((p) => p.rules.splice(i, 1))}>✕</Button>
                    </div>
                {/each}
            </div>
        {:else}
            <span class="text-muted-foreground text-xs">{m.netpolicy_no_rules()}</span>
        {/if}

        <div class="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="outline" disabled={busy}
                onclick={() => (newAction = newAction === "allow" ? "deny" : "allow")}>{newAction}</Button>
            <input class="w-40 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.netpolicy_target_placeholder()} bind:value={newTarget} disabled={busy} />
            <input class="w-20 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.netpolicy_ports_placeholder()} bind:value={newPorts} disabled={busy} />
            <Button size="sm" disabled={busy} onclick={addRule}>
                {m.rule_add()}
            </Button>
        </div>

        <span class="text-muted-foreground mt-2 border-t pt-2 text-xs shrink-0" data-secrets-heading>
            {m.dashboard_secrets_title()}
        </span>
        <span class="text-muted-foreground text-xs">{m.netpolicy_secret_bindings()}</span>
        {#each policy.secrets as binding, i (binding.envVar)}
            <div class="flex items-center gap-2 rounded bg-muted px-2 py-1 text-xs" data-net-policy-secret>
                <span class="flex-1 truncate font-mono">
                    ${binding.envVar} ← {binding.secretId} → {binding.hosts.join(", ")}
                </span>
                <Button variant="ghost" size="icon" disabled={busy}
                    onclick={() => mutate((p) => p.secrets.splice(i, 1))}>✕</Button>
            </div>
        {/each}
        <div class="flex flex-wrap items-center gap-2">
            <input class="w-24 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.secret_id_placeholder()} bind:value={newSecretId} disabled={busy} />
            <input class="w-28 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.netpolicy_env_var_placeholder()} bind:value={newEnvVar} disabled={busy} />
            <input class="w-40 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.netpolicy_secret_hosts_placeholder()} bind:value={newSecretHosts} disabled={busy} />
            <Button size="sm" disabled={busy} onclick={addBinding}>
                {m.netpolicy_bind()}
            </Button>
        </div>
        <div class="flex items-center gap-2">
            <span class="text-muted-foreground text-xs shrink-0">{m.secret_set_value()}</span>
            <input class="flex-1 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                type="password" placeholder={m.secret_value_placeholder()} bind:value={secretValue}
                disabled={busy} />
            <Button size="sm" disabled={busy || !newSecretId.trim() || !secretValue.trim()} onclick={saveSecretValue}>
                {m.settings_save()}
            </Button>
        </div>
        {#if echoWarning}
            <span class="text-xs text-amber-700 dark:text-amber-300" data-net-policy-echo-warning>
                {m.netpolicy_echo_warning({ hosts: echoWarning })}
            </span>
        {/if}

        <div class="flex items-center gap-2">
            <span class="text-muted-foreground text-xs">{m.netpolicy_violation_action_label()}</span>
            {#each [["block", m.netpolicy_violation_block()], ["block-and-log", m.netpolicy_violation_block_and_log()], ["block-and-terminate", m.netpolicy_violation_block_and_stop()]] as const as [value, label] (value)}
                <Button
                    aria-pressed={policy.violationAction === value}
                    variant={policy.violationAction === value ? "default" : "outline"}
                    size="sm"
                    disabled={busy}
                    onclick={() => mutate((p) => (p.violationAction = value))}
                >{label}</Button>
            {/each}
        </div>

        <div class="flex items-center gap-2">
            <Button size="sm" disabled={busy || !dirty} onclick={save} data-net-policy-save>
                {m.netpolicy_save()}
            </Button>
            <span class="text-muted-foreground text-xs">
                {m.netpolicy_save_hint()}
            </span>
        </div>

        {#if violations.length > 0}
            <div class="flex flex-col gap-1" data-net-policy-violations>
                <span class="text-xs text-destructive">{m.netpolicy_violations_title()}</span>
                {#each violations as v, i (v.ts + i)}
                    <div class="flex items-center gap-2 rounded bg-muted px-2 py-1 text-xs font-mono" data-net-policy-violation>
                        <span class="text-muted-foreground">{v.ts.slice(11, 19)}</span>
                        <span>${v.envVar}</span>
                        <span class="flex-1 truncate">{v.method} {v.host}{v.path} ({v.location})</span>
                        <span class="text-muted-foreground">{v.action}</span>
                    </div>
                {/each}
            </div>
        {/if}

        <div class="flex flex-col gap-1.5 border-t pt-2" data-net-policy-reapply>
            <div class="flex items-center gap-2">
                <span class="text-muted-foreground text-xs">{m.proxy_inspect_traffic()}</span>
                <Switch data-net-policy-viewer checked={wantViewer} disabled={busy}
                    onCheckedChange={(v) => (wantViewer = v)} />
            </div>
            {#if !confirming}
                <Button size="sm" variant="outline" disabled={busy || dirty || (!drift && !viewerChange)}
                    onclick={() => (confirming = true)} data-net-policy-reapply-button>
                    {m.netpolicy_reapply()}
                </Button>
            {:else}
                <span class="text-xs" data-net-policy-reapply-confirm>
                    {m.netpolicy_reapply_confirm()}
                    {#if openPanes.length > 0}
                        {m.netpolicy_reapply_open_panes({ panes: openPanes.join(", ") })}
                    {/if}
                </span>
                <div class="flex gap-2">
                    <Button size="sm" variant="destructive" loading={busy} disabled={busy} onclick={reapply}
                        data-net-policy-reapply-confirm-button>
                        {m.netpolicy_recreate()}
                    </Button>
                    <Button size="sm" variant="ghost" disabled={busy} onclick={() => (confirming = false)}>
                        {m.projects_cancel()}
                    </Button>
                </div>
            {/if}
        </div>
    {/if}

    {#if editorError}
        <span class="text-xs text-red-500">{editorError}</span>
    {/if}
</div>
