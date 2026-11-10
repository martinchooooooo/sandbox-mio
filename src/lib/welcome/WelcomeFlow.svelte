<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import MioHero from "./MioHero.svelte";
    import CreateVmStep from "./CreateVmStep.svelte";
    import DownloadNecessitiesStep from "./DownloadNecessitiesStep.svelte";
    import ActivityCenter from "$lib/activity/ActivityCenter.svelte";
    import type { ActivityHeadline } from "$lib/activity/types";
    import OnboardingBook from "$lib/onboarding/OnboardingBook.svelte";
    import { setupHeadline, startBackgroundSetup } from "$lib/onboarding/setup.svelte";
    import { completeOnboarding } from "$lib/settings/onboarding.svelte";
    import { m } from "$lib/paraglide/messages.js";

    let { variant }: { variant: "onboarding" | "replay" } = $props();

    // Component-local on purpose: a step machine that should reset when the route is left
    // (src/AGENTS.md's rune-store convention).
    let step = $state<"hero" | "welcome" | "story" | "actions">("hero");

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

    // Setup starts here so it runs while the user reads. A replay only walks through: it must
    // not trigger anything (docs/features/welcome-onboarding/02_dev.md, Phase D).
    function beginStory() {
        if (variant === "onboarding") startBackgroundSetup();
        step = "story";
    }

    // TODO(i18n): the hint and the action label.
    const headline = $derived.by((): ActivityHeadline | null => {
        const summary = setupHeadline();
        if (!summary) return null;
        return {
            ...summary,
            hint:
                summary.state === "running"
                    ? "While you read, we are getting your computer ready. Check on it here any time."
                    : undefined,
            action:
                summary.state === "attention" && step !== "actions"
                    ? { label: "Go to setup", onclick: () => (step = "actions") }
                    : undefined,
        };
    });
</script>

<div class="flex min-h-full flex-col">
<!-- A bar rather than the usual corner bell: the corner is where the book's Next button sits, and
     a first-run user has no reason to look there. A replay runs inside the navigation column,
     which already has an activity center. -->
{#if variant === "onboarding" && headline}
    <header class="sticky top-0 z-40 flex items-center justify-between gap-3 border-b-2 bg-background px-4 py-2 animate-fade-in" data-welcome-status-bar>
        <span class="font-head text-sm">Sandbox mio</span>
        <ActivityCenter {headline} placement="inline" />
    </header>
{/if}
<main class={["flex flex-1 flex-col items-center justify-center @max-[640px]:px-3", step === "story" ? "px-8 py-4" : "p-8"]} data-welcome-step={step}>
    {#if step === "hero"}
        <div class="space-y-10">
            <MioHero />
            <div class="text-center">
                <Button onclick={() => (step = "welcome")} data-welcome-get-started>{m.welcome_get_started()}</Button>
            </div>
        </div>
    {:else if step === "welcome"}
        <div class="max-w-lg text-center space-y-5">
            <h1 class="text-3xl font-bold">{m.welcome_heading()}</h1>
            <p class="text-muted-foreground">{m.welcome_intro_isolated()}</p>
            <!-- TODO(i18n) -->
            <p class="text-muted-foreground">
                Let's start with a short illustrated tour of how AI agents work and why they belong in a sandbox.
                {#if variant === "onboarding"}
                    While you read, we will set up your computer in the background, so everything is ready when you finish.
                {/if}
            </p>
            <Button onclick={beginStory} data-welcome-begin>{m.welcome_continue()}</Button>
        </div>
    {:else if step === "story"}
        <div class="flex w-full flex-col items-center gap-2">
            <!-- TODO(i18n): the finish label and the skip link. -->
            <OnboardingBook
                onfinish={() => (step = "actions")}
                finishLabel="Finish the tour →"
                class="max-h-[calc(100vh-9.5rem)] min-h-[420px]"
            />
            <Button variant="link" size="sm" onclick={() => (step = "actions")} data-welcome-skip>Skip the tour</Button>
        </div>
    {:else}
        <div class="w-full max-w-3xl space-y-6 pb-16">
            <h1 class="text-2xl font-bold text-center">{m.welcome_first_stops()}</h1>
            <div class="grid gap-4 @max-[720px]:grid-cols-1 grid-cols-2">
                <CreateVmStep ondone={(done) => (vmDone = done)} />
                <DownloadNecessitiesStep ondone={(done) => (imageDone = done)} />
            </div>
            <div class="flex items-center justify-center gap-3">
                <!-- TODO(i18n) -->
                <Button variant="outline" onclick={() => (step = "story")} data-welcome-back-to-story>← Back to the tour</Button>
                {#if variant === "onboarding"}
                    <Button
                        disabled={!vmDone || !imageDone || finishing}
                        loading={finishing}
                        onclick={finish}
                        data-welcome-finish
                    >{m.welcome_finish()}</Button>
                {/if}
            </div>
        </div>
    {/if}
</main>
</div>
