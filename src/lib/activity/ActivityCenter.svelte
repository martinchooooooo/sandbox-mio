<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { tick } from "svelte";
    import {
        tasks,
        activityEvents,
        badgeCount,
        markSeen,
        taskLabel,
    } from "$lib/activity/activity.svelte";
    import type { TaskStatus } from "$lib/activity/types";

    // The global activity surface, mounted once in the root layout: a fixed bottom-right bell with a
    // badge (running + unseen failures) and an expandable panel listing active/recent tasks plus the
    // container/event scrollback. Selectors are data-attribute based (the e2e Selector Contract).

    let open = $state(false);
    let logEl = $state<HTMLDivElement>();

    const count = $derived(badgeCount());
    const taskList = $derived(tasks());
    const events = $derived(activityEvents());

    async function togglePanel() {
        open = !open;
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

<div class="fixed bottom-4 right-4 z-50 flex flex-col items-end gap-2">
    {#if open}
        <div
            data-activity-panel
            class="w-96 max-w-[calc(100vw-2rem)] rounded-lg border bg-background shadow-lg flex flex-col overflow-hidden"
        >
            <div class="flex items-center justify-between px-4 py-2 border-b text-sm font-medium shrink-0">
                <span>{m.activity_title()}</span>
                <button onclick={() => (open = false)} class="text-muted-foreground hover:text-foreground leading-none">✕</button>
            </div>

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
</div>
