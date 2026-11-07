<script lang="ts">
    import { onMount } from "svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import WelcomeFlow from "$lib/welcome/WelcomeFlow.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import {
        ensureOnboardingState,
        onboardingCompleted,
        resetOnboarding,
    } from "$lib/settings/onboarding.svelte";

    onMount(ensureOnboardingState);
</script>

<div class="relative min-h-full">
    {#if onboardingCompleted() === false}
        <WelcomeFlow variant="onboarding" />
    {:else if onboardingCompleted()}
        <main class="min-h-full flex flex-col items-center justify-center gap-4 p-8 text-center">
            <h1 class="text-3xl font-bold">{m.home_all_set_title()}</h1>
            <p class="text-muted-foreground max-w-md">{m.home_all_set_body()}</p>
            <Button href="/projects">{m.home_all_set_cta()}</Button>
        </main>
    {/if}

    <!-- Debug aid, ungated: this route is not reachable from any nav tab once onboarding is
         complete (docs/features/welcome-onboarding/02_dev.md, Resolved Decisions). -->
    <Button
        variant="outline"
        size="sm"
        class="fixed bottom-3 left-3 opacity-60"
        onclick={resetOnboarding}
        data-restart-onboarding
    >Restart onboarding</Button>
</div>
