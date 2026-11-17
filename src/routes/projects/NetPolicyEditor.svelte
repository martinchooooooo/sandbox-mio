<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import Button from "$lib/components/ui/button/button.svelte";
    import { parseError } from "$lib/errors";
    import type { ProviderId } from "$lib/providers";
    import { Switch } from "$lib/components/ui/switch";
    import PencilIcon from "@lucide/svelte/icons/pencil";
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
        sidecar,
        viewerOn,
        onReapplied,
    }: {
        sandboxName: string;
        workspacePath: string;
        provider: ProviderId;
        /**
         * The iron-proxy dialect: host allows and CIDR denies only, applied without a recreate, no
         * violation actions (docs/features/podman-egress-boundary/02_dev.md, Resolved Decision 6).
         */
        sidecar: boolean;
        viewerOn: boolean;
        onReapplied: () => void;
    } = $props();

    // Mirrors Rust `proxy::iron::ApplyReport` and `net_policy::UnstampedBinding`.
    interface ApplyReport {
        missingSecrets: string[];
    }
    interface UnstampedBinding {
        envVar: string;
        placeholder: string;
    }
    let report = $state<ApplyReport | null>(null);
    let unstamped = $state<UnstampedBinding[]>([]);
    const unstampedByVar = $derived(new Map(unstamped.map((u) => [u.envVar, u.placeholder])));

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
        if (sidecar) return;
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
    // Writable derived: follows the prop, and the switch overrides it until the prop changes.
    let wantViewer = $derived(viewerOn);
    let confirming = $state(false);
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

    const SIDECAR_TARGET_PLACEHOLDER = "host, *.suffix, or CIDR to deny"; // TODO(i18n)

    let policy = $state<NetPolicy | null>(null);
    let drift = $state(false);
    let dirty = $state(false);
    let busy = $state(false);
    let editorError = $state<string | null>(null);

    let newAction = $state<"allow" | "deny">("allow");
    let newTarget = $state("");
    let newPorts = $state("443");

    // One form serves adding and editing; `editingVar` names the binding being edited, if any.
    let editingVar = $state<string | null>(null);
    let formEnvVar = $state("");
    let formHosts = $state("");
    let formValue = $state("");
    let showValue = $state(false);
    const formHostList = $derived(formHosts.split(",").map((h) => h.trim().toLowerCase()).filter(Boolean));
    const formReady = $derived(
        formEnvVar.trim() !== "" && formHostList.length > 0 && (editingVar !== null || formValue.trim() !== ""),
    );

    async function refresh() {
        try {
            const name = sandboxName;
            [policy, drift, unstamped] = await Promise.all([
                invoke<NetPolicy>("get_net_policy", { workspacePath }),
                invoke<boolean>("net_policy_drift", { name, workspacePath }),
                sidecar
                    ? invoke<UnstampedBinding[]>("net_policy_unstamped_bindings", { name, workspacePath })
                    : Promise.resolve([]),
            ]);
            dirty = false;
        } catch (e) {
            editorError = parseError(e).message;
        }
    }

    function mutate(fn: (p: NetPolicy) => void) {
        if (!policy) return;
        fn(policy);
        dirty = true;
        report = null;
    }

    async function save() {
        if (!policy) return;
        busy = true;
        editorError = null;
        try {
            if (sidecar) {
                if (dirty) await invoke("set_net_policy", { workspacePath, policy, provider });
                report = await invoke<ApplyReport>("apply_sandbox_policy", { name: sandboxName, workspacePath });
            } else {
                await invoke("set_net_policy", { workspacePath, policy, provider });
            }
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
        const ports = sidecar ? undefined : parsePorts(newPorts);
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

    function resetSecretForm() {
        editingVar = null;
        formEnvVar = "";
        formHosts = "";
        formValue = "";
        showValue = false;
    }

    function startEdit(binding: SecretBinding) {
        resetSecretForm();
        editingVar = binding.envVar;
        formEnvVar = binding.envVar;
        formHosts = binding.hosts.join(", ");
    }

    /** Mirrors `allowed_by_rule` in `src-tauri/src/net_policy.rs`, which rejects an unreachable secret host. */
    function allowedByRule(p: NetPolicy, host: string): boolean {
        return p.rules.some((r) => {
            if (r.action !== "allow" || !r.host) return false;
            const h = r.host.toLowerCase();
            return h.startsWith("*.") ? host.endsWith(h.slice(1)) : h === host;
        });
    }

    /** Mirrors `NetRule::starter_allow` in `src-tauri/src/net_policy.rs`. */
    function allowRule(host: string): NetRule {
        return sidecar ? { action: "allow", host } : { action: "allow", host, protocol: "tcp", ports: [443] };
    }

    /**
     * Saves a secret change at once, with any other unsaved edits, then applies it on the sidecar.
     * The policy is written first, so a rejected binding never replaces or deletes a stored value.
     */
    async function commitSecretChange(
        edit: (p: NetPolicy) => void,
        { store, removedId }: { store?: { secretId: string; value: string }; removedId?: string } = {},
    ): Promise<boolean> {
        if (!policy) return false;
        const before = $state.snapshot(policy) as NetPolicy;
        const wasDirty = dirty;
        let written = false;
        busy = true;
        editorError = null;
        try {
            mutate(edit);
            await invoke("set_net_policy", { workspacePath, policy, provider });
            written = true;
            if (store) await invoke("set_proxy_secret", store);
            // The sidecar reads values only at an apply
            // (docs/features/podman-egress-boundary/02_dev.md, Resolved Decision 15).
            if (sidecar) report = await invoke<ApplyReport>("apply_sandbox_policy", { name: sandboxName, workspacePath });
            // A microsandbox keeps starting from its applied policy until re-applied, and a start
            // refuses a binding without a value, so only the sidecar drops a removed value.
            if (removedId && sidecar) await invoke("delete_proxy_secret", { secretId: removedId });
            await refresh();
            return true;
        } catch (e) {
            editorError = parseError(e).message;
            if (written) {
                await refresh();
            } else {
                policy = before;
                dirty = wasDirty;
            }
            return false;
        } finally {
            busy = false;
        }
    }

    async function saveSecret() {
        if (!policy || !formReady) return;
        const envVar = formEnvVar.trim();
        const hosts = formHostList;
        const value = formValue.trim();
        const existing = editingVar === null ? undefined : policy.secrets.find((b) => b.envVar === editingVar);
        if (!existing && policy.secrets.some((b) => b.envVar === envVar)) {
            editorError = m.netpolicy_secret_exists({ envVar });
            return;
        }
        // Scoped to the sandbox: the value store is app-wide, and an id shared across sandboxes
        // would let an edit here silently change another sandbox's secret.
        const secretId = existing?.secretId ?? `${sandboxName}-${envVar.toLowerCase()}`;
        const ok = await commitSecretChange(
            (p) => {
                const target = existing && p.secrets.find((b) => b.envVar === existing.envVar);
                if (target) target.hosts = hosts;
                else p.secrets.push({ secretId, envVar, hosts, headers: ["authorization"], query: false, body: false });
                if (p.egress === "allowlist") {
                    for (const host of hosts) if (!allowedByRule(p, host)) p.rules.push(allowRule(host));
                }
            },
            { store: value ? { secretId, value } : undefined },
        );
        if (ok) resetSecretForm();
    }

    async function removeSecret(binding: SecretBinding) {
        if (!policy) return;
        const stillUsed = policy.secrets.some((b) => b.envVar !== binding.envVar && b.secretId === binding.secretId);
        const ok = await commitSecretChange(
            (p) => p.secrets.splice(p.secrets.findIndex((b) => b.envVar === binding.envVar), 1),
            { removedId: stillUsed ? undefined : binding.secretId },
        );
        if (ok && editingVar === binding.envVar) resetSecretForm();
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

{#snippet secretForm()}
    {@const editing = editingVar !== null}
    <div class="flex flex-col gap-1.5 rounded border px-2 py-1.5" data-secret-form={editing ? "edit" : "add"}>
        <div class="flex flex-wrap items-center gap-2">
            <input class="w-36 rounded border bg-background px-2 py-1 font-mono text-xs disabled:opacity-50"
                placeholder={m.netpolicy_env_var_placeholder()} bind:value={formEnvVar}
                disabled={busy || editing} data-secret-env-var />
            <input class="min-w-40 flex-1 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.netpolicy_secret_hosts_placeholder()} bind:value={formHosts}
                disabled={busy} data-secret-hosts />
        </div>
        <div class="flex items-center gap-2">
            <input class="flex-1 rounded border bg-background px-2 py-1 font-mono text-xs disabled:opacity-50"
                type={showValue ? "text" : "password"} autocomplete="off" spellcheck="false"
                placeholder={editing ? m.netpolicy_secret_replace_placeholder() : m.netpolicy_secret_value_placeholder()}
                bind:value={formValue} disabled={busy} data-secret-value />
            <label class="text-muted-foreground flex shrink-0 items-center gap-1.5 text-xs">
                <Switch checked={showValue} disabled={busy} onCheckedChange={(v) => (showValue = v)} data-secret-show />
                {m.netpolicy_secret_show()}
            </label>
        </div>
        <div class="flex items-center gap-2">
            <Button size="sm" disabled={busy || !formReady} onclick={saveSecret} data-secret-save>
                {editing ? m.netpolicy_secret_save() : m.netpolicy_secret_add()}
            </Button>
            {#if editing}
                <Button size="sm" variant="ghost" disabled={busy} onclick={resetSecretForm} data-secret-cancel>
                    {m.projects_cancel()}
                </Button>
            {/if}
        </div>
    </div>
{/snippet}

<div class="flex flex-col gap-1.5 border-t pt-2" data-net-policy-editor>
    <span class="text-muted-foreground text-xs shrink-0">{m.netpolicy_title()}</span>

    {#if drift}
        <span class="rounded bg-amber-100 px-2 py-1 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-200" data-net-policy-drift>
            {#if sidecar}
                The workspace file differs from what the sidecar runs; save to apply it. <!-- TODO(i18n) -->
            {:else}
                {m.netpolicy_drift()}
            {/if}
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
            {#if policy.egress === "allowlist" && sidecar}
                Only hosts allowed below are reachable. <!-- TODO(i18n) -->
            {:else if policy.egress === "allowlist"}
                {m.netpolicy_mode_allowlist_hint()}
            {:else if sidecar}
                Every public host is reachable; the traffic panel marks requests an allowlist would block. <!-- TODO(i18n) -->
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
                        {#if !sidecar}
                            <span class="text-muted-foreground font-mono">{portsLabel(rule)}</span>
                        {/if}
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
                placeholder={sidecar ? SIDECAR_TARGET_PLACEHOLDER : m.netpolicy_target_placeholder()}
                bind:value={newTarget} disabled={busy} data-net-policy-target />
            {#if !sidecar}
                <input class="w-20 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                    placeholder={m.netpolicy_ports_placeholder()} bind:value={newPorts} disabled={busy} />
            {/if}
            <Button size="sm" disabled={busy} onclick={addRule} data-net-policy-add-rule>
                {m.rule_add()}
            </Button>
        </div>
        {#if sidecar}
            <span class="text-muted-foreground text-xs" data-net-policy-sidecar-hint>
                Allow rules take hostnames and deny rules take CIDR ranges. An allowed host is reachable on every port, and *.example.com also allows example.com. <!-- TODO(i18n) -->
            </span>
        {/if}

        <span class="text-muted-foreground mt-2 border-t pt-2 text-xs shrink-0" data-secrets-heading>
            {m.dashboard_secrets_title()}
        </span>
        <span class="text-muted-foreground text-xs">
            {m.netpolicy_secrets_hint()}
        </span>
        {#each policy.secrets as binding (binding.envVar)}
            {#if editingVar === binding.envVar}
                {@render secretForm()}
            {:else}
                <div class="flex items-center gap-2 rounded bg-muted px-2 py-1 text-xs" data-net-policy-secret={binding.envVar}>
                    <span class="flex-1 truncate font-mono">${binding.envVar} → {binding.hosts.join(", ")}</span>
                    <Button variant="ghost" size="icon" disabled={busy} aria-label={m.netpolicy_secret_edit()}
                        onclick={() => startEdit(binding)} data-secret-edit><PencilIcon class="size-3.5" /></Button>
                    <Button variant="ghost" size="icon" disabled={busy} aria-label={m.netpolicy_secret_remove()}
                        onclick={() => removeSecret(binding)} data-secret-remove>✕</Button>
                </div>
            {/if}
            {#if unstampedByVar.has(binding.envVar)}
                <span class="text-xs text-amber-700 dark:text-amber-300" data-net-policy-unstamped={binding.envVar}>
                    Added after this sandbox was created, so ${binding.envVar} is unset inside it. Export it by hand: <!-- TODO(i18n) -->
                    <code class="font-mono">export {binding.envVar}={unstampedByVar.get(binding.envVar)}</code>
                </span>
            {/if}
        {/each}
        {#if editingVar === null}
            {@render secretForm()}
        {/if}
        {#if echoWarning}
            <span class="text-xs text-amber-700 dark:text-amber-300" data-net-policy-echo-warning>
                {m.netpolicy_echo_warning({ hosts: echoWarning })}
            </span>
        {/if}

        {#if !sidecar}
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
        {/if}

        <div class="flex items-center gap-2">
            <!-- Sidecar drift is applied by saving the file unchanged, so it needs no edit first. -->
            <Button size="sm" disabled={busy || !(dirty || (sidecar && drift))} onclick={save} data-net-policy-save>
                {m.netpolicy_save()}
            </Button>
            <span class="text-muted-foreground text-xs">
                {#if sidecar}
                    Saving applies the policy to the running sidecar at once. <!-- TODO(i18n) -->
                {:else}
                    {m.netpolicy_save_hint()}
                {/if}
            </span>
        </div>

        {#if report}
            <span class="text-muted-foreground text-xs" data-net-policy-apply-report>
                {#if report.missingSecrets.length > 0}
                    Applied. No value is stored for {report.missingSecrets.map((v) => "$" + v).join(", ")}, so those placeholders pass through unswapped. <!-- TODO(i18n) -->
                {:else}
                    Applied. <!-- TODO(i18n) -->
                {/if}
            </span>
        {/if}

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

        {#if !sidecar}
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
    {/if}

    {#if editorError}
        <span class="text-xs text-red-500" data-net-policy-error>{editorError}</span>
    {/if}
</div>
