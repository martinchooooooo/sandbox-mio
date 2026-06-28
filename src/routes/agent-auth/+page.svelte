<script lang="ts">
    import { onMount, onDestroy } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { listen, type UnlistenFn } from "@tauri-apps/api/event";
    import { openUrl } from "@tauri-apps/plugin-opener";
    import { getCurrentWindow } from "@tauri-apps/api/window";
    import Button from "$lib/components/ui/button/button.svelte";
    import { AGENTS } from "$lib/agents/catalogue";
    import type { AgentAuthEvent } from "$lib/agents/types";

    let sessionId = $state("");
    let sandbox = $state("");
    let agentId = $state("");
    let outputLines = $state<string[]>([]);
    let authUrl = $state<string | null>(null);
    let showCodeInput = $state(false);
    let codeInput = $state("");
    let exitCode = $state<number | null>(null);
    let submitting = $state(false);

    let unlisten: UnlistenFn | null = null;
    let outputEl: HTMLPreElement | undefined;

    const agent = $derived(AGENTS.find((a) => a.id === agentId));

    function scrollToBottom() {
        if (outputEl) outputEl.scrollTop = outputEl.scrollHeight;
    }

    async function submitCode() {
        if (!codeInput.trim() || submitting) return;
        submitting = true;
        try {
            await invoke("send_agent_auth_input", {
                sessionId,
                input: codeInput.trim(),
            });
            codeInput = "";
            showCodeInput = false;
        } catch {
            submitting = false;
        }
    }

    onMount(async () => {
        const params = new URLSearchParams(window.location.search);
        sessionId = params.get("session") ?? "";
        sandbox = params.get("sandbox") ?? "";
        agentId = params.get("agent") ?? "";

        if (!sessionId) return;

        unlisten = await listen<AgentAuthEvent>(`agent-auth-output-${sessionId}`, (event) => {
            const data = event.payload;
            if (data.kind === "Output") {
                outputLines = [...outputLines, data.line];
                // Detect OAuth URL
                const match = data.line.match(/https:\/\/[a-z0-9.-]+\.[a-z]{2,}\/[^\s]+/i);
                if (match && !authUrl) authUrl = match[0];
                // Detect code paste prompt
                if (data.line.toLowerCase().includes("paste code")) showCodeInput = true;
                setTimeout(scrollToBottom, 10);
            } else if (data.kind === "Exit") {
                exitCode = data.code;
                if (data.code === 0) {
                    setTimeout(() => getCurrentWindow().close(), 2000);
                }
            }
        });
    });

    onDestroy(async () => {
        unlisten?.();
        if (sessionId && exitCode === null) {
            invoke("cancel_agent_auth", { sessionId }).catch(() => {});
        }
    });
</script>

<div class="flex flex-col h-screen bg-zinc-950 text-zinc-100 text-sm font-mono p-3 gap-3">
    <div class="text-xs text-zinc-400 shrink-0">
        {#if agent}
            {agent.label} — authenticate {sandbox ? `in ${sandbox}` : ""}
        {:else}
            Authenticating…
        {/if}
    </div>

    <pre
        bind:this={outputEl}
        class="flex-1 overflow-auto whitespace-pre-wrap break-all text-xs leading-relaxed text-zinc-200 bg-zinc-900 rounded p-2"
    >{#each outputLines as line}{line + "\n"}{/each}</pre>

    {#if authUrl && exitCode === null}
        <div class="shrink-0 flex items-center gap-2">
            <span class="text-xs text-zinc-400 truncate flex-1">{authUrl}</span>
            <Button size="sm" onclick={() => openUrl(authUrl!)}>Open in browser</Button>
        </div>
    {/if}

    {#if showCodeInput && exitCode === null}
        <div class="shrink-0 flex items-center gap-2">
            <input
                class="flex-1 rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-xs text-zinc-100 placeholder:text-zinc-600"
                placeholder="Paste authorization code here…"
                bind:value={codeInput}
                onkeydown={(e) => e.key === "Enter" && submitCode()}
                disabled={submitting}
            />
            <Button size="sm" onclick={submitCode} disabled={submitting || !codeInput.trim()}>
                Submit
            </Button>
        </div>
    {/if}

    {#if exitCode !== null}
        <div class="shrink-0">
            {#if exitCode === 0}
                <p class="text-xs text-green-400">Authentication successful. Closing…</p>
            {:else}
                <div class="flex items-center gap-2">
                    <p class="text-xs text-red-400 flex-1">Authentication failed (exit code {exitCode}).</p>
                    <Button size="sm" variant="outline" onclick={() => getCurrentWindow().close()}>
                        Close
                    </Button>
                </div>
            {/if}
        </div>
    {/if}
</div>
