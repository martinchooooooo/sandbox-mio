<script lang="ts">
    import * as Trace from "$lib/components/ai-elements/trace/index.js";
    import { m } from "$lib/paraglide/messages.js";

    let { text, streaming }: { text: string; streaming: boolean } = $props();
</script>

<Trace.Root data-part="reasoning">
    <Trace.Collapsible>
        <Trace.CollapsibleTrigger>
            <Trace.Line status={streaming ? "running" : undefined}>
                <Trace.Title>{streaming ? m.agent_chat_thinking_active() : m.agent_chat_thought()}</Trace.Title>
            </Trace.Line>
        </Trace.CollapsibleTrigger>
        <Trace.CollapsibleContent>
            <p class="text-muted-foreground py-2 text-xs whitespace-pre-wrap">{text}</p>
        </Trace.CollapsibleContent>
    </Trace.Collapsible>
</Trace.Root>
