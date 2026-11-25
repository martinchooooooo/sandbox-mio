<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import Button from "$lib/components/ui/button/button.svelte";
    import { parseError } from "$lib/errors";
    import { Switch } from "$lib/components/ui/switch";
    import PencilIcon from "@lucide/svelte/icons/pencil";
    import { m } from "$lib/paraglide/messages.js";
    import NetPolicyProposalDialog from "./NetPolicyProposalDialog.svelte";
    import type { ApplyReport, NetPolicyState, NetRule, NetPolicy, Proposal, SecretBinding, UnstampedBinding } from "./net-policy";

    let { sandboxName }: { sandboxName: string } = $props();

    let report = $state<ApplyReport | null>(null);
    let unstamped = $state<UnstampedBinding[]>([]);
    const unstampedByVar = $derived(new Map(unstamped.map((u) => [u.envVar, u.placeholder])));

    // Hosts known to echo request headers back, which would reflect a substituted secret into the
    // guest (docs/features/microsandbox-network-policy/02_dev.md, Secret handling invariants).
    const ECHO_HOSTS = ["httpbin.org", "postman-echo.com"];

    // The iron-proxy dialect: host allows and CIDR denies only, applied without a recreate
    // (docs/features/podman-egress-boundary/02_dev.md, Resolved Decision 6).
    const TARGET_PLACEHOLDER = "host, *.suffix, or CIDR to deny"; // TODO(i18n)

    let policy = $state<NetPolicy | null>(null);
    let baseRevision = $state(0);
    let proposal = $state<Proposal | null>(null);
    let proposalError = $state<string | null>(null);
    let reviewOpen = $state(false);
    let dirty = $state(false);
    let busy = $state(false);
    // Editing waits for the user to apply or discard a pending proposal
    // (docs/features/net-policy-review/02_dev.md, Resolved Decision 3).
    const locked = $derived(busy || proposal !== null || proposalError !== null);
    let editorError = $state<string | null>(null);

    let newAction = $state<"allow" | "deny">("allow");
    let newTarget = $state("");

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

    let lastRefresh = 0;

    async function refresh() {
        lastRefresh = Date.now();
        try {
            const state = await invoke<NetPolicyState>("net_policy_state", { name: sandboxName });
            policy = state.accepted;
            baseRevision = state.revision;
            unstamped = state.unstamped;
            proposal = state.proposal;
            proposalError = state.proposalError;
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
            report = await invoke<ApplyReport>("apply_sandbox_policy", {
                name: sandboxName,
                policy: $state.snapshot(policy),
                baseRevision,
            });
            await refresh();
        } catch (e) {
            editorError = parseError(e).message;
        } finally {
            busy = false;
        }
    }

    async function discardProposal() {
        busy = true;
        editorError = null;
        try {
            await invoke("discard_net_policy_proposal", { name: sandboxName });
            await refresh();
        } catch (e) {
            editorError = parseError(e).message;
        } finally {
            busy = false;
        }
    }

    async function proposalResolved(applied: ApplyReport | null) {
        report = applied;
        await refresh();
    }

    function targetLabel(rule: NetRule): string {
        return rule.host ?? rule.cidr ?? (rule.group ? `@${rule.group}` : "?");
    }

    function addRule() {
        const target = newTarget.trim();
        if (!target) return;
        const rule: NetRule = { action: newAction };
        if (["public", "private", "host"].includes(target)) rule.group = target as NetRule["group"];
        else if (/^[0-9a-fA-F:.]+(\/\d+)?$/.test(target) && /[\d:]/.test(target[0])) rule.cidr = target;
        else rule.host = target;
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
        return { action: "allow", host };
    }

    /**
     * Applies a secret change at once, with any other unsaved edits, in one backend call: the value
     * is stored only when the new policy applies, so a rejected binding never replaces or deletes a
     * stored value.
     */
    async function commitSecretChange(
        edit: (p: NetPolicy) => void,
        { store, removedId }: { store?: { secretId: string; value: string }; removedId?: string } = {},
    ): Promise<boolean> {
        if (!policy) return false;
        busy = true;
        editorError = null;
        try {
            mutate(edit);
            // The sidecar reads values only at an apply
            // (docs/features/podman-egress-boundary/02_dev.md, Resolved Decision 15).
            report = await invoke<ApplyReport>("apply_sandbox_policy", {
                name: sandboxName,
                policy: $state.snapshot(policy),
                baseRevision,
                storeSecret: store,
                dropSecretId: removedId,
            });
            await refresh();
            return true;
        } catch (e) {
            // The apply may have taken effect before the error (a failed cleanup), so reload rather
            // than restore the pre-edit copy; a stale revision needs the reload too.
            await refresh();
            editorError = parseError(e).message;
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
        // Values are stored per sandbox, so this id resolves only within this sandbox.
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

    // Nothing watches the workspace file, so a window focus is when a new proposal shows up.
    const FOCUS_REFRESH_MIN_MS = 5000;
    function refreshOnFocus() {
        if (busy || dirty || reviewOpen || Date.now() - lastRefresh < FOCUS_REFRESH_MIN_MS) return;
        refresh();
    }

    $effect(() => {
        void sandboxName;
        refresh();
    });
</script>

<svelte:window onfocus={refreshOnFocus} />

{#snippet secretForm()}
    {@const editing = editingVar !== null}
    <div class="flex flex-col gap-1.5 rounded border px-2 py-1.5" data-secret-form={editing ? "edit" : "add"}>
        <div class="flex flex-wrap items-center gap-2">
            <input class="w-36 rounded border bg-background px-2 py-1 font-mono text-xs disabled:opacity-50"
                placeholder={m.netpolicy_env_var_placeholder()} bind:value={formEnvVar}
                disabled={locked || editing} data-secret-env-var />
            <input class="min-w-40 flex-1 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.netpolicy_secret_hosts_placeholder()} bind:value={formHosts}
                disabled={locked} data-secret-hosts />
        </div>
        <div class="flex items-center gap-2">
            <input class="flex-1 rounded border bg-background px-2 py-1 font-mono text-xs disabled:opacity-50"
                type={showValue ? "text" : "password"} autocomplete="off" spellcheck="false"
                placeholder={editing ? m.netpolicy_secret_replace_placeholder() : m.netpolicy_secret_value_placeholder()}
                bind:value={formValue} disabled={locked} data-secret-value />
            <label class="text-muted-foreground flex shrink-0 items-center gap-1.5 text-xs">
                <Switch checked={showValue} disabled={locked} onCheckedChange={(v) => (showValue = v)} data-secret-show />
                {m.netpolicy_secret_show()}
            </label>
        </div>
        <div class="flex items-center gap-2">
            <Button size="sm" disabled={locked || !formReady} onclick={saveSecret} data-secret-save>
                {editing ? m.netpolicy_secret_save() : m.netpolicy_secret_add()}
            </Button>
            {#if editing}
                <Button size="sm" variant="ghost" disabled={locked} onclick={resetSecretForm} data-secret-cancel>
                    {m.projects_cancel()}
                </Button>
            {/if}
        </div>
    </div>
{/snippet}

<div class="flex flex-col gap-1.5 border-t pt-2" data-net-policy-editor>
    <span class="text-muted-foreground text-xs shrink-0">{m.netpolicy_title()}</span>

    {#if proposal || proposalError}
        <div class="flex flex-wrap items-center gap-2 rounded bg-amber-100 px-2 py-1 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-200" data-net-policy-proposal>
            <span class="flex-1">
                {#if proposal}
                    The workspace file has network policy changes that are not applied. <!-- TODO(i18n) -->
                {:else}
                    The workspace file cannot be used: {proposalError} <!-- TODO(i18n) -->
                {/if}
            </span>
            {#if proposal}
                <Button size="sm" disabled={busy} onclick={() => (reviewOpen = true)} data-net-policy-review>Review <!-- TODO(i18n) --></Button>
            {/if}
            <Button size="sm" variant="outline" disabled={busy} onclick={discardProposal} data-net-policy-discard>Discard <!-- TODO(i18n) --></Button>
        </div>
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
                    disabled={locked}
                    onclick={() => mutate((p) => (p.egress = value))}
                >{label}</Button>
            {/each}
        </div>

        <span class="text-muted-foreground text-xs">
            {#if policy.egress === "allowlist"}
                Only hosts allowed below are reachable. <!-- TODO(i18n) -->
            {:else}
                Every public host is reachable; the traffic panel marks requests an allowlist would block. <!-- TODO(i18n) -->
            {/if}
        </span>

        {#if policy.rules.length > 0}
            <div class="flex flex-col gap-1" data-net-policy-rules>
                {#each policy.rules as rule, i (i)}
                    <div class="flex items-center gap-2 rounded bg-muted px-2 py-1 text-xs" data-net-policy-rule>
                        <span class={rule.action === "deny" ? "text-destructive" : ""}>{rule.action}</span>
                        <span class="flex-1 truncate font-mono">{targetLabel(rule)}</span>
                        <Button variant="ghost" size="icon" disabled={locked || i === 0} onclick={() => moveRule(i, -1)}>↑</Button>
                        <Button variant="ghost" size="icon" disabled={locked || i === policy.rules.length - 1} onclick={() => moveRule(i, 1)}>↓</Button>
                        <Button variant="ghost" size="icon" disabled={locked}
                            onclick={() => mutate((p) => p.rules.splice(i, 1))}>✕</Button>
                    </div>
                {/each}
            </div>
        {:else}
            <span class="text-muted-foreground text-xs">{m.netpolicy_no_rules()}</span>
        {/if}

        <div class="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="outline" disabled={locked}
                onclick={() => (newAction = newAction === "allow" ? "deny" : "allow")}>{newAction}</Button>
            <input class="w-40 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={TARGET_PLACEHOLDER}
                bind:value={newTarget} disabled={locked} data-net-policy-target />
            <Button size="sm" disabled={locked} onclick={addRule} data-net-policy-add-rule>
                {m.rule_add()}
            </Button>
        </div>
        <span class="text-muted-foreground text-xs" data-net-policy-sidecar-hint>
            Allow rules take hostnames and deny rules take CIDR ranges. An allowed host is reachable on every port, and *.example.com also allows example.com. <!-- TODO(i18n) -->
        </span>

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
                    <Button variant="ghost" size="icon" disabled={locked} aria-label={m.netpolicy_secret_edit()}
                        onclick={() => startEdit(binding)} data-secret-edit><PencilIcon class="size-3.5" /></Button>
                    <Button variant="ghost" size="icon" disabled={locked} aria-label={m.netpolicy_secret_remove()}
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

        <div class="flex items-center gap-2">
            <Button size="sm" disabled={locked || !dirty} onclick={save} data-net-policy-save>
                {m.netpolicy_save()}
            </Button>
            <span class="text-muted-foreground text-xs">
                Saving applies the policy to the running sidecar at once. <!-- TODO(i18n) -->
            </span>
        </div>

        {#if proposal}
            <NetPolicyProposalDialog bind:open={reviewOpen} sandboxName={sandboxName} {baseRevision} {proposal} onresolved={proposalResolved} />
        {/if}

        {#if report}
            <span class="text-muted-foreground text-xs" data-net-policy-apply-report>
                {#if report.missingSecrets.length > 0}
                    Applied. No value is stored for {report.missingSecrets.map((v) => "$" + v).join(", ")}, so those placeholders pass through unswapped. <!-- TODO(i18n) -->
                {:else}
                    Applied. <!-- TODO(i18n) -->
                {/if}
            </span>
        {/if}

    {/if}

    {#if editorError}
        <span class="text-xs text-red-500" data-net-policy-error>{editorError}</span>
    {/if}
</div>
