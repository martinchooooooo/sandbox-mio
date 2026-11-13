<script lang="ts">
    import SandboxHeader from "$lib/workspace/SandboxHeader.svelte";
    import WorkspaceDashboard from "$lib/workspace/WorkspaceDashboard.svelte";
    import ChatView from "$lib/agent-chat/ChatView.svelte";
    import { activePane } from "$lib/workspace/state.svelte";

    let { sandbox }: { sandbox: string } = $props();

    // Manage shows only once Rust reports no active pane (docs/features/consolidate-app-container/
    // 02_dev.md, Resolved Decision 2). A webview pane draws itself over the empty region below the
    // header; the native chat has no webview, so it renders here.
    const pane = $derived(activePane());
</script>

<div class="animate-fade-in flex min-w-0 flex-1 flex-col overflow-hidden">
    <SandboxHeader {sandbox} />
    <div class="min-h-0 flex-1 overflow-hidden">
        {#if pane === null}
            <WorkspaceDashboard sandboxName={sandbox} />
        {:else if pane.kind === "agentchat"}
            <ChatView sandbox={pane.sandbox} workspacePath={pane.workspace_path} />
        {/if}
    </div>
</div>
