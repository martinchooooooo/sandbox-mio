<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import Input from "$lib/components/ui/input/input.svelte";
    import * as Collapsible from "$lib/components/ui/collapsible";
    import { invoke, Channel } from "@tauri-apps/api/core";
    import { openUrl } from "@tauri-apps/plugin-opener";
    import { onMount } from "svelte";
    import { toast } from "svelte-sonner";
    import { createActionRunner } from "$lib/actions.svelte";
    import { AGENTS, onboardingSeed } from "$lib/agents/catalogue";
    import { m } from "$lib/paraglide/messages.js";
    import { parseError } from "$lib/errors";
    import type { AgentInstallEvent } from "$lib/agents/types";
    import {
        authSessionId,
        authPhase,
        authOutputLines,
        authUrlDetected,
        authExitCode,
        beginSigningIn,
        start as startAuthSession,
        reset as resetAuthSession,
    } from "$lib/agents/authSession.svelte";

    interface CredentialManifest {
        capturedAtMs: number;
        accountEmail: string | null;
        expiresAtMs: number | null;
    }

    const action = createActionRunner();

    let bundles = $state<Record<string, CredentialManifest>>({});
    let activeServiceId = $state<string | null>(null);
    let cardOpen = $state<Record<string, boolean>>({});
    let showDetails = $state(false);
    let codeInput = $state("");
    let submittingCode = $state(false);
    let handledExitId: string | null = null;

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

    function dismiss(serviceId: string) {
        resetAuthSession();
        activeServiceId = null;
        showDetails = false;
        codeInput = "";
        delete cardOpen[serviceId];
    }

    async function handleExit(sessionId: string) {
        const serviceId = activeServiceId;
        if (!serviceId) return;
        const agent = AGENTS.find((a) => a.credentialService === serviceId && a.auth);
        if (!agent || !agent.auth) return;
        const auth = agent.auth;
        const authContainer = `mio-auth-${serviceId}`;
        const code = authExitCode();

        if (code === 0) {
            try {
                const seed = onboardingSeed(agent);
                if (seed) {
                    await invoke("seed_agent_onboarding", { name: authContainer, onboarding: seed }).catch(
                        () => {},
                    );
                }
                await invoke("capture_agent_credentials", {
                    name: authContainer,
                    serviceId,
                    credentialPath: auth.credentialFile,
                    expiresAtJsonKey: auth.expiresAtJsonKey,
                    accountSourcePath: auth.accountSourceFile ?? null,
                    accountEmailJsonKey: auth.accountEmailJsonKey ?? null,
                });
                await loadBundles();
            } catch (e) {
                // A failed capture must still let the card collapse below — otherwise a CLI login
                // that printed success leaves the row stuck open with nothing rendered, which is
                // indistinguishable from a hang.
                toast.error(parseError(e).message);
            }
        }

        // Ephemeral auth container: destroyed after every login attempt regardless
        // of outcome, so a failed capture never leaks a container (02_dev.md's
        // "Auth container cleanup on failure" cross-cutting concern).
        await invoke("destroy_sandbox", { name: authContainer, force: true }).catch(() => {});

        if (code === 0) dismiss(serviceId);
    }

    async function logIn(serviceId: string) {
        const agent = AGENTS.find((a) => a.credentialService === serviceId && a.auth);
        if (!agent || !agent.auth) return;
        const auth = agent.auth;
        const authContainer = `mio-auth-${serviceId}`;

        activeServiceId = serviceId;
        showDetails = false;
        codeInput = "";
        beginSigningIn();
        cardOpen[serviceId] = true;

        const ok = await action.run(`login-${serviceId}`, async () => {
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

            if (activeServiceId !== serviceId) {
                // Cancelled/dismissed while ensure_auth_container/install_agent were still
                // running, before a sessionId existed to cancel against — clean up the session
                // and container that just came into existence instead of silently reattaching.
                await invoke("cancel_agent_auth", { sessionId }).catch(() => {});
                await invoke("destroy_sandbox", { name: authContainer, force: true }).catch(() => {});
                return;
            }

            await startAuthSession(sessionId);
        });

        if (!ok) dismiss(serviceId);
    }

    async function logOut(serviceId: string) {
        await action.run(`logout-${serviceId}`, async () => {
            await invoke("delete_credential_bundle", { serviceId });
            await loadBundles();
        });
    }

    async function submitCode() {
        const sessionId = authSessionId();
        if (!sessionId || !codeInput.trim() || submittingCode) return;
        submittingCode = true;
        try {
            await invoke("send_agent_auth_input", { sessionId, input: codeInput.trim() });
            codeInput = "";
        } finally {
            submittingCode = false;
        }
    }

    function cancelOrDismiss(serviceId: string) {
        const sessionId = authSessionId();
        if (authPhase() === "exited" || !sessionId) {
            dismiss(serviceId);
        } else {
            invoke("cancel_agent_auth", { sessionId }).catch(() => {});
        }
    }

    $effect(() => {
        const id = authSessionId();
        const ph = authPhase();
        if (ph === "exited" && id && id !== handledExitId) {
            handledExitId = id;
            void handleExit(id);
        }
    });

    onMount(loadBundles);
