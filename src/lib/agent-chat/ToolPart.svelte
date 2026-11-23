<script lang="ts">
    import * as Trace from "$lib/components/ai-elements/trace/index.js";
    import * as CodeBlock from "$lib/components/ai-elements/code-block/index.js";
    import AgentRow from "./AgentRow.svelte";
    import { askFrameOf, frameOf, hasRun, isLive, toolKindOf } from "./anatomy";
    import { avatarLabel } from "./anatomy-copy";
    import type { RunState, ToolChatPart } from "./types";

    let {
        part,
        learning = false,
        run = "idle",
    }: {
        part: ToolChatPart;
        /** Splits the call into the Brain's request and the Arms' execution. */
        learning?: boolean;
        /** After the run ended, a call still in state `input` or `composing` was never run. */
        run?: RunState;
    } = $props();

    const str = (v: unknown) => (typeof v === "string" ? v : "");
    const input = $derived(part.input);
    const path = $derived(str(input.path) || str(input.file_path));
    // Keyed by tool name, like pi-web-ui's renderer registry (01_ideas.md, Components).
    const summary = $derived.by(() => {
        switch (part.toolName) {
            case "bash":
                return str(input.command);
            case "read":
            case "write":
            case "edit":
                return path;
            default:
                return "";
        }
    });
    const status = $derived(
        part.state === "running"
            ? "running"
            : part.state === "error"
              ? "error"
              : part.state === "input" || part.state === "composing"
                ? "pending"
                : undefined,
    );
    const settled = $derived(run === "idle");
    const composing = $derived(part.state === "composing" && !settled);
    const kind = $derived(toolKindOf(part.toolName));
    const frame = $derived(frameOf(part, run));
    const askFrame = $derived(askFrameOf(frame));
    const live = $derived(isLive(part, run));
    /** An edit's replacements as removed and added lines. */
    const editLines = $derived.by(() => {
        if (part.toolName !== "edit") return [];
        const edits: unknown[] = Array.isArray(input.edits) ? input.edits : [input];
        return edits.flatMap((e) => {
            const edit = (e ?? {}) as Record<string, unknown>;
            const removed = str(edit.oldText ?? edit.old_string)
                .split("\n")
                .map((code) => ({ code, variant: "removed" as const }));
            const added = str(edit.newText ?? edit.new_string)
                .split("\n")
                .map((code) => ({ code, variant: "added" as const }));
            return [...removed, ...added];
        });
    });
</script>

{#snippet details()}
    <div class="flex flex-col gap-2 py-2">
        {#if part.toolName === "bash"}
            <CodeBlock.Root
                size="sm"
                variant={part.state === "error" ? "destructive" : "default"}
                class="bg-neutral-950 text-neutral-100"
            >
                <CodeBlock.Content class="font-mono" wrap code={`$ ${str(input.command)}\n${part.output}`} />
            </CodeBlock.Root>
        {:else if part.toolName === "edit" && editLines.length > 0}
            <CodeBlock.Root size="sm">
                <CodeBlock.Header><CodeBlock.Title>{path}</CodeBlock.Title></CodeBlock.Header>
                <CodeBlock.Content wrap>
                    {#each editLines as line, i (i)}
                        <CodeBlock.Line code={line.code} variant={line.variant} />
                    {/each}
                </CodeBlock.Content>
            </CodeBlock.Root>
            {#if part.state === "error"}
                <p class="text-destructive text-xs whitespace-pre-wrap">{part.output}</p>
            {/if}
        {:else if part.toolName === "write"}
            <CodeBlock.Root size="sm">
                <CodeBlock.Header><CodeBlock.Title>{path}</CodeBlock.Title></CodeBlock.Header>
                <CodeBlock.Content wrap code={str(input.content)} />
            </CodeBlock.Root>
        {:else}
            {#if part.toolName !== "read"}
                <CodeBlock.Root size="sm">
                    <CodeBlock.Content wrap code={JSON.stringify(input, null, 2)} />
                </CodeBlock.Root>
            {/if}
            {#if part.output}
                <CodeBlock.Root size="sm" variant={part.state === "error" ? "destructive" : "default"}>
                    <CodeBlock.Content wrap code={part.output} />
                </CodeBlock.Root>
            {/if}
        {/if}
    </div>
{/snippet}

{#if !learning}
    <Trace.Root data-tool={part.toolName} data-tool-state={part.state}>
        <Trace.Collapsible>
            <Trace.CollapsibleTrigger>
                <Trace.Line {status}>
                    <Trace.Title>{part.toolName}</Trace.Title>
                    {#if summary}
                        <Trace.Description class="truncate font-mono">{summary}</Trace.Description>
                    {/if}
                </Trace.Line>
            </Trace.CollapsibleTrigger>
            <Trace.CollapsibleContent>
                {@render details()}
            </Trace.CollapsibleContent>
        </Trace.Collapsible>
    </Trace.Root>
{:else}
    <div class="flex flex-col" data-tool={part.toolName} data-tool-state={part.state}>
        <AgentRow
            role="brain-ask"
            frame={askFrame}
            live={live && (frame === "compose" || frame === "request")}
            label={avatarLabel("brain-ask", askFrame, part.toolName, live)}
            tool={kind}
        >
            <div data-tool-band="ask">
                <Trace.Root>
                    <Trace.Line status={composing ? "running" : undefined}>
                        <Trace.Title>{part.toolName}</Trace.Title>
                        {#if part.state === "input" && !settled}
                            <Trace.Description>· waiting to run</Trace.Description> <!-- TODO(i18n) -->
                        {/if}
                        {#if summary}
                            <Trace.Description class="truncate font-mono">{summary}</Trace.Description>
                        {/if}
                    </Trace.Line>
                </Trace.Root>
            </div>
        </AgentRow>
        {#if hasRun(part)}
            <AgentRow
                role="arms-act"
                {frame}
                live={live && frame === "act"}
                label={avatarLabel("arms-act", frame, part.toolName, live)}
                tool={kind}
            >
                <div data-tool-band="act">
                    <Trace.Root>
                        <Trace.Collapsible>
                            <Trace.CollapsibleTrigger>
                                <Trace.Line {status}>
                                    <!-- TODO(i18n) -->
                                    <Trace.Title>
                                        {part.state === "running"
                                            ? "Running in your sandbox"
                                            : part.state === "error"
                                              ? "Ran into an error"
                                              : "Finished"}
                                    </Trace.Title>
                                </Trace.Line>
                            </Trace.CollapsibleTrigger>
                            <Trace.CollapsibleContent>
                                {@render details()}
                            </Trace.CollapsibleContent>
                        </Trace.Collapsible>
                    </Trace.Root>
                </div>
            </AgentRow>
        {:else if settled && (part.state === "input" || part.state === "composing")}
            <AgentRow>
                <p class="text-muted-foreground m-0 text-xs" data-tool-band="act">Asked, but not run</p> <!-- TODO(i18n) -->
            </AgentRow>
        {/if}
    </div>
{/if}
