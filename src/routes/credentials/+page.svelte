<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Text } from "$lib/components/ui/text";
    import { Input } from "$lib/components/ui/input";
    import * as Collapsible from "$lib/components/ui/collapsible";
    import { Channel } from "@tauri-apps/api/core";
    import { cancelAgentAuth, installAgent, sendAgentAuthInput, startAgentAuth } from "$lib/api/agents";
    import {
        deleteCredentialBundle,
        ensureAuthContainer,
        listCredentialBundles,
        setGithubToken,
        type CredentialManifest,
    } from "$lib/api/credentials";
    import { destroySandbox } from "$lib/api/sandbox";
    import { onMount, untrack } from "svelte";
    import { createActionRunner } from "$lib/actions.svelte";
    import { AGENTS } from "$lib/agents/catalogue";
    import { m } from "$lib/paraglide/messages.js";
    import type { AgentInstallEvent } from "$lib/agents/types";
    import { GITHUB_SERVICE, GITHUB_AUTH_CONTAINER, GITHUB_LOGIN_COMMAND } from "$lib/github";
    import AuthSessionCard from "$lib/agents/AuthSessionCard.svelte";
    import {
        authSessionId,
        authPhase,
        authActiveServiceId,
        authCapturedCount,
        beginSigningIn,
        start as startAuthSession,
        reset as resetAuthSession,
    } from "$lib/agents/authSession.svelte";

    const action = createActionRunner();

    let bundles = $state<Record<string, CredentialManifest>>({});
    let cardOpen = $state<Record<string, boolean>>({});

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
        const entries = await listCredentialBundles();
        bundles = Object.fromEntries(entries);
    }

    function dismiss(serviceId: string) {
        resetAuthSession();
        delete cardOpen[serviceId];
    }

    async function logIn(serviceId: string) {
        const agent = AGENTS.find((a) => a.credentialService === serviceId && a.auth);
        if (!agent || !agent.auth) return;
        const auth = agent.auth;
        const authContainer = `mio-auth-${serviceId}`;

        beginSigningIn(serviceId);
        cardOpen[serviceId] = true;

        const ok = await action.run(`login-${serviceId}`, async () => {
            await ensureAuthContainer(authContainer);

            const npmStep = agent.postInstallSteps.find((s) => s.kind === "npm-postinstall");
            const installChannel = new Channel<AgentInstallEvent>();
            await installAgent({
                name: authContainer,
                packages: agent.misePackages,
                postinstallNpm: npmStep ? [npmStep.package, npmStep.nodeVersion] : null,
                credentialService: null,
                onProgress: installChannel,
            });

            const sessionId = await startAgentAuth(authContainer, auth.command);

            if (authActiveServiceId() !== serviceId) {
                // Cancelled/dismissed while ensure_auth_container/install_agent were still
                // running, before a sessionId existed to cancel against — clean up the session
                // and container that just came into existence instead of silently reattaching.
                await cancelAgentAuth(sessionId).catch(() => {});
                await destroySandbox(authContainer, true).catch(() => {});
                return;
            }

            await startAuthSession(sessionId);
        });

        if (!ok) dismiss(serviceId);
    }

    let githubToken = $state("");
    const githubBundle = $derived(bundles[GITHUB_SERVICE]);
    const githubActive = $derived(authActiveServiceId() === GITHUB_SERVICE && authPhase() !== "idle");

    async function logInGithub() {
        beginSigningIn(GITHUB_SERVICE);
        const ok = await action.run("github-login", async () => {
            await ensureAuthContainer(GITHUB_AUTH_CONTAINER);
            const sessionId = await startAgentAuth(GITHUB_AUTH_CONTAINER, GITHUB_LOGIN_COMMAND);

            if (authActiveServiceId() !== GITHUB_SERVICE) {
                // Dismissed before a sessionId existed to cancel against.
                await cancelAgentAuth(sessionId).catch(() => {});
                await destroySandbox(GITHUB_AUTH_CONTAINER, true).catch(() => {});
                return;
            }

            await startAuthSession(sessionId);
        });
        if (!ok) dismiss(GITHUB_SERVICE);
    }

    async function saveGithubToken() {
        const ok = await action.run("github-save", async () => {
            await setGithubToken(githubToken);
            await loadBundles();
        });
        // The value stays in the backend; clearing the field leaves nothing readable in the webview.
        if (ok) githubToken = "";
    }

    async function removeGithubToken() {
        await action.run("github-remove", async () => {
            await deleteCredentialBundle(GITHUB_SERVICE);
            await loadBundles();
        });
    }

    async function logOut(serviceId: string) {
        await action.run(`logout-${serviceId}`, async () => {
            await deleteCredentialBundle(serviceId);
            await loadBundles();
        });
    }

    async function submitCode(code: string) {
        const sessionId = authSessionId();
        if (!sessionId) return;
        await sendAgentAuthInput(sessionId, code);
    }

    function cancelOrDismiss(serviceId: string) {
        const sessionId = authSessionId();
        if (authPhase() === "exited" || !sessionId) {
            dismiss(serviceId);
        } else {
            cancelAgentAuth(sessionId).catch(() => {});
        }
    }

    // The store captures credentials and destroys the auth container on exit, even while this
    // page is unmounted; the page only refreshes its list when a capture lands.
    $effect(() => {
        if (authCapturedCount() > 0) void untrack(loadBundles);
    });

    onMount(loadBundles);
