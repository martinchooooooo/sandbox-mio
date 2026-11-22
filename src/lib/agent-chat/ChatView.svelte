<script lang="ts">
    import * as Empty from "$lib/components/ui/empty/index.js";
    import { Button } from "$lib/components/ui/button/index.js";
    import * as Dialog from "$lib/components/ui/dialog/index.js";
    import { Spinner } from "$lib/components/ui/spinner/index.js";
    import { PI_PINNED_VERSION } from "$lib/agents/pi";
    import { m } from "$lib/paraglide/messages.js";
    import { ensureChat } from "./chat.svelte";
    import AnatomyPanel from "./AnatomyPanel.svelte";
    import ChatHeader from "./ChatHeader.svelte";
    import Composer from "./Composer.svelte";
    import MessageList from "./MessageList.svelte";

    let { sandbox, workspacePath }: { sandbox: string; workspacePath: string } = $props();

    // +layout.svelte keys the sandbox view by sandbox, so `sandbox` is fixed for this instance.
    // svelte-ignore state_referenced_locally
    const store = ensureChat(sandbox, workspacePath);

    const PANEL_COLUMN_MIN_WIDTH = 896;
    const PANEL_WIDTH = "w-[22rem]";
    let paneWidth = $state(0);
    let drawerOpen = $state(false);
    const panelMode = $derived(
        !store.learning || paneWidth === 0 ? null : paneWidth >= PANEL_COLUMN_MIN_WIDTH ? "column" : "drawer",
    );
    $effect(() => {
        if (panelMode !== "drawer") drawerOpen = false;
    });

    // Once per mount, not reactively: a stream that ends while the view is open shows Reconnect
    // rather than reattaching in a loop.
    if (store.connection === "idle" || store.connection === "disconnected") void store.attach();
</script>

<div
    class="flex h-full flex-col"
    bind:clientWidth={paneWidth}
    data-agent-chat={sandbox}
    data-connection={store.connection}
    data-run={store.chat.run}
    data-writer={store.writer}
    data-learning={store.learning}
>
    {#if store.connection === "deployed"}
        <Empty.Root class="flex-1" data-chat-deployed>
            <Empty.Header>
                <Empty.Title>{m.agent_chat_deployed_title()}</Empty.Title>
                <Empty.Description>{m.agent_chat_deployed_description()}</Empty.Description>
            </Empty.Header>
        </Empty.Root>
    {:else if store.connection === "stopped"}
        <Empty.Root class="flex-1" data-chat-stopped>
            <Empty.Header>
                <Empty.Title>{m.agent_chat_stopped_title()}</Empty.Title>
            </Empty.Header>
            <Empty.Content>
                <Button size="sm" onclick={() => store.attach()} data-chat-start-agent>{m.agent_chat_start_agent()}</Button>
            </Empty.Content>
        </Empty.Root>
    {:else if store.connection === "attaching" && store.chat.messages.length === 0}
        <div class="flex flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
            <Spinner /> {m.agent_chat_starting()}
        </div>
    {:else if store.connection === "error"}
        <div class="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center" data-chat-error>
            <p class="text-sm">{m.agent_chat_error_title()}</p>
            <p class="max-w-lg text-xs break-words text-muted-foreground">{store.error}</p>
            <Button size="sm" onclick={() => store.attach()} data-chat-retry>{m.agent_chat_retry()}</Button>
        </div>
    {:else}
        <div class="flex min-h-0 flex-1">
            {#if panelMode === "column"}
                <aside class="{PANEL_WIDTH} shrink-0 border-r-2" data-anatomy-column>
                    <AnatomyPanel {store} />
                </aside>
            {/if}
            <div class="flex min-w-0 flex-1 flex-col">
                <ChatHeader {store} onOpenAnatomy={panelMode === "drawer" ? () => (drawerOpen = true) : undefined} />
                {#if store.connection === "disconnected"}
                    <div class="flex items-center justify-between gap-2 border-b-2 bg-muted px-4 py-2 text-xs" data-chat-disconnected>
                        <span>{m.agent_chat_disconnected()}</span>
                        <Button size="sm" variant="outline" onclick={() => store.attach()} data-chat-reconnect>{m.agent_chat_reconnect()}</Button>
                    </div>
                {:else if store.connection === "attached" && !store.writer}
                    <div class="flex items-center justify-between gap-2 border-b-2 bg-muted px-4 py-2 text-xs" data-chat-readonly>
                        <span>{m.agent_chat_readonly()}</span>
                        <Button size="sm" variant="outline" disabled={store.busy !== null} onclick={() => store.takeControl()} data-chat-take-control>
                            {m.agent_chat_take_control()}
                        </Button>
                    </div>
                {/if}
                {#if store.piOutdated}
                    <div class="flex items-center justify-between gap-2 border-b-2 bg-muted px-4 py-2 text-xs" data-chat-outdated>
                        <span>{m.agent_chat_outdated({ version: store.host?.piVersion ?? "", pinned: PI_PINNED_VERSION })}</span>
                        <Button size="sm" variant="outline" loading={store.busy === "update"} disabled={store.busy !== null} onclick={() => store.updatePi()} data-chat-update-pi>
                            {m.agent_chat_update_pi()}
                        </Button>
                    </div>
                {/if}
                {#if store.inferenceDown}
                    <div class="flex items-center justify-between gap-2 border-b-2 bg-muted px-4 py-2 text-xs" data-chat-inference-down>
                        <span>{m.agent_chat_inference_down()}</span>
                        <Button size="sm" variant="outline" href="/models">{m.agent_chat_start_model()}</Button>
                    </div>
                {/if}
                <div class="min-h-0 flex-1">
                    {#if store.chat.messages.length === 0}
                        <div class="flex h-full items-center justify-center p-6 text-sm text-muted-foreground" data-chat-empty>
                            {m.agent_chat_empty()}
                        </div>
                    {:else}
                        <MessageList {store} />
                    {/if}
                </div>
                <Composer {store} />
            </div>
        </div>
        {#if panelMode === "drawer"}
            <Dialog.Root bind:open={drawerOpen}>
                <Dialog.Content
                    class="top-0 left-0 h-full max-h-none {PANEL_WIDTH} max-w-[calc(100%-2rem)] translate-x-0 translate-y-0 gap-0 rounded-none p-0 sm:max-w-[22rem]"
                    data-anatomy-sheet
                >
                    <Dialog.Title class="sr-only">Brain and Arms</Dialog.Title> <!-- TODO(i18n) -->
                    <AnatomyPanel {store} />
                </Dialog.Content>
            </Dialog.Root>
        {/if}
    {/if}
</div>
