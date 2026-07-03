<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { invoke, Channel } from "@tauri-apps/api/core";
    import { listen } from "@tauri-apps/api/event";
    import { onMount } from "svelte";
    import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
    import { createActionRunner } from "$lib/actions.svelte";
    import { AGENTS } from "$lib/agents/catalogue";
    import { m } from "$lib/paraglide/messages.js";
    import type { AgentInstallEvent, AgentAuthEvent } from "$lib/agents/types";

    interface CredentialManifest {
        capturedAtMs: number;
        accountEmail: string | null;
        expiresAtMs: number | null;
    }

    const action = createActionRunner();

    let bundles = $state<Record<string, CredentialManifest>>({});

    // Distinct services referenced by the catalogue, keeping the first agent descriptor that
    // names each one — it supplies the install/auth shape `logIn` needs to drive the auth container.
    const services = AGENTS.filter((a) => a.credentialService && a.auth).reduce<
        { serviceId: string; label: string }[]
    >((acc, a) => {
        if (!acc.some((s) => s.serviceId === a.credentialService)) {
            acc.push({ serviceId: a.credentialService!, label: a.label });
        }
        return acc;
    }, []);

    async function loadBundles() {
        const entries = await invoke<[string, CredentialManifest][]>("list_credential_bundles");
        bundles = Object.fromEntries(entries);
    }

    async function logIn(serviceId: string) {
        const agent = AGENTS.find((a) => a.credentialService === serviceId && a.auth);
        if (!agent || !agent.auth) return;
        const auth = agent.auth;
        const authContainer = `mio-auth-${serviceId}`;

        await action.run(`login-${serviceId}`, async () => {
            await invoke("ensure_auth_container", { name: authContainer });

            const npmStep = agent.postInstallSteps.find((s) => s.kind === "npm-postinstall");
            const installChannel = new Channel<AgentInstallEvent>();
            await invoke("install_agent", {
                name: authContainer,
                packages: agent.misePackages,
                postinstallNpm: npmStep ? [npmStep.package, npmStep.nodeVersion] : null,
                credentialService: null,
                onProgress: installChannel,
            });

            const sessionId = await invoke<string>("start_agent_auth", {
                name: authContainer,
                authCommand: auth.command,
            });

            const unlisten = await listen<AgentAuthEvent>(`agent-auth-output-${sessionId}`, (event) => {
                if (event.payload.kind !== "Exit") return;
                unlisten();
                (async () => {
                    try {
                        if (event.payload.kind === "Exit" && event.payload.code === 0) {
                            await invoke("capture_agent_credentials", {
                                name: authContainer,
                                serviceId,
                                credentialPath: auth.credentialFile,
                                expiresAtJsonKey: auth.expiresAtJsonKey,
                                accountSourcePath: auth.accountSourceFile ?? null,
                                accountEmailJsonKey: auth.accountEmailJsonKey ?? null,
                            });
                            await loadBundles();
                        }
                    } finally {
                        // Ephemeral auth container: destroyed after every login attempt regardless
                        // of outcome, so a failed capture never leaks a container (02_dev.md's
                        // "Auth container cleanup on failure" cross-cutting concern).
                        await invoke("destroy_sandbox", { name: authContainer, force: true }).catch(() => {});
                    }
                })();
            });

            const label = `agent-auth-credentials-${serviceId}`;
            new WebviewWindow(label, {
                url: `/agent-auth?sandbox=${encodeURIComponent(authContainer)}&agent=${encodeURIComponent(agent.id)}&session=${encodeURIComponent(sessionId)}`,
                title: m.credentials_auth_window_title({ agent: agent.label }),
                width: 540,
                height: 460,
            });
        });
    }

    async function logOut(serviceId: string) {
        await action.run(`logout-${serviceId}`, async () => {
            await invoke("delete_credential_bundle", { serviceId });
            await loadBundles();
        });
    }

    onMount(loadBundles);
</script>

<main class="p-6 pb-24 max-w-2xl mx-auto">
    <Button href="/" variant="outline" class="mb-6">{m.back()}</Button>
    <h1 class="text-3xl mb-2">{m.credentials_title()}</h1>
    <p class="text-sm text-muted-foreground mb-8">{m.credentials_intro()}</p>

    <div class="flex flex-col gap-3">
        {#each services as service}
            {@const bundle = bundles[service.serviceId]}
            <div
                class="flex items-center justify-between gap-2 rounded-lg border p-3"
                data-service={service.serviceId}
            >
                <div>
                    <div class="text-sm font-medium">{service.label}</div>
                    <div class="text-xs text-muted-foreground" data-status>
                        {bundle ? (bundle.accountEmail ?? m.credentials_status_connected()) : m.credentials_status_not_connected()}
                    </div>
                </div>
                {#if bundle}
                    <Button
                        size="sm"
                        variant="outline"
                        onclick={() => logOut(service.serviceId)}
                        disabled={action.busy !== null}
                    >
                        {m.credentials_log_out()}
                    </Button>
                {:else}
                    <Button size="sm" onclick={() => logIn(service.serviceId)} disabled={action.busy !== null}>
                        {m.credentials_log_in()}
                    </Button>
                {/if}
            </div>
        {/each}
    </div>

    {#if action.error}<p class="text-destructive mt-3 text-sm">{action.error}</p>{/if}
</main>
