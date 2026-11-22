<script lang="ts">
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { ArrowUp02Icon, StopIcon } from "@hugeicons/core-free-icons";
    import { toast } from "svelte-sonner";
    import * as InputGroup from "$lib/components/ui/input-group/index.js";
    import { m } from "$lib/paraglide/messages.js";
    import { parseError } from "$lib/errors";
    import type { ChatStore } from "./chat.svelte";
    import ConfirmationDialog from "./ConfirmationDialog.svelte";
    import QueuePanel from "./QueuePanel.svelte";

    let { store }: { store: ChatStore } = $props();

    let sending = $state(false);

    const running = $derived(store.chat.run === "running");
    const canWrite = $derived(store.connection === "attached" && store.writer);
    const canSend = $derived(canWrite && store.draft.trim() !== "" && !sending);
    const statusLine = $derived(Object.values(store.chat.statuses).join(" · "));
    const widgets = $derived(Object.entries(store.chat.widgets));

    async function send() {
        if (!canSend) return;
        const text = store.draft.trim();
        sending = true;
        try {
            await store.sendMessage(text);
            store.draft = "";
        } catch (e) {
            toast.error(parseError(e).message);
        } finally {
            sending = false;
        }
    }

    async function stop() {
        try {
            await store.abort();
        } catch (e) {
            toast.error(parseError(e).message);
        }
    }

    async function answer(id: string, payload: Record<string, unknown>) {
        try {
            await store.answerDialog(id, payload);
        } catch (e) {
            toast.error(parseError(e).message);
        }
    }

    function onkeydown(event: KeyboardEvent) {
        if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
            event.preventDefault();
            void send();
        }
    }
</script>

<!-- The right gutter keeps the composer clear of the activity bell fixed in the window corner. -->
<div class="pr-16 pl-4 pb-4">
    <div class="mx-auto flex w-full max-w-3xl flex-col gap-2">
        {#each store.chat.dialogs as dialog (dialog.id)}
            <ConfirmationDialog {dialog} disabled={!canWrite} answer={(payload) => answer(dialog.id, payload)} />
        {/each}
        <QueuePanel queue={store.chat.queue} />
        {#each widgets as [key, lines] (key)}
            <pre class="rounded-md border-2 bg-card px-3 py-2 text-xs whitespace-pre-wrap" data-chat-widget={key}>{lines.join("\n")}</pre>
        {/each}
        <InputGroup.Root>
            <InputGroup.Textarea
                value={store.draft}
                oninput={(e) => (store.draft = e.currentTarget.value)}
                {onkeydown}
                placeholder={running ? m.agent_chat_placeholder_running() : m.agent_chat_placeholder()}
                disabled={!canWrite}
                class="max-h-48 min-h-16"
                data-chat-input
            />
            <InputGroup.Addon align="block-end" class="justify-between">
                <div class="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
                    {#if running}
                        <InputGroup.Button
                            size="xs"
                            variant="ghost"
                            onclick={() => (store.deliveryMode = store.deliveryMode === "steer" ? "follow_up" : "steer")}
                            data-chat-delivery={store.deliveryMode}
                        >
                            {store.deliveryMode === "steer" ? m.agent_chat_delivery_steer() : m.agent_chat_delivery_follow_up()}
                        </InputGroup.Button>
                    {/if}
                    {#if statusLine}<span class="truncate" data-chat-status>{statusLine}</span>{/if}
                </div>
                <div class="flex items-center gap-1">
                    {#if running}
                        <InputGroup.Button size="icon-sm" variant="outline" onclick={stop} aria-label={m.agent_chat_stop()} data-chat-stop>
                            <HugeiconsIcon icon={StopIcon} />
                        </InputGroup.Button>
                    {/if}
                    <InputGroup.Button size="icon-sm" disabled={!canSend} onclick={send} aria-label={m.agent_chat_send()} data-chat-send>
                        <HugeiconsIcon icon={ArrowUp02Icon} />
                    </InputGroup.Button>
                </div>
            </InputGroup.Addon>
        </InputGroup.Root>
    </div>
</div>
