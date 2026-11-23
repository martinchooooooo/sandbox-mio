<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Textarea } from "$lib/components/ui/textarea";
    import { Message, MessageContent } from "$lib/components/ai-elements/message";
    import { Bubble, BubbleContent } from "$lib/components/ai-elements/bubble";
    import { m } from "$lib/paraglide/messages.js";
    import { parseError } from "$lib/errors";
    import ModelAvatar from "./ModelAvatar.svelte";
    import { modelsState } from "./model.svelte";
    import { PAGE } from "./copy";

    let prompt = $state("");
    let reply = $state("");
    let chatting = $state(false);

    async function send() {
        const { baseUrl, modelId } = modelsState().status;
        if (!baseUrl || !modelId || !prompt.trim()) return;
        chatting = true;
        reply = "";
        try {
            const res = await fetch(`${baseUrl}/chat/completions`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ model: modelId, messages: [{ role: "user", content: prompt }], max_tokens: 256 }),
            });
            const body = await res.json();
            reply = body?.choices?.[0]?.message?.content?.trim() || m.models_test_empty();
        } catch (e) {
            reply = parseError(e).message;
        }
        chatting = false;
    }
</script>

<div class="flex flex-col gap-2" data-chat>
    <Textarea
        value={prompt}
        oninput={(e) => (prompt = e.currentTarget.value)}
        placeholder={m.models_test_placeholder()}
        rows={2}
        aria-label={PAGE.tryIt}
    />
    <div class="flex items-center gap-3">
        <Button size="sm" loading={chatting} disabled={chatting} onclick={send}>{m.models_test_send()}</Button>
        {#if chatting}<span class="text-xs text-muted-foreground">{m.models_test_thinking()}</span>{/if}
    </div>
    {#if reply}
        <Message>
            <ModelAvatar size="sm" />
            <MessageContent>
                <Bubble variant="muted"><BubbleContent><span class="whitespace-pre-wrap" data-reply>{reply}</span></BubbleContent></Bubble>
            </MessageContent>
        </Message>
    {/if}
</div>
