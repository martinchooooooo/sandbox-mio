<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { tick } from "svelte";
    import { pop } from "$lib/motion";
    import {
        tasks,
        activityEvents,
        badgeCount,
        markSeen,
        taskLabel,
    } from "$lib/activity/activity.svelte";
    import type { ActivityHeadline, TaskStatus } from "$lib/activity/types";

    // The global activity surface, mounted once in the root layout: a fixed bottom-right bell with a
    // badge (running + unseen failures) and an expandable panel listing active/recent tasks plus the
    // container/event scrollback. Selectors are data-attribute based (the e2e Selector Contract).

    // `headline` widens the bell into a status pill that spells the background work out, for
    // screens where the user would not think to look for a bell (onboarding).
    // `placement="inline"` leaves positioning to the caller and opens the panel downward, for a
    // toggle that sits in a top bar instead of the window's corner.
    let {
        headline = null,
        placement = "corner",
    }: {
        headline?: ActivityHeadline | null;
        placement?: "corner" | "inline";
    } = $props();

    const popupClass = $derived(
        placement === "inline" ? "absolute right-0 top-full mt-2 origin-top-right" : "origin-bottom-right",
    );

    let open = $state(false);
    let hintDismissed = $state(false);
    const showHint = $derived(!!headline?.hint && !hintDismissed && !open);
    let logEl = $state<HTMLDivElement>();

    const count = $derived(badgeCount());
    const taskList = $derived(tasks());
    const events = $derived(activityEvents());

    async function togglePanel() {
        open = !open;
        hintDismissed = true;
        if (open) {
            markSeen();
            await tick();
            logEl?.scrollTo({ top: logEl.scrollHeight });
        }
    }

    function statusIcon(status: TaskStatus): string {
        switch (status) {
            case "succeeded": return "✓";
            case "failed": return "✗";
            default: return "◐";
        }
    }
    function statusClass(status: TaskStatus): string {
        switch (status) {
            case "succeeded": return "text-emerald-600 dark:text-emerald-400";
            case "failed": return "text-destructive";
            default: return "text-blue-600 dark:text-blue-400";
        }
    }
    function pct(progress: number | null): number | null {
        return progress === null ? null : Math.round(progress * 100);
    }
</script>

