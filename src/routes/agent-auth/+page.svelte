<script lang="ts">
    import { onMount, onDestroy } from "svelte";
    import { cancelAgentAuth, sendAgentAuthInput } from "$lib/api/agents";
    import { seedAgentOnboarding } from "$lib/api/credentials";
    import { getCurrentWindow } from "@tauri-apps/api/window";
    import { AGENTS, onboardingSeed } from "$lib/agents/catalogue";
    import { m } from "$lib/paraglide/messages.js";
    import AuthSessionCard from "$lib/agents/AuthSessionCard.svelte";
    import {
        authSessionId,
        authPhase,
        authExitCode,
        start as startAuthSession,
    } from "$lib/agents/authSession.svelte";

    let sandbox = $state("");
    let agentId = $state("");
    let handledExitId: string | null = null;

    const agent = $derived(AGENTS.find((a) => a.id === agentId));

    onMount(async () => {
        const params = new URLSearchParams(window.location.search);
        const sessionId = params.get("session") ?? "";
        sandbox = params.get("sandbox") ?? "";
        agentId = params.get("agent") ?? "";
        if (!sessionId) return;
        // Attaches the listener given the session id `start_agent_auth` already returned
        // (AgentManager.svelte's openAgentAuth, before this window opened) — mirrors
        // credentials/+page.svelte's logIn().
        await startAuthSession(sessionId);
    });

    // This window's own responsibility on top of the shared card: seed onboarding and close on
    // success. Distinct store instance from credentials/+page.svelte's — each WebviewWindow loads
    // its own JS bundle (docs/features/desktop-app-feel/02_dev.md Phase E).
    $effect(() => {
        const id = authSessionId();
        const ph = authPhase();
        if (ph !== "exited" || !id || id === handledExitId) return;
        handledExitId = id;
        if (authExitCode() === 0) {
            const seed = agent ? onboardingSeed(agent) : null;
            if (seed && sandbox) {
                seedAgentOnboarding(sandbox, seed).catch(() => {});
            }
            setTimeout(() => getCurrentWindow().close(), 2000);
        }
    });

    // A window close without reaching a terminal exit code (native × button mid-login) must still
    // cancel the in-sandbox session — otherwise it hangs until the sandbox itself is stopped.
    onDestroy(() => {
        const sessionId = authSessionId();
        if (sessionId && authPhase() !== "exited") {
            cancelAgentAuth(sessionId).catch(() => {});
        }
    });

    function cancel() {
        const sessionId = authSessionId();
        if (authPhase() === "exited" || !sessionId) {
            getCurrentWindow().close();
        } else {
            cancelAgentAuth(sessionId).catch(() => {});
        }
    }

    function submitCode(code: string) {
        const sessionId = authSessionId();
        if (!sessionId) return;
        sendAgentAuthInput(sessionId, code).catch(() => {});
    }
</script>

<div class="flex flex-col h-full p-3 gap-2">
    <div class="text-xs text-muted-foreground shrink-0">
        {#if agent}
            <!-- TODO(i18n) -->
            {agent.label} — authenticate {sandbox ? `in ${sandbox}` : ""}
        {:else}
            {m.agent_auth_authenticating()}
        {/if}
    </div>
    <AuthSessionCard onCancel={cancel} onSubmitCode={submitCode} />
</div>
