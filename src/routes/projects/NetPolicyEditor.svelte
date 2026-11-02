<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import Button from "$lib/components/ui/button/button.svelte";
    import { parseError } from "$lib/errors";
    import type { ProviderId } from "$lib/providers";
    import { Switch } from "$lib/components/ui/switch";
    import { workspaceApps } from "$lib/workspace/state.svelte";

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
    <!-- TODO(i18n) -->
    <span class="text-muted-foreground text-xs shrink-0">Network policy</span>

    {#if drift}
        <span class="rounded bg-amber-100 px-2 py-1 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-200" data-net-policy-drift>
            <!-- TODO(i18n) -->
            Changes pending — re-apply to recreate the sandbox with them.
        </span>
    {/if}

    {#if policy}
        <div class="flex items-center gap-2" data-net-policy-mode>
            <!-- TODO(i18n) -->
            <span class="text-muted-foreground text-xs">Mode</span>
            {#each [["open", "Open"], ["allowlist", "Allowlist"]] as const as [value, label] (value)}
                <Button
                    data-net-policy-mode-option={value}
                    aria-pressed={policy.egress === value}
                    variant={policy.egress === value ? "default" : "outline"}
                    size="sm"
                    disabled={busy}
                    onclick={() => mutate((p) => (p.egress = value))}
                >{label}</Button><!-- TODO(i18n) -->
            {/each}
        </div>

        <span class="text-muted-foreground text-xs">
            {#if policy.egress === "allowlist"}
                Only hosts allowed below are reachable; rules apply top to bottom, first match wins. <!-- TODO(i18n) -->
            {:else}
                Every public host is reachable except what a deny rule below blocks. <!-- TODO(i18n) -->
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
            <!-- TODO(i18n) -->
            <span class="text-muted-foreground text-xs">No rules.</span>
        {/if}

        <div class="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="outline" disabled={busy}
                onclick={() => (newAction = newAction === "allow" ? "deny" : "allow")}>{newAction}</Button>
            <input class="w-40 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder="host, *.suffix, CIDR or group" bind:value={newTarget} disabled={busy} /><!-- TODO(i18n) -->
            <input class="w-20 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder="ports" bind:value={newPorts} disabled={busy} /><!-- TODO(i18n) -->
            <Button size="sm" disabled={busy} onclick={addRule}>
                <!-- TODO(i18n) -->
                Add rule
            </Button>
        </div>

        <!-- TODO(i18n) -->
        <span class="text-muted-foreground text-xs mt-1">Secret bindings</span>
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
                placeholder="secret id" bind:value={newSecretId} disabled={busy} /><!-- TODO(i18n) -->
            <input class="w-28 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder="ENV_VAR" bind:value={newEnvVar} disabled={busy} /><!-- TODO(i18n) -->
            <input class="w-40 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder="hosts, comma separated" bind:value={newSecretHosts} disabled={busy} /><!-- TODO(i18n) -->
            <Button size="sm" disabled={busy} onclick={addBinding}>
                <!-- TODO(i18n) -->
                Bind
            </Button>
        </div>
        <div class="flex items-center gap-2">
            <!-- TODO(i18n) -->
            <span class="text-muted-foreground text-xs shrink-0">Set secret value</span>
            <input class="flex-1 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                type="password" placeholder="value for the secret id above" bind:value={secretValue}
                disabled={busy} /><!-- TODO(i18n) -->
            <Button size="sm" disabled={busy || !newSecretId.trim() || !secretValue.trim()} onclick={saveSecretValue}>
                <!-- TODO(i18n) -->
                Save
            </Button>
        </div>
        {#if echoWarning}
            <span class="text-xs text-amber-700 dark:text-amber-300" data-net-policy-echo-warning>
                <!-- TODO(i18n) -->
                {echoWarning} echoes requests back, so a bound secret's real value would reach the sandbox.
            </span>
        {/if}

        <div class="flex items-center gap-2">
            <!-- TODO(i18n) -->
            <span class="text-muted-foreground text-xs">On a misused secret</span>
            {#each [["block", "Block"], ["block-and-log", "Block and log"], ["block-and-terminate", "Block and stop"]] as const as [value, label] (value)}
                <Button
                    aria-pressed={policy.violationAction === value}
                    variant={policy.violationAction === value ? "default" : "outline"}
                    size="sm"
                    disabled={busy}
                    onclick={() => mutate((p) => (p.violationAction = value))}
                >{label}</Button><!-- TODO(i18n) -->
            {/each}
        </div>

        <div class="flex items-center gap-2">
            <Button size="sm" disabled={busy || !dirty} onclick={save} data-net-policy-save>
                <!-- TODO(i18n) -->
                Save policy
            </Button>
            <span class="text-muted-foreground text-xs">
                <!-- TODO(i18n) -->
                Saving changes the workspace file only; the running sandbox keeps its policy until re-applied.
            </span>
        </div>

        {#if violations.length > 0}
            <div class="flex flex-col gap-1" data-net-policy-violations>
                <!-- TODO(i18n) -->
                <span class="text-xs text-destructive">Blocked secret uses</span>
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
                <!-- TODO(i18n) -->
                <span class="text-muted-foreground text-xs">Inspect traffic</span>
                <Switch data-net-policy-viewer checked={wantViewer} disabled={busy}
                    onCheckedChange={(v) => (wantViewer = v)} />
            </div>
            {#if !confirming}
                <Button size="sm" variant="outline" disabled={busy || dirty || (!drift && !viewerChange)}
                    onclick={() => (confirming = true)} data-net-policy-reapply-button>
                    <!-- TODO(i18n) -->
                    Re-apply network settings
                </Button>
            {:else}
                <span class="text-xs" data-net-policy-reapply-confirm>
                    <!-- TODO(i18n) -->
                    This recreates the sandbox VM around its workspace; tools installed outside the workspace are reinstalled from the lock file.
                    {#if openPanes.length > 0}
                        Open panes will close: {openPanes.join(", ")}. <!-- TODO(i18n) -->
                    {/if}
                </span>
                <div class="flex gap-2">
                    <Button size="sm" variant="destructive" loading={busy} disabled={busy} onclick={reapply}
                        data-net-policy-reapply-confirm-button>
                        <!-- TODO(i18n) -->
                        Recreate
                    </Button>
                    <Button size="sm" variant="ghost" disabled={busy} onclick={() => (confirming = false)}>
                        <!-- TODO(i18n) -->
                        Cancel
                    </Button>
                </div>
            {/if}
        </div>
    {/if}

    {#if editorError}
        <span class="text-xs text-red-500">{editorError}</span>
    {/if}
</div>
