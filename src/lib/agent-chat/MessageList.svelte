<script lang="ts">
    import * as MessageScroller from "$lib/components/ai-elements/message-scroller/index.js";
    import * as Message from "$lib/components/ai-elements/message/index.js";
    import * as Bubble from "$lib/components/ai-elements/bubble/index.js";
    import Markdown from "./Markdown.svelte";
    import ReasoningPart from "./ReasoningPart.svelte";
    import ToolPart from "./ToolPart.svelte";
    import type { ChatMessage, NoticeKind } from "./types";

    let { messages }: { messages: ChatMessage[] } = $props();

    const noticeClass = (kind: NoticeKind) =>
        kind === "error" ? "text-destructive text-xs" : "text-muted-foreground text-xs";
</script>

<MessageScroller.Provider autoScroll>
    <MessageScroller.Root>
        <MessageScroller.Viewport>
            <MessageScroller.Content class="mx-auto w-full max-w-3xl px-4 py-6">
                {#each messages as message (message.id)}
                    <MessageScroller.Item messageId={message.id} scrollAnchor={message.role === "user"}>
                        {#if message.role === "user"}
                            <Message.Root align="end" data-role="user">
                                <Bubble.Root variant="tinted" align="end">
                                    <Bubble.Content class="whitespace-pre-wrap">
                                        {message.parts[0]?.type === "text" ? message.parts[0].text : ""}
                                    </Bubble.Content>
                                </Bubble.Root>
                            </Message.Root>
                        {:else if message.role === "notice"}
                            {#each message.parts as part, i (i)}
                                {#if part.type === "notice"}
                                    <p class="text-center {noticeClass(part.kind)}" data-notice={part.kind}>
                                        {part.text}
                                    </p>
                                {/if}
                            {/each}
                        {:else}
                            <Message.Root align="start" data-role="assistant">
                                <Message.Content class="flex min-w-0 flex-col gap-3">
                                    {#each message.parts as part, i (i)}
                                        {#if part.type === "text"}
                                            <Markdown content={part.text} streaming={part.streaming} />
                                        {:else if part.type === "reasoning"}
                                            <ReasoningPart text={part.text} streaming={part.streaming} />
                                        {:else if part.type === "tool"}
                                            <ToolPart {part} />
                                        {:else if part.type === "notice"}
                                            <p class={noticeClass(part.kind)} data-notice={part.kind}>{part.text}</p>
                                        {/if}
                                    {/each}
                                </Message.Content>
                            </Message.Root>
                        {/if}
                    </MessageScroller.Item>
                {/each}
            </MessageScroller.Content>
        </MessageScroller.Viewport>
        <MessageScroller.Button />
    </MessageScroller.Root>
</MessageScroller.Provider>
