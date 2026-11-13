<script lang="ts">
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { ArrowUp02Icon, StopIcon } from "@hugeicons/core-free-icons";
    import { toast } from "svelte-sonner";
    import * as InputGroup from "$lib/components/ui/input-group/index.js";
    import * as Empty from "$lib/components/ui/empty/index.js";
    import { Button } from "$lib/components/ui/button/index.js";
    import { Spinner } from "$lib/components/ui/spinner/index.js";
    import { PI_PINNED_VERSION } from "$lib/agents/pi";
    import { ensureChat } from "./chat.svelte";
    import ChatHeader from "./ChatHeader.svelte";
    import ConfirmationDialog from "./ConfirmationDialog.svelte";
    import MessageList from "./MessageList.svelte";
    import QueuePanel from "./QueuePanel.svelte";

    let { sandbox, workspacePath }: { sandbox: string; workspacePath: string } = $props();

    // +layout.svelte keys the sandbox view by sandbox, so `sandbox` is fixed for this instance.
    // svelte-ignore state_referenced_locally
    const store = ensureChat(sandbox, workspacePath);
    let draft = $state("");
    let sending = $state(false);

    // Once per mount, not reactively: a stream that ends while the view is open shows Reconnect
    // rather than reattaching in a loop.
    if (store.connection === "idle" || store.connection === "disconnected") void store.attach();

    $effect(() => {
        if (store.editorText !== null) {
            draft = store.editorText;
            store.editorText = null;
        }
    });

    const running = $derived(store.chat.run === "running");
    const canWrite = $derived(store.connection === "attached" && store.writer);
    const canSend = $derived(canWrite && draft.trim() !== "" && !sending);
    const statusLine = $derived(Object.values(store.chat.statuses).join(" · "));
    const widgets = $derived(Object.entries(store.chat.widgets));

    async function send() {
        if (!canSend) return;
        const text = draft.trim();
        sending = true;
        try {
            await store.sendMessage(text);
            draft = "";
        } catch (e) {
            toast.error(e instanceof Error ? e.message : String(e));
        } finally {
            sending = false;
        }
    }

    async function stop() {
        try {
            await store.abort();
        } catch (e) {
            toast.error(e instanceof Error ? e.message : String(e));
        }
    }

    async function answer(id: string, payload: Record<string, unknown>) {
        try {
            await store.answerDialog(id, payload);
        } catch (e) {
            toast.error(e instanceof Error ? e.message : String(e));
        }
    }

    function onkeydown(event: KeyboardEvent) {
        if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
            event.preventDefault();
            void send();
        }
    }
</script>

<div
    class="flex h-full flex-col"
    data-agent-chat={sandbox}
    data-connection={store.connection}
    data-run={store.chat.run}
    data-writer={store.writer}
