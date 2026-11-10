<script lang="ts">
    import "../stage/stage.css";
    import { onMount, untrack } from "svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import { CANCEL, createStage, type Scene, type StoryStage } from "../stage";

    // The animated whiteboard. Plays `scene` whenever it changes; with no scene it collapses, so
    // a text-only page costs no empty space.
    let {
        scene,
        auto = false,
        speed = 1,
    }: {
        scene: Scene | undefined;
        /** Click-along scenes play themselves with a fake cursor. */
        auto?: boolean;
        speed?: number;
    } = $props();

    let svg = $state<SVGSVGElement>();
    let stage = $state.raw<StoryStage>();
    let caption = $state("");
    // Bumped per caption so an identical line still replays its entrance.
    let captionTick = $state(0);

    onMount(() => {
        stage = createStage(svg!, {
            auto,
            speed,
            onCaption(text) {
                caption = text;
                captionTick += 1;
            },
        });
        const built = stage;
        return () => built.cancelAll();
    });

    function run(target: StoryStage, next: Scene | undefined) {
        // Stops the previous page's script before anything is redrawn.
        target.cancelAll();
        if (!next) return;
        const script = next(target);
        target.resetStage(script.layout);
        script.setup?.();
        script.play().catch((e) => {
            if (e !== CANCEL) console.error(e);
        });
    }

    $effect(() => {
        const target = stage;
        const next = scene;
        // Scenes write caption state; untracked so that never re-triggers this effect.
        if (target) untrack(() => run(target, next));
    });

    /** Restart the current scene from its first beat. */
    export function replay() {
        if (stage) run(stage, scene);
    }
</script>

<section class="story-stage shrink-0 border-b-2 border-border" class:hidden={!scene} data-story-stage>
    <!-- TODO(i18n) -->
    <svg bind:this={svg} viewBox="0 0 1000 440" role="img" aria-label="Animated diagram"></svg>
    <p class="m-0 min-h-[2em] px-4 pb-2 text-center text-[17px] font-bold" aria-live="polite" data-story-caption>
        {#key captionTick}
            <span class="inline-block animate-enter">{caption}</span>
        {/key}
    </p>
    <!-- TODO(i18n) -->
    <Button variant="outline" size="sm" class="absolute right-3 top-3 bg-white" onclick={replay} data-story-replay>↻ Replay</Button>
</section>
