<script lang="ts">
    import { onMount } from "svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import { Text } from "$lib/components/ui/text";
    import WelcomeFlow from "$lib/welcome/WelcomeFlow.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { ensureOnboardingState, onboardingCompleted } from "$lib/settings/onboarding.svelte";

    onMount(ensureOnboardingState);
</script>

<div class="relative min-h-full">
    {#if onboardingCompleted() === false}
        <WelcomeFlow variant="onboarding" />
    {:else if onboardingCompleted()}
        <main class="min-h-full flex flex-col items-center justify-center gap-4 p-8 text-center">
            <Text as="h1">{m.home_all_set_title()}</Text>
            <p class="text-muted-foreground max-w-md">{m.home_all_set_body()}</p>
            <Button href="/projects">{m.home_all_set_cta()}</Button>
        </main>
    {/if}
</div>
