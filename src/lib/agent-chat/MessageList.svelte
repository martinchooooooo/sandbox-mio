<script lang="ts">
    import * as MessageScroller from "$lib/components/ai-elements/message-scroller/index.js";
    import * as Message from "$lib/components/ai-elements/message/index.js";
    import * as Bubble from "$lib/components/ai-elements/bubble/index.js";
    import Markdown from "./Markdown.svelte";
    import ReasoningPart from "./ReasoningPart.svelte";
    import RoleFrame from "./RoleFrame.svelte";
    import ToolPart from "./ToolPart.svelte";
    import type { Snippet } from "svelte";
    import type { ChatStore } from "./chat.svelte";
    import type { Role } from "./anatomy";
    import type { NoticeKind } from "./types";

    let { store }: { store: ChatStore } = $props();

    const messages = $derived(store.chat.messages);
    const learning = $derived(store.learning);

    function selectedRole(messageId: string, index: number): Role | null {
        const sel = store.selected;
        return sel && sel.messageId === messageId && sel.partIndex === index ? sel.role : null;
    }
    const select = (messageId: string, partIndex: number, role: Role) =>
        (store.selected = selectedRole(messageId, partIndex) === role ? null : { messageId, partIndex, role });

    const noticeClass = (kind: NoticeKind) =>
        kind === "error" ? "text-destructive text-xs" : "text-muted-foreground text-xs";
</script>

{#snippet framed(messageId: string, index: number, role: Role, body: Snippet)}
    {#if learning}
        <RoleFrame {role} selected={selectedRole(messageId, index) === role} onselect={() => select(messageId, index, role)}>
            {@render body()}
        </RoleFrame>
    {:else}
        {@render body()}
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
                                            {@render framed(message.id, i, "brain-reply", reply)}
                                        {:else if part.type === "reasoning"}
                                            {#snippet think()}
                                                <ReasoningPart
                                                    text={part.text}
                                                    streaming={part.streaming}
                                                    open={learning ? selectedRole(message.id, i) === "brain-think" : undefined}
                                                />
                                            {/snippet}
                                            {@render framed(message.id, i, "brain-think", think)}
                                        {:else if part.type === "tool"}
                                            <ToolPart
                                                {part}
                                                {learning}
                                                settled={store.chat.run === "idle"}
                                                selectedRole={selectedRole(message.id, i)}
                                                onselect={(role) => select(message.id, i, role)}
                                            />
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
