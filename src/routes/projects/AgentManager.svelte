<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { invoke, Channel } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
    import { AGENTS, miseKey } from "$lib/agents/catalogue";
    import type { AgentDescriptor, AgentInstallEvent, AgentAuthStatus } from "$lib/agents/types";
    import type { createActionRunner } from "$lib/actions.svelte";

    let {
        sandboxName,
        workspacePath,
        action,
    }: {
        sandboxName: string;
        workspacePath: string;
        action: ReturnType<typeof createActionRunner>;
    } = $props();

    // Package list read from the workspace manifest, used solely to derive which agents are
    // installed. PackageManager keeps its own copy for the packages UI; this one is private.
    let packages = $state<string[]>([]);

    let agentStatuses = $state<Record<string, AgentAuthStatus>>({});
    let installingAgent = $state<string | null>(null);
    let installLog = $state<string[]>([]);
    let showAgentPicker = $state(false);

    const installedAgents = $derived(
        AGENTS.filter((a) => a.misePackages.every((p) => packages.includes(miseKey(p))))
    );
    const availableAgents = $derived(AGENTS.filter((a) => !installedAgents.includes(a)));

    function formatInstallEvent(e: AgentInstallEvent): string {
        switch (e.kind) {
            case "PackageStart": return `Installing ${e.package}…`;
            case "PackageDone": return `✓ ${e.package}`;
            case "PostInstallStart": return `Running ${e.step}…`;
            case "PostInstallDone": return `✓ ${e.step}`;
            case "Done": return "Done.";
        }
    }

    async function loadPackages() {
        packages = await invoke<string[]>("list_packages", { workspacePath });
    }

    async function loadAgentStatuses() {
        for (const agent of installedAgents) {
            const auth = agent.auth;
            try {
                const result = await invoke<{ authenticated: boolean; expiresAtMs: number | null; accountEmail: string | null }>(
                    "get_agent_auth_status",
                    {
                        name: sandboxName,
                        credentialPath: auth.credentialFile,
                        expiresAtJsonKey: auth.expiresAtJsonKey,
                        accountSourcePath: auth.accountSourceFile ?? null,
                        accountEmailJsonKey: auth.accountEmailJsonKey ?? null,
                    }
                );
                const expiresAt = result.expiresAtMs;
                const authenticated = result.authenticated && (expiresAt === null || expiresAt > Date.now());
                agentStatuses[agent.id] = { authenticated, expiresAtMs: expiresAt, accountEmail: result.accountEmail };
            } catch {
                agentStatuses[agent.id] = { authenticated: false, expiresAtMs: null, accountEmail: null };
            }
        }
    }

    async function installAgent(descriptor: AgentDescriptor) {
        installingAgent = descriptor.id;
        installLog = [];
        const npmStep = descriptor.postInstallSteps.find((s) => s.kind === "npm-postinstall");
        const success = await action.run("install-agent", async () => {
            const channel = new Channel<AgentInstallEvent>();
            channel.onmessage = (event) => {
                installLog = [...installLog, formatInstallEvent(event)];
            };
            await invoke("install_agent", {
                name: sandboxName,
                packages: descriptor.misePackages,
                postinstallNpm: npmStep ? [npmStep.package, npmStep.nodeVersion] : null,
                onProgress: channel,
            });
        });
        installingAgent = null;
        if (success) {
            await loadPackages();
            await loadAgentStatuses();
        }
    }

    async function openAgentAuth(descriptor: AgentDescriptor) {
        await action.run("auth", async () => {
            const label = `agent-auth-${sandboxName}-${descriptor.id}`;
            const existing = await WebviewWindow.getByLabel(label);
            if (existing) {
                await existing.setFocus();
                return;
            }
            const sessionId = await invoke<string>("start_agent_auth", {
                name: sandboxName,
                authCommand: descriptor.auth.command,
            });
            new WebviewWindow(label, {
                url: `/agent-auth?sandbox=${encodeURIComponent(sandboxName)}&agent=${encodeURIComponent(descriptor.id)}&session=${encodeURIComponent(sessionId)}`,
                title: `${descriptor.label} — authenticate`,
                width: 540,
                height: 460,
            });
        });
    }

    onMount(async () => {
        await loadPackages();
        await loadAgentStatuses();
    });
</script>

<div class="flex flex-col gap-1.5 border-t pt-2">
    <div class="flex items-center justify-between">
        <span class="text-muted-foreground text-xs shrink-0">{m.projects_agents()}</span>
        {#if availableAgents.length > 0}
            <Button size="sm" variant="ghost"
                onclick={() => (showAgentPicker = true)}
                disabled={installingAgent !== null || action.busy !== null}>
                {m.projects_add_agent()}
            </Button>
        {/if}
    </div>

    {#if installedAgents.length === 0 && !showAgentPicker}
        <span class="text-muted-foreground text-xs">{m.projects_no_agents()}</span>
    {/if}

    {#each installedAgents as agent}
        {@const status = agentStatuses[agent.id]}
        <div class="flex items-center justify-between gap-2 text-xs">
            <span class="font-medium">{agent.label}</span>
            {#if status?.authenticated}
                <span class="text-muted-foreground">{status.accountEmail ?? "authenticated"}</span>
            {:else}
                <span class="text-amber-600 dark:text-amber-400">{m.projects_agent_not_authed()}</span>
            {/if}
            <Button size="sm" variant="ghost"
                onclick={() => openAgentAuth(agent)}
                disabled={action.busy !== null}>
                {status?.authenticated ? m.projects_agent_reauth() : m.projects_agent_auth()}
            </Button>
        </div>
    {/each}

    {#if installingAgent !== null}
        <div class="text-xs text-muted-foreground space-y-0.5">
            {#each installLog as line}
                <div>{line}</div>
            {/each}
        </div>
    {/if}

    {#if showAgentPicker}
        <div class="flex flex-col gap-1.5">
            {#each availableAgents as agent}
                <div class="flex items-center justify-between gap-2">
                    <div>
                        <span class="text-xs font-medium">{agent.label}</span>
                        <span class="text-xs text-muted-foreground ml-1">{agent.description}</span>
                    </div>
                    <Button size="sm"
                        onclick={() => { showAgentPicker = false; installAgent(agent); }}
                        disabled={installingAgent !== null}>
                        {m.projects_install()}
                    </Button>
                </div>
            {/each}
            <Button size="sm" variant="outline" onclick={() => (showAgentPicker = false)}>
                {m.projects_cancel()}
            </Button>
        </div>
    {/if}
</div>
