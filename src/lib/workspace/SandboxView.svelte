<script lang="ts">
    import WorkspaceDashboard from "$lib/workspace/WorkspaceDashboard.svelte";
    import ChatView from "$lib/agent-chat/ChatView.svelte";
    import { pendingOp } from "$lib/sandbox/lifecycle.svelte";
    import { activePane } from "$lib/workspace/state.svelte";

    let { sandbox }: { sandbox: string } = $props();

    // The dashboard shows only once Rust reports no active pane (docs/features/consolidate-app-container/
    // 02_dev.md, Resolved Decision 2). A webview pane and the layout-level terminal layer draw over
    // this whole region; the native chat renders here.
    const pane = $derived(activePane());
</script>

<div
    data-sandbox-view={sandbox}
    data-pending={pendingOp(sandbox)}
    class="animate-fade-in min-h-0 min-w-0 flex-1 overflow-clip"
>
    {#if pane === null}
        <WorkspaceDashboard sandboxName={sandbox} />
    {:else if pane.kind === "agentchat"}
        <ChatView sandbox={pane.sandbox} workspacePath={pane.workspace_path} />
    {/if}
</div>