<div class={placement === "inline" ? "relative z-50" : "fixed bottom-4 right-4 z-50 flex flex-col items-end gap-2"}>
    {#if open}
        <div
            data-activity-panel
            in:pop
            out:pop
            class={[popupClass, "w-96 max-w-[calc(100vw-2rem)] rounded-lg border bg-background shadow-lg flex flex-col overflow-hidden"]}
        >
            <div class="flex items-center justify-between px-4 py-2 border-b text-sm font-medium shrink-0">
                <span>{m.activity_title()}</span>
                <button onclick={() => (open = false)} class="text-muted-foreground hover:text-foreground leading-none">✕</button>
            </div>

            {#if headline}
                <div data-activity-headline-detail class="border-b px-4 py-3 text-sm space-y-2">
                    <p class="font-medium">{headline.title}</p>
                    {#if headline.detail}<p class="text-muted-foreground">{headline.detail}</p>{/if}
                    {#if headline.action}
                        {@const action = headline.action}
                        <Button size="sm" onclick={() => { open = false; action.onclick(); }}>{action.label}</Button>
                    {/if}
                </div>
            {/if}

            <!-- Active/recent tasks -->
            <div class="max-h-64 overflow-y-auto p-3 space-y-2 border-b">
                {#if taskList.length === 0}
                    <p class="text-xs text-muted-foreground">{m.activity_empty()}</p>
                {:else}
                    {#each taskList as task (task.id)}
                        <div data-activity-task={task.id} data-task-status={task.status} class="text-sm">
                            <div class="flex items-center gap-2">
                                <span class={statusClass(task.status) + " shrink-0 w-4 text-center"}>{statusIcon(task.status)}</span>
                                <span class="font-medium truncate">{taskLabel(task.kind)}</span>
                                {#if task.target}<span class="text-muted-foreground text-xs truncate">{task.target}</span>{/if}
                            </div>
                            {#if task.status === "running"}
                                <div class="mt-1 ml-6 h-1.5 rounded bg-muted overflow-hidden">
                                    {#if pct(task.progress) !== null}
                                        <div class="h-full bg-blue-600 dark:bg-blue-400 transition-all" style={`width: ${pct(task.progress)}%`}></div>
                                    {:else}
                                        <div class="h-full w-1/3 bg-blue-600/60 dark:bg-blue-400/60 animate-pulse"></div>
                                    {/if}
                                </div>
                            {/if}
                            {#if task.detail}
                                <p data-task-detail class="ml-6 mt-0.5 text-xs text-foreground/80 truncate">{task.detail}</p>
                            {:else if task.error}
                                <p data-task-detail class="ml-6 mt-0.5 text-xs text-destructive truncate">{task.error}</p>
                            {/if}
                        </div>
                    {/each}
                {/if}
            </div>

            <!-- Event scrollback -->
            <div class="px-4 py-1.5 text-xs text-muted-foreground font-medium shrink-0">{m.activity_events_title()}</div>
            <div bind:this={logEl} data-activity-events class="max-h-40 overflow-y-auto px-4 pb-2 font-mono text-xs space-y-0.5">
                {#if events.length === 0}
                    <p class="text-muted-foreground">{m.event_log_waiting()}</p>
                {:else}
                    {#each events as ev (ev.id)}
                        <div class={ev.level === "error" ? "text-destructive" : ""}>{ev.text}</div>
                    {/each}
                {/if}
            </div>
        </div>
    {/if}

    {#if showHint}
        <div
            data-activity-hint
            in:pop
            out:pop
            class={[popupClass, "w-72 max-w-[calc(100vw-2rem)] rounded-md border-2 bg-accent text-accent-foreground shadow-md px-3 py-2 pr-7 text-sm", placement === "corner" && "relative"]}
        >
            {headline?.hint}
            <!-- TODO(i18n) -->
            <button
                onclick={() => (hintDismissed = true)}
                class="absolute right-2 top-1.5 leading-none opacity-70 hover:opacity-100"
                aria-label="Dismiss"
            >✕</button>
        </div>
    {/if}

    {#if headline}
        <button
            data-activity-toggle
            data-activity-headline={headline.state}
            title={m.activity_badge_tooltip()}
            onclick={togglePanel}
            class="flex max-w-[calc(100vw-2rem)] cursor-pointer items-center gap-3 rounded-md border-2 bg-background px-3 py-2 text-left shadow-md transition-all hover:translate-y-1 hover:shadow-none"
        >
            <span class="relative flex size-6 shrink-0 items-center justify-center">
                {#if headline.state === "running"}
                    <span class="absolute inset-0 rounded-full border-2 border-muted"></span>
                    <span class="absolute inset-0 animate-spin rounded-full border-2 border-transparent border-t-primary"></span>
                {:else if headline.state === "done"}
                    <span class="text-lg leading-none text-emerald-600 dark:text-emerald-400">✓</span>
                {:else}
                    <span class="font-head text-lg leading-none text-destructive">!</span>
                {/if}
            </span>
            <span class="min-w-0">
                <span class="block truncate text-sm font-medium">{headline.title}</span>
                {#if headline.detail}
                    <span class="block truncate text-xs text-muted-foreground">{headline.detail}</span>
                {/if}
            </span>
            <span class="shrink-0">🔔</span>
        </button>
    {:else}
        <Button
            data-activity-toggle
            variant="outline"
            size="sm"
            title={m.activity_badge_tooltip()}
            onclick={togglePanel}
        >
            🔔
            {#if count > 0}
                <span data-activity-badge class="ml-1 inline-flex items-center justify-center rounded-full bg-destructive text-destructive-foreground text-xs min-w-5 h-5 px-1">{count}</span>
            {/if}
        </Button>
    {/if}
</div>
