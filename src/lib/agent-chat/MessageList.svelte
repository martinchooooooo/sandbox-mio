<script lang="ts">
    import * as MessageScroller from "$lib/components/ai-elements/message-scroller/index.js";
    import * as Message from "$lib/components/ai-elements/message/index.js";
    import * as Bubble from "$lib/components/ai-elements/bubble/index.js";
    import Markdown from "./Markdown.svelte";
    import ReasoningPart from "./ReasoningPart.svelte";
    import AgentRow from "./AgentRow.svelte";
    import ToolPart from "./ToolPart.svelte";
    import type { Snippet } from "svelte";
    import type { ChatStore } from "./chat.svelte";
    import { avatarLabel } from "./anatomy-copy";
    import { frameOf, isLive, type Role } from "./anatomy";
    import type { ChatPart, NoticeKind, RunState } from "./types";

    let { store }: { store: ChatStore } = $props();

    const messages = $derived(store.chat.messages);
    const learning = $derived(store.learning);
    // Only the newest assistant message can hold an in-progress part: pi runs a message's tools
    // before the next assistant message starts.
    const liveId = $derived(messages.findLast((m) => m.role === "assistant")?.id);
    const runFor = (id: string): RunState => (id === liveId ? store.chat.run : "idle");

    const noticeClass = (kind: NoticeKind) =>
        kind === "error" ? "text-destructive text-xs" : "text-muted-foreground text-xs";
</script>

{#snippet partRow(part: ChatPart, run: RunState, role: Role, body: Snippet)}
    {#if !learning}
        {@render body()}
    {:else}
        {@const frame = frameOf(part, run)}
        {@const live = isLive(part, run)}
        <AgentRow {role} {frame} {live} label={avatarLabel(role, frame, "", live)}>{@render body()}</AgentRow>
    {/if}
{/snippet}

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
                                            {#snippet reply()}
                                                <Markdown content={part.text} streaming={part.streaming} />
                                            {/snippet}
                                            {@render partRow(part, runFor(message.id), "brain-reply", reply)}
                                        {:else if part.type === "reasoning"}
                                            {#snippet think()}
                                                <ReasoningPart text={part.text} streaming={part.streaming} />
                                            {/snippet}
                                            {@render partRow(part, runFor(message.id), "brain-think", think)}
                                        {:else if part.type === "tool"}
                                            <ToolPart {part} {learning} run={runFor(message.id)} />
                                        {:else if part.type === "notice"}
                                            {#snippet notice()}
                                                <p class={noticeClass(part.kind)} data-notice={part.kind}>{part.text}</p>
                                            {/snippet}
                                            {#if learning}
                                                <AgentRow>{@render notice()}</AgentRow>
                                            {:else}
                                                {@render notice()}
                                            {/if}
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
