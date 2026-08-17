<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Input } from "$lib/components/ui/input";
    import { m } from "$lib/paraglide/messages.js";
    import {
        authPhase,
        authOutputLines,
        authUrlDetected,
        authExitCode,
    } from "$lib/agents/authSession.svelte";
    import { openUrl } from "@tauri-apps/plugin-opener";

    // Presentational: reads the in-flight session directly from the shared store (the single
    // source of truth for whichever window/route is driving it) rather than taking it as props.
    // credentials/+page.svelte and agent-auth/+page.svelte each supply their own cancel/dismiss and
    // code-submission behavior, since only the caller knows what "done" means for it — closing a
    // window vs. collapsing a card (docs/features/desktop-app-feel/02_dev.md Phase E).
    let {
        onCancel,
        onSubmitCode,
    }: {
        onCancel: () => void;
        onSubmitCode: (code: string) => void;
    } = $props();

    let showDetails = $state(false);
    let codeInput = $state("");
    let submittingCode = $state(false);

    async function submitCode() {
        const code = codeInput.trim();
        if (!code || submittingCode) return;
        submittingCode = true;
        try {
            onSubmitCode(code);
            codeInput = "";
        } finally {
            submittingCode = false;
        }
    }
</script>

<div class="flex flex-col gap-2">
    {#if authPhase() === "exited"}
        {#if authExitCode() === 0}
            <p class="text-xs text-muted-foreground">{m.auth_login_successful()}</p>
        {:else}
            <p class="text-xs text-destructive">
                {m.auth_failed({ code: authExitCode() ?? 0 })}
            </p>
        {/if}
    {:else if !authUrlDetected()}
        <p class="text-xs text-muted-foreground">{m.auth_signing_in()}</p>
    {/if}

    {#if authUrlDetected() && authPhase() !== "exited"}
        <div class="flex items-center gap-2">
            <span class="text-xs text-muted-foreground truncate flex-1">
                {authUrlDetected()}
            </span>
            <Button size="sm" onclick={() => openUrl(authUrlDetected()!)}>
                {m.auth_open_in_browser()}
            </Button>
        </div>
    {/if}

    {#if authPhase() === "awaiting-code"}
        <div class="flex items-center gap-2">
            <Input
                class="flex-1"
                placeholder={m.auth_code_placeholder()}
                bind:value={codeInput}
                onkeydown={(e) => e.key === "Enter" && submitCode()}
                disabled={submittingCode}
            />
            <Button size="sm" onclick={submitCode} disabled={submittingCode || !codeInput.trim()}>
                {m.auth_submit()}
            </Button>
        </div>
    {/if}

    <div class="flex items-center gap-2">
        <Button size="sm" variant="outline" onclick={onCancel}>
            {m.auth_cancel()}
        </Button>
        <Button size="sm" variant="ghost" onclick={() => (showDetails = !showDetails)}>
            {showDetails ? m.auth_hide_details() : m.auth_show_details()}
        </Button>
    </div>

    {#if showDetails}
        <pre
            class="max-h-40 overflow-auto whitespace-pre-wrap break-all text-xs leading-relaxed bg-muted rounded p-2"
        >{#each authOutputLines() as line}{line + "\n"}{/each}</pre>
    {/if}
</div>