>
    {#if store.connection === "deployed"}
        <Empty.Root class="flex-1" data-chat-deployed>
            <Empty.Header>
                <Empty.Title>Chat is not available for a deployed sandbox</Empty.Title> <!-- TODO(i18n) -->
                <Empty.Description>
                    This sandbox runs on a remote host. Use the terminal or the agent web UI instead.
                </Empty.Description><!-- TODO(i18n) -->
            </Empty.Header>
        </Empty.Root>
    {:else if store.connection === "stopped"}
        <Empty.Root class="flex-1" data-chat-stopped>
            <Empty.Header>
                <Empty.Title>The agent is stopped</Empty.Title> <!-- TODO(i18n) -->
            </Empty.Header>
            <Empty.Content>
                <Button size="sm" onclick={() => store.attach()} data-chat-start-agent>Start agent</Button> <!-- TODO(i18n) -->
            </Empty.Content>
        </Empty.Root>
    {:else if store.connection === "attaching" && store.chat.messages.length === 0}
        <div class="flex flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
            <Spinner /> Starting the agent… <!-- TODO(i18n) -->
        </div>
    {:else if store.connection === "error"}
        <div class="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center" data-chat-error>
            <p class="text-sm">The chat could not connect to the agent.</p> <!-- TODO(i18n) -->
            <p class="max-w-lg text-xs break-words text-muted-foreground">{store.error}</p>
            <Button size="sm" onclick={() => store.attach()} data-chat-retry>Retry</Button> <!-- TODO(i18n) -->
        </div>
    {:else}
        <ChatHeader {store} />
        {#if store.connection === "disconnected"}
            <div class="flex items-center justify-between gap-2 border-b-2 bg-muted px-4 py-2 text-xs" data-chat-disconnected>
                <span>Disconnected from the agent.</span> <!-- TODO(i18n) -->
                <Button size="sm" variant="outline" onclick={() => store.attach()} data-chat-reconnect>Reconnect</Button> <!-- TODO(i18n) -->
            </div>
        {:else if store.connection === "attached" && !store.writer}
            <div class="flex items-center justify-between gap-2 border-b-2 bg-muted px-4 py-2 text-xs" data-chat-readonly>
                <span>Another window controls this agent. You can watch, but not send.</span> <!-- TODO(i18n) -->
                <Button size="sm" variant="outline" disabled={store.busy !== null} onclick={() => store.takeControl()} data-chat-take-control>
                    Take control
                </Button><!-- TODO(i18n) -->
            </div>
        {/if}
        {#if store.piOutdated}
            <div class="flex items-center justify-between gap-2 border-b-2 bg-muted px-4 py-2 text-xs" data-chat-outdated>
                <span>
                    This sandbox runs pi {store.host?.piVersion}; the chat needs {PI_PINNED_VERSION}. Some events
                    may not show.
                </span><!-- TODO(i18n) -->
                <Button size="sm" variant="outline" loading={store.busy === "update"} disabled={store.busy !== null} onclick={() => store.updatePi()} data-chat-update-pi>
                    Update pi
                </Button><!-- TODO(i18n) -->
            </div>
        {/if}
        {#if store.inferenceDown}
            <div class="flex items-center justify-between gap-2 border-b-2 bg-muted px-4 py-2 text-xs" data-chat-inference-down>
                <span>No local model is running, so pi cannot answer.</span> <!-- TODO(i18n) -->
                <Button size="sm" variant="outline" href="/models">Start a model</Button> <!-- TODO(i18n) -->
            </div>
        {/if}
        <div class="min-h-0 flex-1">
            {#if store.chat.messages.length === 0}
                <div class="flex h-full items-center justify-center p-6 text-sm text-muted-foreground" data-chat-empty>
                    Ask pi to work on this sandbox's code. <!-- TODO(i18n) -->
                </div>
            {:else}
                <MessageList messages={store.chat.messages} />
            {/if}
        </div>
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
                    value={draft}
                    oninput={(e) => (draft = e.currentTarget.value)}
                    {onkeydown}
                    placeholder={running ? "Message pi while it works…" : "Message pi…"}
                    disabled={!canWrite}
                    class="max-h-48 min-h-16"
                    data-chat-input
                /><!-- TODO(i18n) -->
                <InputGroup.Addon align="block-end" class="justify-between">
                    <div class="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
                        {#if running}
                            <InputGroup.Button
                                size="xs"
                                variant="ghost"
                                onclick={() => (store.deliveryMode = store.deliveryMode === "steer" ? "follow_up" : "steer")}
                                data-chat-delivery={store.deliveryMode}
                            >
                                {store.deliveryMode === "steer" ? "Steer now" : "Queue as follow-up"}
                            </InputGroup.Button><!-- TODO(i18n) -->
                        {/if}
                        {#if statusLine}<span class="truncate" data-chat-status>{statusLine}</span>{/if}
                    </div>
                    <div class="flex items-center gap-1">
                        {#if running}
                            <InputGroup.Button size="icon-sm" variant="outline" onclick={stop} aria-label="Stop" data-chat-stop>
                                <HugeiconsIcon icon={StopIcon} />
                            </InputGroup.Button><!-- TODO(i18n) -->
                        {/if}
                        <InputGroup.Button size="icon-sm" disabled={!canSend} onclick={send} aria-label="Send" data-chat-send>
                            <HugeiconsIcon icon={ArrowUp02Icon} />
                        </InputGroup.Button><!-- TODO(i18n) -->
                    </div>
                </InputGroup.Addon>
            </InputGroup.Root>
        </div>
        </div>
    {/if}
</div>
