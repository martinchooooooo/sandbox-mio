<script lang="ts">
    import * as Trace from "$lib/components/ai-elements/trace/index.js";
    import * as CodeBlock from "$lib/components/ai-elements/code-block/index.js";
    import type { ChatPart } from "./types";

    type ToolChatPart = Extract<ChatPart, { type: "tool" }>;
    let { part }: { part: ToolChatPart } = $props();

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
              : part.state === "input"
                ? "pending"
                : undefined,
    );
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
        </Trace.CollapsibleContent>
    </Trace.Collapsible>
</Trace.Root>
