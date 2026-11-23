<script lang="ts">
    import { modelsState } from "./model.svelte";
    import { PAGE, PLAIN_STEP_TITLES } from "./copy";

    const wake = $derived(modelsState().wake);

    // The step still running, folded from the stream the way RuntimeSteps.svelte folds it.
    const line = $derived.by(() => {
        let current: string | null = null;
        for (const e of wake.events) {
            if (e.kind === "ready") return { text: PAGE.ready, failed: false };
            if (e.kind === "stepFailed") return { text: PAGE.failed, failed: true };
            if (e.kind === "stepStart" || e.kind === "stepProgress") current = e.id;
            else if (e.id === current) current = null;
        }
        return { text: (current && PLAIN_STEP_TITLES[current]) || PAGE.waking, failed: false };
    });
</script>

<p class={["text-sm", line.failed ? "text-destructive" : "text-muted-foreground"]} data-wake-status>
    {line.text}
</p>
