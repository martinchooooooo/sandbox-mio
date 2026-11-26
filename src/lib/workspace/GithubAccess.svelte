<script lang="ts">
    import { githubSandboxStatus, setGithubSandboxAccess, type GithubSandboxStatus } from "$lib/api/credentials";
    import { listen } from "@tauri-apps/api/event";
    import { untrack } from "svelte";
    import { Switch } from "$lib/components/ui/switch";
    import { parseError } from "$lib/errors";
    import { authCapturedCount } from "$lib/agents/authSession.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { showPage } from "$lib/workspace/navigation.svelte";

    let { sandboxName }: { sandboxName: string } = $props();

    let status = $state<GithubSandboxStatus | null>(null);
    let busy = $state(false);
    // Remounts the switch after every call so its thumb always shows `status`, even when the call
    // failed or changed nothing and `status` kept the same value.
    let switchKey = $state(0);
    let error = $state<string | null>(null);

    type Display = "deployed" | "no-credential" | "opted-out" | "needs-image" | "failed" | "applied" | "pending";

    const display = $derived.by<Display | null>(() => {
        const s = status;
        if (!s) return null;
        if (s.deployed) return "deployed";
        if (s.credentialCapturedAtMs === null) return "no-credential";
        if (s.optedOut) return "opted-out";
        if (s.outcome?.kind === "needsImage") return "needs-image";
        if (s.outcome?.kind === "failed") return "failed";
        // The placeholder never changes, so a rotation does not make gh stale.
        if (s.outcome?.kind === "applied") return "applied";
        return "pending";
    });

    const showSwitch = $derived(display !== null && display !== "deployed" && display !== "no-credential");

    async function refresh() {
        try {
            status = await githubSandboxStatus(sandboxName);
            error = null;
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function setAccess(enabled: boolean) {
        busy = true;
        try {
            status = await setGithubSandboxAccess(sandboxName, enabled);
            error = null;
        } catch (e) {
            error = parseError(e).message;
            await refresh();
        } finally {
            busy = false;
            switchKey += 1;
        }
    }

    $effect(() => {
        void sandboxName;
        void authCapturedCount();
        untrack(() => void refresh());
    });

    // Background reconciles (start, save, delete) change the status without any call from here.
    // Event name and payload mirror `CHANGED_EVENT` in src-tauri/src/services/github_access.rs.
    $effect(() => {
        const mine = sandboxName;
        let stopped = false;
        let unlisten: (() => void) | undefined;
        void listen<{ name: string }>("github-access://changed", (event) => {
            if (event.payload.name === mine) {
                void refresh().then(() => (switchKey += 1));
            }
        }).then((fn) => {
            if (stopped) fn();
            else unlisten = fn;
        });
        return () => {
            stopped = true;
            unlisten?.();
        };
    });
</script>

{#if status}
    <div
        class="flex items-center justify-between gap-2 rounded-lg border p-3 text-xs"
        data-github-access
        data-state={display}
    >
        <div class="min-w-0">
            <div class="text-sm font-medium">{m.github_card_title()}</div>
            <div class="text-muted-foreground" data-github-access-text>
                {#if display === "deployed"}
                    {m.github_deployed()}
                {:else if display === "no-credential"}
                    {m.github_no_credential()}
                    <button type="button" class="underline" onclick={() => showPage("/credentials")}>
                        {m.github_login_link()}
                    </button>
                {:else if display === "opted-out"}
                    {m.github_opted_out()}
                {:else if display === "needs-image"}
                    {m.github_needs_image()}
                {:else if display === "failed" && status.outcome?.kind === "failed"}
                    {m.github_failed({ reason: status.outcome.reason })}
                {:else if display === "applied"}
                    {`${status.accountLogin ? m.github_account({ login: status.accountLogin }) : m.github_card_title()} · ${m.github_token_sidecar()}`}
                {:else}
                    {m.github_pending()}
                {/if}
            </div>
            {#if error}<div class="text-destructive">{error}</div>{/if}
        </div>
        {#if showSwitch}
            {#key switchKey}
                <Switch
                    checked={!status.optedOut}
                    onCheckedChange={setAccess}
                    disabled={busy}
                    aria-label={m.github_access_switch_label()}
                    data-github-access-switch
                />
            {/key}
        {/if}
    </div>
{/if}