</script>

<main class="p-3 pb-6 @max-[420px]:p-2">
    <h1 class="text-3xl mb-2">{m.credentials_title()}</h1>
    <p class="text-sm text-muted-foreground mb-8">{m.credentials_intro()}</p>

    <div class="flex flex-col gap-3">
        {#each services as service}
            {@const bundle = bundles[service.serviceId]}
            {@const active = activeServiceId === service.serviceId && authPhase() !== "idle"}
            {@const open = cardOpen[service.serviceId] ?? active}
            <Collapsible.Root
                {open}
                onOpenChange={(o) => (cardOpen[service.serviceId] = o)}
                class="rounded-lg border p-3"
                data-service={service.serviceId}
            >
                <div class="flex items-center justify-between gap-2">
                    <Collapsible.Trigger class="flex-1 min-w-0 text-left">
                        <div class="text-sm font-medium truncate">{service.label}</div>
                        <div class="text-xs text-muted-foreground truncate" data-status>
                            {#if active}
                                Signing in… <!-- TODO(i18n) -->
                            {:else if bundle}
                                {bundle.accountEmail ?? m.credentials_status_connected()}
                            {:else}
                                {m.credentials_status_not_connected()}
                            {/if}
                        </div>
                    </Collapsible.Trigger>
                    {#if !active}
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
                            <Button
                                size="sm"
                                onclick={() => logIn(service.serviceId)}
                                disabled={action.busy !== null}
                            >
                                {m.credentials_log_in()}
                            </Button>
                        {/if}
                    {/if}
                </div>

                {#if active}
                    <Collapsible.Content class="pt-3 flex flex-col gap-2">
                        {#if authPhase() === "exited"}
                            {#if authExitCode() === 0}
                                <p class="text-xs text-muted-foreground">Login successful. Finishing up…</p> <!-- TODO(i18n) -->
                            {:else}
                                <p class="text-xs text-destructive">
                                    Authentication failed (exit code {authExitCode()}). <!-- TODO(i18n) -->
                                </p>
                            {/if}
                        {:else if !authUrlDetected()}
                            <p class="text-xs text-muted-foreground">Signing in…</p> <!-- TODO(i18n) -->
                        {/if}

                        {#if authUrlDetected() && authPhase() !== "exited"}
                            <div class="flex items-center gap-2">
                                <span class="text-xs text-muted-foreground truncate flex-1">
                                    {authUrlDetected()}
                                </span>
                                <Button size="sm" onclick={() => openUrl(authUrlDetected()!)}>
                                    Open in browser <!-- TODO(i18n) -->
                                </Button>
                            </div>
                        {/if}

                        {#if authPhase() === "awaiting-code"}
                            <!-- TODO(i18n) placeholder text below -->
                            <div class="flex items-center gap-2">
                                <Input
                                    class="flex-1"
                                    placeholder="Paste authorization code here…"
                                    bind:value={codeInput}
                                    onkeydown={(e) => e.key === "Enter" && submitCode()}
                                    disabled={submittingCode}
                                />
                                <Button size="sm" onclick={submitCode} disabled={submittingCode || !codeInput.trim()}>
                                    Submit <!-- TODO(i18n) -->
                                </Button>
                            </div>
                        {/if}

                        <div class="flex items-center gap-2">
                            <Button size="sm" variant="outline" onclick={() => cancelOrDismiss(service.serviceId)}>
                                Cancel <!-- TODO(i18n) -->
                            </Button>
                            <Button size="sm" variant="ghost" onclick={() => (showDetails = !showDetails)}>
                                {showDetails ? "Hide details" : "Show details"} <!-- TODO(i18n) -->
                            </Button>
                        </div>

                        {#if showDetails}
                            <pre
                                class="max-h-40 overflow-auto whitespace-pre-wrap break-all text-xs leading-relaxed bg-muted rounded p-2"
                            >{#each authOutputLines() as line}{line + "\n"}{/each}</pre>
                        {/if}
                    </Collapsible.Content>
                {/if}
            </Collapsible.Root>
        {/each}
    </div>

    {#if action.error}<p class="text-destructive mt-3 text-sm">{action.error}</p>{/if}
</main>
