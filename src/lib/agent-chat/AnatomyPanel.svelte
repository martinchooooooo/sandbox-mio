<script lang="ts">
    import { Text } from "$lib/components/ui/text/index.js";
    import { currentBeat } from "./anatomy";
    import LiveStage from "./LiveStage.svelte";
    import { beatLine, INTRO, ROLE_COPY, roleForBeat } from "./anatomy-copy";
    import type { ChatStore } from "./chat.svelte";

    let { store }: { store: ChatStore } = $props();

    const beat = $derived(currentBeat(store.chat));
    const role = $derived(store.selected?.role ?? roleForBeat(beat.kind));
    const copy = $derived(role ? ROLE_COPY[role] : INTRO);
</script>

<div
    class="flex h-full min-h-0 flex-col gap-3 overflow-y-auto p-4"
    data-anatomy-panel
    data-beat={beat.kind}
    data-explainer-role={role ?? "intro"}
>
    <LiveStage {store} {beat} />
    <p class="m-0 min-h-[2.5em] text-sm font-bold" aria-live="polite" data-anatomy-beat-line>{beatLine(beat)}</p>
    <section class="flex flex-col gap-1" data-anatomy-explainer>
        <Text as="h3">{copy.title}</Text>
        <p class="m-0 text-sm text-muted-foreground">{copy.body}</p>
    </section>
</div>
