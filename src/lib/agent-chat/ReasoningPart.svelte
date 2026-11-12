<script lang="ts">
    import * as Trace from "$lib/components/ai-elements/trace/index.js";

    let { text, streaming }: { text: string; streaming: boolean } = $props();
</script>

<Trace.Root data-part="reasoning">
    <Trace.Collapsible>
        <Trace.CollapsibleTrigger>
            <Trace.Line status={streaming ? "running" : undefined}>
                <Trace.Title>{streaming ? "Thinking…" : "Thought"}</Trace.Title><!-- TODO(i18n) -->
            </Trace.Line>
        </Trace.CollapsibleTrigger>
        <Trace.CollapsibleContent>
            <p class="text-muted-foreground py-2 text-xs whitespace-pre-wrap">{text}</p>
        </Trace.CollapsibleContent>
    </Trace.Collapsible>
</Trace.Root>
