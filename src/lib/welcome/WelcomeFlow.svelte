<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import MioHero from "./MioHero.svelte";
    import CreateVmStep from "./CreateVmStep.svelte";
    import DownloadNecessitiesStep from "./DownloadNecessitiesStep.svelte";
    import { completeOnboarding } from "$lib/settings/onboarding.svelte";
    import { m } from "$lib/paraglide/messages.js";

    let { variant }: { variant: "onboarding" | "replay" } = $props();

    // Component-local on purpose: a step machine that should reset when the route is left
    // (src/AGENTS.md's rune-store convention).
    let step = $state<"hero" | "welcome" | "actions">("hero");

    let vmDone = $state(false);
    let imageDone = $state(false);

    let finishing = $state(false);

    async function finish() {
        finishing = true;
        try {
            await completeOnboarding();
        } finally {
            finishing = false;
        }
    }
</script>

<main class="min-h-full flex flex-col items-center justify-center p-8" data-welcome-step={step}>
    {#if step === "hero"}
        <div class="space-y-10">
            <MioHero />
            <div class="text-center">
                <Button onclick={() => (step = "welcome")}>{m.welcome_get_started()}</Button>
            </div>
        </div>
    {:else if step === "welcome"}
        <div class="max-w-lg text-center space-y-5">
            <h1 class="text-3xl font-bold">{m.welcome_heading()}</h1>
            <p class="text-muted-foreground">{m.welcome_intro_isolated()}</p>
            <p class="text-muted-foreground">{m.welcome_intro_walkthrough()}</p>
            <Button onclick={() => (step = "actions")}>{m.welcome_continue()}</Button>
        </div>
    {:else}
        <div class="w-full max-w-3xl space-y-6">
            <h1 class="text-2xl font-bold text-center">{m.welcome_first_stops()}</h1>
            <div class="grid gap-4 @max-[720px]:grid-cols-1 grid-cols-2">
                <CreateVmStep ondone={(done) => (vmDone = done)} />
                <DownloadNecessitiesStep ondone={(done) => (imageDone = done)} />
            </div>
            {#if variant === "onboarding"}
                <div class="text-center">
                    <Button
                        disabled={!vmDone || !imageDone || finishing}
                        loading={finishing}
                        onclick={finish}
                        data-welcome-finish
                    >{m.welcome_finish()}</Button>
                </div>
            {/if}
        </div>
    {/if}
</main>
