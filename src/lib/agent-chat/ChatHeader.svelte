<script lang="ts">
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { MoreHorizontalIcon, ArrowDown01Icon, Clock01Icon } from "@hugeicons/core-free-icons";
    import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
    import * as HoverCard from "$lib/components/ui/hover-card/index.js";
    import { Progress } from "$lib/components/ui/progress/index.js";
    import { Button } from "$lib/components/ui/button/index.js";
    import { Switch } from "$lib/components/ui/switch/index.js";
    import { Label } from "$lib/components/ui/label/index.js";
    import type { ChatStore } from "./chat.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { LOCAL_LLM_PROVIDER_ID } from "$lib/agents/local-llm";

    let { store, onOpenAnatomy }: { store: ChatStore; onOpenAnatomy?: () => void } = $props();

    const model = $derived(store.piState?.model as { id?: string; name?: string; provider?: string; reasoning?: boolean } | null | undefined);
    const thinkingLevel = $derived(String(store.piState?.thinkingLevel ?? "off"));
    const context = $derived(store.contextUse);
    const percent = $derived(context ? Math.min(100, Math.round((context.used / context.window) * 100)) : 0);
    const disabled = $derived(store.connection !== "attached" || !store.writer || store.chat.run === "running");

    /** The local provider holds the one served model, so it is labelled by role, not by name. */
    function modelLabel(entry: { name?: string; provider?: string }): string {
        return entry.provider === LOCAL_LLM_PROVIDER_ID ? m.agent_chat_local_llm() : (entry.name ?? "");
    }

    function sessionLabel(s: (typeof store.sessions)[number]): string {
        return s.name ?? s.firstMessage ?? m.agent_chat_session_empty();
    }
</script>

<div class="flex items-center gap-2 border-b-2 px-4 py-2" data-chat-header>
    <DropdownMenu.Root onOpenChange={(open: boolean) => open && store.loadModels()}>
        <DropdownMenu.Trigger>
            {#snippet child({ props })}
                <Button {...props} size="sm" variant="outline" {disabled} data-chat-model={model?.id ?? ""}>
                    {model ? modelLabel(model) : m.agent_chat_no_model()}
                    <HugeiconsIcon icon={ArrowDown01Icon} />
                </Button>
            {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="start">
            <DropdownMenu.RadioGroup
                value={model ? `${model.provider}/${model.id}` : ""}
                onValueChange={(v) => {
                    const next = store.selectableModels.find((entry) => `${entry.provider}/${entry.id}` === v);
                    if (next) void store.setModel(next);
                }}
            >
                {#each store.selectableModels as entry (`${entry.provider}/${entry.id}`)}
                    <DropdownMenu.RadioItem value={`${entry.provider}/${entry.id}`} data-model-option={entry.id} title={entry.id}>{modelLabel(entry)}</DropdownMenu.RadioItem>
                {/each}
            </DropdownMenu.RadioGroup>
        </DropdownMenu.Content>
    </DropdownMenu.Root>

    {#if model?.reasoning}
        <DropdownMenu.Root>
            <DropdownMenu.Trigger>
                {#snippet child({ props })}
                    <Button {...props} size="sm" variant="ghost" {disabled} data-chat-thinking={thinkingLevel}>
                        {m.agent_chat_thinking({ level: thinkingLevel })}
                    </Button>
                {/snippet}
            </DropdownMenu.Trigger>
            <DropdownMenu.Content align="start">
                <DropdownMenu.RadioGroup value={thinkingLevel} onValueChange={(v) => store.setThinkingLevel(v)}>
                    {#each store.thinkingLevels as level (level)}
                        <DropdownMenu.RadioItem value={level} data-thinking-option={level}>{level}</DropdownMenu.RadioItem>
                    {/each}
                </DropdownMenu.RadioGroup>
            </DropdownMenu.Content>
        </DropdownMenu.Root>
    {/if}

    <div class="flex-1"></div>

    <div class="flex items-center gap-2" data-chat-learning={store.learning}>
        <Switch
            id="chat-learning-mode"
            checked={store.learning}
            onCheckedChange={(on) => store.setLearning(on)}
        />
        <Label for="chat-learning-mode" class="text-xs whitespace-nowrap">Show the Brain and Arms</Label> <!-- TODO(i18n) -->
    </div>

    {#if onOpenAnatomy}
        <Button size="sm" variant="outline" onclick={onOpenAnatomy} data-chat-open-anatomy>Brain and Arms</Button> <!-- TODO(i18n) -->
    {/if}

    {#if context}
        <HoverCard.Root>
            <HoverCard.Trigger class="flex w-24 items-center gap-2 text-xs text-muted-foreground" data-chat-context={percent}>
                <Progress value={percent} class="h-2" />
                {percent}%
            </HoverCard.Trigger>
            <HoverCard.Content class="w-64 text-xs">
                <p class="font-medium">{m.agent_chat_context_title()}</p>
                <p class="text-muted-foreground">
                    {m.agent_chat_context_usage({ used: context.used.toLocaleString(), window: context.window.toLocaleString() })}
                </p>
            </HoverCard.Content>
        </HoverCard.Root>
    {/if}

    <DropdownMenu.Root onOpenChange={(open: boolean) => open && store.loadSessions()}>
        <DropdownMenu.Trigger>
            {#snippet child({ props })}
                <Button {...props} size="icon" variant="ghost" aria-label={m.agent_chat_sessions()} data-chat-sessions>
                    <HugeiconsIcon icon={Clock01Icon} />
                </Button>
            {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end" class="max-h-96 w-72 overflow-y-auto">
            <DropdownMenu.Item {disabled} onSelect={() => store.newSession()} data-chat-new-session>
                {m.agent_chat_new_session()}
            </DropdownMenu.Item>
            {#if store.sessions.length > 0}
                <DropdownMenu.Separator />
                {#each store.sessions as session (session.id)}
                    <DropdownMenu.Item
                        {disabled}
                        onSelect={() => store.switchSession(session)}
                        data-session-id={session.id}
                    >
                        <div class="flex min-w-0 flex-col">
                            <span class="truncate">{sessionLabel(session)}</span>
                            <span class="text-xs text-muted-foreground">
                                {m.agent_chat_session_meta({ date: new Date(session.modifiedMs).toLocaleString(), count: session.messageCount })}
                            </span>
                        </div>
                    </DropdownMenu.Item>
                {/each}
            {/if}
        </DropdownMenu.Content>
    </DropdownMenu.Root>

    <DropdownMenu.Root>
        <DropdownMenu.Trigger>
            {#snippet child({ props })}
                <Button {...props} size="icon" variant="ghost" aria-label={m.agent_chat_menu()} data-chat-menu>
                    <HugeiconsIcon icon={MoreHorizontalIcon} />
                </Button>
            {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end">
            <DropdownMenu.Item
                disabled={store.connection !== "attached" || store.busy !== null}
                onSelect={() => store.stopAgent()}
                data-chat-stop-agent
            >
                {m.agent_chat_stop_agent()}
            </DropdownMenu.Item>
        </DropdownMenu.Content>
    </DropdownMenu.Root>
</div>