</script>

<main class="mx-auto w-full max-w-3xl p-6 pb-16 @max-[420px]:p-3">
    <Text as="h1" class="mb-2">{m.credentials_title()}</Text>
    <p class="text-sm text-muted-foreground mb-8">{m.credentials_intro()}</p>

    <div class="flex flex-col gap-3">
        {#each services as service}
            {@const bundle = bundles[service.serviceId]}
            {@const active = authActiveServiceId() === service.serviceId && authPhase() !== "idle"}
            {@const open = active && (cardOpen[service.serviceId] ?? true)}
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
                                {m.auth_signing_in()}
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
                    <Collapsible.Content class="pt-3">
                        <AuthSessionCard
                            onCancel={() => cancelOrDismiss(service.serviceId)}
                            onSubmitCode={submitCode}
                        />
                    </Collapsible.Content>
                {/if}
            </Collapsible.Root>
        {/each}

        <div class="rounded-lg border p-3 flex flex-col gap-2" data-service={GITHUB_SERVICE}>
            <div class="flex items-center justify-between gap-2">
                <div class="min-w-0">
                    <div class="text-sm font-medium truncate">{m.credentials_github_title()}</div>
                    <div class="text-xs text-muted-foreground truncate" data-status>
                        {#if githubActive}
                            {m.auth_signing_in()}
                        {:else if githubBundle?.accountLogin}
                            {`Logged in as ${githubBundle.accountLogin}`}<!-- TODO(i18n) -->
                        {:else if githubBundle}
                            {m.credentials_github_saved({ date: new Date(githubBundle.capturedAtMs).toLocaleDateString() })}
                        {:else}
                            {m.credentials_github_not_saved()}
                        {/if}
                    </div>
                </div>
                {#if !githubActive}
                    {#if githubBundle}
                        <Button
                            size="sm"
                            variant="outline"
                            onclick={removeGithubToken}
                            disabled={action.busy !== null}
                            data-github-remove
                        >
                            {m.credentials_github_remove()}
                        </Button>
                    {/if}
                    <Button
                        size="sm"
                        onclick={logInGithub}
                        disabled={action.busy !== null}
                        data-github-login
                    >
                        {githubBundle ? "Log in again with GitHub" : "Log in with GitHub"}<!-- TODO(i18n) -->
                    </Button>
                {/if}
            </div>
            {#if githubBundle && githubBundle.missingScopes.length > 0 && !githubActive}
                <p class="text-xs text-destructive" data-github-scopes>
                    {`The token lacks ${githubBundle.missingScopes.join(", ")}; sandboxes may reject it.`}<!-- TODO(i18n) -->
                </p>
            {/if}
            {#if githubActive}
                <AuthSessionCard
                    onCancel={() => cancelOrDismiss(GITHUB_SERVICE)}
                    onSubmitCode={submitCode}
                />
            {/if}
            <p class="text-xs text-muted-foreground">
                {m.credentials_github_help()}
            </p>
            <form
                class="flex gap-2"
                onsubmit={(e) => {
                    e.preventDefault();
                    void saveGithubToken();
                }}
            >
                <Input
                    type="password"
                    autocomplete="off"
                    placeholder={githubBundle ? m.credentials_github_placeholder_replace() : m.credentials_github_placeholder()}
                    bind:value={githubToken}
                    data-github-token
                />
                <Button
                    size="sm"
                    type="submit"
                    disabled={action.busy !== null || githubToken.trim() === ""}
                    data-github-save
                >
                    {m.settings_save()}
                </Button>
            </form>
        </div>
    </div>

    {#if action.error}<p class="text-destructive mt-3 text-sm">{action.error}</p>{/if}
</main>
