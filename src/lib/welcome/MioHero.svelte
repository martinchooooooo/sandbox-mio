<script lang="ts">
    import { onMount, onDestroy } from "svelte";
    import gsap from "gsap";
    import Robot from "$lib/Robot.svelte";

    // Fixed decorative list, deliberately not derived from the supported-locale list in `messages/`
    // (docs/features/welcome-onboarding/02_dev.md, Resolved Decisions).
    const words = [
        { word: "mio", lang: "it" },
        { word: "Mine", lang: "en" },
        { word: "Meins", lang: "de" },
        { word: "Мой", lang: "ru" },
    ];

    let slots = $state<HTMLElement[]>([]);
    let timeline: gsap.core.Timeline | undefined;

    // Element refs rather than selector strings: two hero instances would otherwise drive each
    // other's tweens, since gsap resolves a selector against the whole document.
    onMount(() => {
        timeline = gsap.timeline({ repeat: -1 });
        for (let i = 0; i < slots.length; i++) {
            gsap.set(slots[i], { opacity: i === 0 ? 1 : 0, y: i === 0 ? 0 : 16 });
        }
        for (let i = 0; i < slots.length; i++) {
            const next = slots[(i + 1) % slots.length];
            timeline
                .to(slots[i], { opacity: 0, y: -16, duration: 0.45, ease: "power2.in" }, `+=1.6`)
                .fromTo(
                    next,
                    { opacity: 0, y: 16 },
                    { opacity: 1, y: 0, duration: 0.45, ease: "power2.out" },
                    "<"
                );
        }
    });

    onDestroy(() => timeline?.kill());
</script>

<div class="flex flex-col items-center gap-6 text-center">
    <div class="w-full max-w-[220px]">
        <Robot />
    </div>
    <h1 class="text-4xl sm:text-5xl font-bold flex items-baseline justify-center gap-3">
        <span>Sandbox</span>
        <!-- The stack is sized by the longest word so the headline never reflows mid-cycle. -->
        <span class="relative inline-grid text-primary">
            {#each words as entry, i (entry.lang)}
                <span
                    bind:this={slots[i]}
                    lang={entry.lang}
                    class="col-start-1 row-start-1 whitespace-nowrap"
                >{entry.word}</span>
            {/each}
        </span>
    </h1>
</div>
