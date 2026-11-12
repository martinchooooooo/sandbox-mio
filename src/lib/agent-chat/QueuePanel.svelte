<script lang="ts">
    import * as Trace from "$lib/components/ai-elements/trace/index.js";
    import type { QueueState } from "./types";

    /** Messages pi has queued during a run, in the AI Elements Queue pattern. Steering messages are
     * delivered after the current tool call, follow-ups once the run would otherwise end. */
    let { queue }: { queue: QueueState } = $props();
</script>

{#if queue.steering.length + queue.followUp.length > 0}
    <Trace.Root class="rounded-md border-2 bg-card px-3 py-2" data-chat-queue>
        {#each queue.steering as message, i (`s${i}`)}
            <Trace.Line status="pending" data-queued="steer">
                <Trace.Title>Steer</Trace.Title> <!-- TODO(i18n) -->
                <Trace.Description class="truncate">{message}</Trace.Description>
            </Trace.Line>
        {/each}
        {#each queue.followUp as message, i (`f${i}`)}
            <Trace.Line status="pending" data-queued="follow_up">
                <Trace.Title>Follow-up</Trace.Title> <!-- TODO(i18n) -->
                <Trace.Description class="truncate">{message}</Trace.Description>
            </Trace.Line>
        {/each}
    </Trace.Root>
{/if}
