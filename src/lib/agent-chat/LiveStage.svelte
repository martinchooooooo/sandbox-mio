<script lang="ts">
    import "$lib/story-stage/stage.css";
    import { onMount } from "svelte";
    import { createStage } from "$lib/story-stage";
    import type { Beat } from "./anatomy";
    import { BOARD, LAYOUT, createDriver, type Driver } from "./anatomy-driver";
    import type { ChatStore } from "./chat.svelte";

    let { store, beat }: { store: ChatStore; beat: Beat } = $props();

    let svg = $state<SVGSVGElement>();
    let driver = $state.raw<Driver>();
    let drawn = $state("idle");
    let reducedMotion = false;
    let settledEpoch = -1;

    const highlight = $derived(store.selected ? (store.selected.role === "arms-act" ? "arms" : "brain") : "");

    onMount(() => {
        // The stage animates with JS tweens, which the central reduced-motion tokens cannot reach.
        reducedMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
        const stage = createStage(svg!, { size: BOARD });
        stage.resetStage(LAYOUT);
        stage.actors.brain.g.classList.add("live-brain");
        const made = createDriver(stage, (kind) => (drawn = kind));
        driver = made;
        return () => made.dispose();
    });

    // The first beat after mount and the first after a history rebuild are drawn settled, so
    // opening an old chat never replays it.
    $effect(() => {
        const target = driver;
        if (!target) return;
        const current = beat;
        const epoch = store.epoch;
        if (reducedMotion || epoch !== settledEpoch) {
            settledEpoch = epoch;
            target.settle(current);
        } else {
            target.push(current);
        }
    });
</script>

<section class="story-stage live" data-live-stage data-stage-beat={drawn} data-hl={highlight}>
    <!-- TODO(i18n) -->
    <svg bind:this={svg} viewBox="0 0 {BOARD.width} {BOARD.height}" role="img" aria-label="Animated diagram of the Brain and Arms"></svg>
</section>
