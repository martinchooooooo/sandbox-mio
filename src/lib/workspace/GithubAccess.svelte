<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { listen } from "@tauri-apps/api/event";
    import { untrack } from "svelte";
    import { Switch } from "$lib/components/ui/switch";
    import { parseError } from "$lib/errors";
    import { authCapturedCount } from "$lib/agents/authSession.svelte";
    import { showPage } from "$lib/workspace/navigation.svelte";

    // Mirrors `ApplyOutcome` and `GithubSandboxStatus` in src-tauri/src/credentials/github.rs and
    // src-tauri/src/services/github_access.rs.
    type Outcome =
        | { kind: "applied"; capturedAtMs: number }
        | { kind: "needsImage" }
        | { kind: "failed"; reason: string; capturedAtMs: number };
    interface GithubStatus {
        optedOut: boolean;
        outcome: Outcome | null;
        credentialCapturedAtMs: number | null;
        accountLogin: string | null;
        deployed: boolean;
    }

    let { sandboxName }: { sandboxName: string } = $props();

    const switchLabel = "GitHub access for this sandbox"; // TODO(i18n)

    let status = $state<GithubStatus | null>(null);
    let busy = $state(false);
    // Remounts the switch after every call so its thumb always shows `status`, even when the call
    // failed or changed nothing and `status` kept the same value.
    let switchKey = $state(0);
    let error = $state<string | null>(null);

    type Display = "deployed" | "no-credential" | "opted-out" | "needs-image" | "failed" | "applied" | "out-of-date" | "pending";

    const display = $derived.by<Display | null>(() => {
        const s = status;
        if (!s) return null;
        if (s.deployed) return "deployed";
        if (s.credentialCapturedAtMs === null) return "no-credential";
        if (s.optedOut) return "opted-out";
        if (s.outcome?.kind === "needsImage") return "needs-image";
        if (s.outcome?.kind === "failed") return "failed";
        if (s.outcome?.kind === "applied") {
            return s.outcome.capturedAtMs === s.credentialCapturedAtMs ? "applied" : "out-of-date";
        }
        return "pending";
    });

    const showSwitch = $derived(display !== null && display !== "deployed" && display !== "no-credential");

    async function refresh() {
        try {
            status = await invoke<GithubStatus>("github_sandbox_status", { name: sandboxName });
            error = null;
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function setAccess(enabled: boolean) {
        busy = true;
        try {
            status = await invoke<GithubStatus>("set_github_sandbox_access", { name: sandboxName, enabled });
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
            <div class="text-sm font-medium">GitHub</div><!-- TODO(i18n) -->
            <div class="text-muted-foreground" data-github-access-text>
                {#if display === "deployed"}
                    Not available for deployed sandboxes<!-- TODO(i18n) -->
                {:else if display === "no-credential"}
                    No GitHub login.<!-- TODO(i18n) -->
                    <button type="button" class="underline" onclick={() => showPage("/credentials")}>
                        Log in on the Credentials page<!-- TODO(i18n) -->
                    </button>
                {:else if display === "opted-out"}
                    Off for this sandbox<!-- TODO(i18n) -->
                {:else if display === "needs-image"}
                    Needs the sandbox recreated over its workspace for gh<!-- TODO(i18n) -->
                {:else if display === "failed" && status.outcome?.kind === "failed"}
                    {`Failed: ${status.outcome.reason}`}<!-- TODO(i18n) -->
                {:else if display === "applied"}
                    {status.accountLogin ? `GitHub: ${status.accountLogin}` : "GitHub token applied"}<!-- TODO(i18n) -->
                {:else if display === "out-of-date"}
                    Out of date; applies on next start<!-- TODO(i18n) -->
                {:else}
                    Applies when the sandbox runs<!-- TODO(i18n) -->
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
                    aria-label={switchLabel}
                    data-github-access-switch
                />
            {/key}
        {/if}
    </div>
{/if}
