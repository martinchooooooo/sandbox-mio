<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { invoke, Channel } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
    import { AGENTS, miseKey, onboardingSeed } from "$lib/agents/catalogue";
    import { buildLocalLlmProvider } from "$lib/agents/opencode";
    import { buildPiLocalLlmProvider, PI_CONFIG_ENV, PI_CONFIG_DIR_CONTAINER } from "$lib/agents/pi";
    import type { AgentDescriptor, AgentInstallEvent, AgentAuthStatus } from "$lib/agents/types";
    import type { InferenceStatus } from "$lib/models/types";
    import type { createActionRunner } from "$lib/actions.svelte";
    import { packagesFor, ensurePackages, refreshPackages } from "$lib/workspace/packages.svelte";

    let {
        sandboxName,
        workspacePath,
        action,
    }: {
        sandboxName: string;
        workspacePath: string;
        action: ReturnType<typeof createActionRunner>;
    } = $props();

    const packages = $derived(packagesFor(sandboxName));

    let agentStatuses = $state<Record<string, AgentAuthStatus>>({});
    let credentialBundles = $state<Record<string, boolean>>({});
    let inferenceRunning = $state(false);
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

    async function loadCredentialBundles() {
        try {
            const entries = await invoke<[string, unknown][]>("list_credential_bundles");
            credentialBundles = Object.fromEntries(entries.map(([serviceId]) => [serviceId, true]));
        } catch {
            credentialBundles = {};
        }
    }

    async function loadAgentStatuses() {
        for (const agent of installedAgents) {
            const auth = agent.auth;
            if (!auth) continue;
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
                credentialService: descriptor.credentialService ?? null,
                onboarding: onboardingSeed(descriptor),
                onProgress: channel,
            });
            if (descriptor.provision) {
                installLog = [...installLog, m.projects_agent_provisioning()];
                if (descriptor.provision.kind === "pi-provider") {
                    const { providerId, providerJson } = buildPiLocalLlmProvider();
                    await invoke("provision_pi", {
                        workspacePath,
                        relativePath: descriptor.provision.relativeConfigPath,
                        providerId,
                        providerJson,
                        configEnv: PI_CONFIG_ENV,
                        configDirContainer: PI_CONFIG_DIR_CONTAINER,
                    });
                } else {
                    const { providerId, providerJson, defaultModel } = buildLocalLlmProvider();
                    await invoke("provision_opencode", {
                        workspacePath,
                        relativePath: descriptor.provision.relativeConfigPath,
                        providerId,
                        providerJson,
                        defaultModel,
                    });
                }
                installLog = [...installLog, `✓ ${m.projects_agent_provider_configured()}`];
            }
        });
        installingAgent = null;
        if (success) {
            // Triggers the `installedAgents` effect below, which re-checks auth status once the
            // newly installed agent shows up in the shared packages store.
            await refreshPackages(sandboxName, workspacePath);
        }
    }

    async function openAgentAuth(descriptor: AgentDescriptor) {
        if (!descriptor.auth) return;
        const auth = descriptor.auth;
        await action.run("auth", async () => {
            const label = `agent-auth-${sandboxName}-${descriptor.id}`;
            const existing = await WebviewWindow.getByLabel(label);
            if (existing) {
                await existing.setFocus();
                return;
            }
            const sessionId = await invoke<string>("start_agent_auth", {
                name: sandboxName,
                authCommand: auth.command,
            });
            new WebviewWindow(label, {
                url: `/agent-auth?sandbox=${encodeURIComponent(sandboxName)}&agent=${encodeURIComponent(descriptor.id)}&session=${encodeURIComponent(sessionId)}`,
                title: `${descriptor.label} — authenticate`,
                width: 540,
                height: 460,
            });
        });
    }

    async function useSavedLogin(descriptor: AgentDescriptor) {
        if (!descriptor.auth || !descriptor.credentialService) return;
        const serviceId = descriptor.credentialService;
        await action.run("auth", async () => {
            await invoke("inject_agent_credentials", {
                name: sandboxName,
                serviceId,
                onboarding: onboardingSeed(descriptor),
            });
        });
        await loadAgentStatuses();
    }

    async function loadInferenceStatus() {
        try {
            const status = await invoke<InferenceStatus>("inference_status");
            inferenceRunning = status.running;
        } catch {
            inferenceRunning = false;
        }
    }

    onMount(async () => {
        await loadCredentialBundles();
        await loadInferenceStatus();
    });

    $effect(() => {
        ensurePackages(sandboxName, workspacePath);
    });

    // `installedAgents` derives from the shared `packages` store, so re-check auth status whenever
    // it changes (initial load, install/uninstall, or a sandbox switch) rather than only once at mount.
    $effect(() => {
        installedAgents;
        loadAgentStatuses();
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
            {#if agent.auth}
                {#if status?.authenticated}
                    <span class="text-muted-foreground">{status.accountEmail ?? "authenticated"}</span>
                    <Button size="sm" variant="ghost"
                        onclick={() => openAgentAuth(agent)}
                        disabled={action.busy !== null}>
                        {m.projects_agent_reauth()}
                    </Button>
                {:else if agent.credentialService && credentialBundles[agent.credentialService]}
                    <span class="text-amber-600 dark:text-amber-400">{m.projects_agent_not_authed()}</span>
                    <Button size="sm" variant="ghost"
                        onclick={() => useSavedLogin(agent)}
                        disabled={action.busy !== null}>
                        {m.projects_agent_use_saved_login()}
                    </Button>
                {:else}
                    <span class="text-amber-600 dark:text-amber-400">{m.projects_agent_not_authed()}</span>
                    <Button size="sm" variant="ghost"
                        onclick={() => openAgentAuth(agent)}
                        disabled={action.busy !== null}>
                        {m.projects_agent_auth()}
                    </Button>
                {/if}
            {:else if agent.provision}
                {#if inferenceRunning}
                    <span class="text-muted-foreground">{m.projects_agent_provider_configured()}</span>
                {:else}
                    <a href="/models" class="text-amber-600 dark:text-amber-400 hover:underline">
                        {m.projects_agent_start_model_hint()}
                    </a>
                {/if}
            {/if}
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
