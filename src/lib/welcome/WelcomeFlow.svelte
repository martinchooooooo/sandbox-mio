<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Text } from "$lib/components/ui/text";
    import MioHero from "./MioHero.svelte";
    import HeadsUpStep from "./HeadsUpStep.svelte";
    import SetupProgress from "./SetupProgress.svelte";
    import ActivityCenter from "$lib/activity/ActivityCenter.svelte";
    import type { ActivityHeadline } from "$lib/activity/types";
    import OnboardingBook from "$lib/onboarding/OnboardingBook.svelte";
    import {
        firstSandboxReady,
        firstSandboxWanted,
        requestFirstSandbox,
        setupComplete,
        setupHeadline,
        skipModelSetup,
        startBackgroundSetup,
        startFirstSandbox,
        startModelSetup,
    } from "$lib/onboarding/setup.svelte";
    import {
        cancelHandoff,
        handoffError,
        handoffPhase,
        retryHandoff,
        startHandoff,
    } from "$lib/onboarding/handoff.svelte";
    import { recordMilestone } from "$lib/onboarding/milestones";
    import { completeOnboarding } from "$lib/settings/onboarding.svelte";
    import { m } from "$lib/paraglide/messages.js";

    let { variant }: { variant: "onboarding" | "replay" } = $props();

    // Component-local on purpose: a step machine that should reset when the route is left
    // (src/AGENTS.md's rune-store convention).
    let step = $state<"hero" | "consent" | "story" | "actions">("hero");

    let cancelling = $state(false);

    // Leaves for the sandbox list; the first sandbox keeps building in the background.
    async function cancelHandoffToList() {
        cancelling = true;
        cancelHandoff();
        try {
            await completeOnboarding();
        } finally {
            cancelling = false;
        }
    }

    // Consent is given here, so everything starts from this one action and nowhere else: not the
    // tour's pages, a skip or a replay (docs/features/fastest-time-to-wow/02_dev.md, Phase E).
    function confirmHeadsUp() {
        recordMilestone("heads-up-confirmed");
        requestFirstSandbox();
        startBackgroundSetup();
        void startModelSetup();
        step = "story";
    }

    function declineModel() {
        skipModelSetup();
        requestFirstSandbox();
        startBackgroundSetup();
        step = "story";
    }

    // The first sandbox is built in the background as soon as machine and image are ready, in
    // parallel with the model; the effect also covers a machine or image retried from its card.
    $effect(() => {
        if (variant === "onboarding" && firstSandboxWanted() && setupComplete()) void startFirstSandbox();
    });

    // The progress page hands over to the first sandbox on its own once everything is ready.
    $effect(() => {
        if (variant === "onboarding" && step === "actions" && firstSandboxReady() && handoffPhase() === "idle") {
            void startHandoff();
        }
    });

    // TODO(i18n): the hint and the action label.
    const headline = $derived.by((): ActivityHeadline | null => {
        const summary = setupHeadline("podman", variant === "onboarding");
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
                <Button onclick={() => (step = variant === "onboarding" ? "consent" : "story")} data-welcome-get-started>{m.welcome_get_started()}</Button>
            </div>
        </div>
    {:else if step === "consent"}
        <HeadsUpStep onconfirm={confirmHeadsUp} ondecline={declineModel} />
    {:else if step === "story"}
        <div class="flex w-full flex-col items-center gap-2">
            <!-- TODO(i18n): the finish label and the skip link. -->
            <OnboardingBook
                onfinish={() => (step = "actions")}
                finishLabel="Finish the tour →"
                class="max-h-[calc(100vh-9.5rem)] min-h-[420px]"
            />
            <!-- TODO(i18n) -->
            <Button variant="link" size="sm" onclick={() => (step = "actions")} data-welcome-skip>Skip the tour</Button>
        </div>
    {:else}
        <div class="w-full max-w-3xl space-y-6 pb-16">
            <!-- TODO(i18n) -->
            <Text as="h1" class="text-center">Getting things ready</Text>
            <SetupProgress showModel={variant === "onboarding"} />
            {#if variant === "onboarding" && firstSandboxReady()}
                <!-- TODO(i18n) -->
                <div class="text-center space-y-2" data-welcome-handoff={handoffPhase()}>
                    {#if handoffPhase() === "failed"}
                        <p class="text-destructive text-sm">Your first sandbox did not come together. {handoffError()}</p>
                        <Button onclick={retryHandoff}>Try again</Button>
                    {:else}
                        <p class="text-sm font-medium">Opening your first sandbox, hello world…</p>
                    {/if}
                </div>
            {/if}
            <div class="flex items-center justify-center gap-3">
                <!-- TODO(i18n) -->
                <Button variant="outline" onclick={() => (step = "story")} data-welcome-back-to-story>← Back to the tour</Button>
                {#if variant === "onboarding" && firstSandboxReady()}
                    <Button variant="outline" loading={cancelling} onclick={cancelHandoffToList} data-welcome-cancel>Go to my sandbox list</Button>
                {/if}
            </div>
        </div>
    {/if}
</main>
</div>
