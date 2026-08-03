<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import MioHero from "./MioHero.svelte";
    import CreateVmStep from "./CreateVmStep.svelte";
    import DownloadNecessitiesStep from "./DownloadNecessitiesStep.svelte";
    import { completeOnboarding } from "$lib/settings/onboarding.svelte";

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
                <!-- TODO(i18n) -->
                <Button onclick={() => (step = "welcome")}>Get started</Button>
            </div>
        </div>
    {:else if step === "welcome"}
        <div class="max-w-lg text-center space-y-5">
            <!-- TODO(i18n) -->
            <h1 class="text-3xl font-bold">Welcome to Sandbox mio</h1>
            <!-- TODO(i18n) -->
            <p class="text-muted-foreground">
                Sandbox mio gives every project its own isolated container, with its own runtime,
                terminal and AI coding agents.
            </p>
            <!-- TODO(i18n) -->
            <p class="text-muted-foreground">
                Let's walk through it together. There is a short setup first — two steps, and then
                you can create your first sandbox.
            </p>
            <!-- TODO(i18n) -->
            <Button onclick={() => (step = "actions")}>Continue</Button>
        </div>
    {:else}
        <div class="w-full max-w-3xl space-y-6">
            <!-- TODO(i18n) -->
            <h1 class="text-2xl font-bold text-center">First two stops</h1>
            <div class="grid gap-4 @max-[720px]:grid-cols-1 grid-cols-2">
                <CreateVmStep ondone={(done) => (vmDone = done)} />
                <DownloadNecessitiesStep ondone={(done) => (imageDone = done)} />
            </div>
            {#if variant === "onboarding"}
                <div class="text-center">
                    <!-- TODO(i18n) -->
                    <Button
                        disabled={!vmDone || !imageDone || finishing}
                        loading={finishing}
                        onclick={finish}
                        data-welcome-finish
                    >Finish</Button>
                </div>
            {/if}
        </div>
    {/if}
</main>
