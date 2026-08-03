<script lang="ts">
    import { onMount } from "svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import WelcomeFlow from "$lib/welcome/WelcomeFlow.svelte";
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
            <!-- TODO(i18n) -->
            <h1 class="text-3xl font-bold">You're all set</h1>
            <!-- TODO(i18n) -->
            <p class="text-muted-foreground max-w-md">
                Everything is ready. Create your first sandbox to start working in it.
            </p>
            <!-- TODO(i18n) -->
            <Button href="/projects">Get started with your first sandbox</Button>
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
