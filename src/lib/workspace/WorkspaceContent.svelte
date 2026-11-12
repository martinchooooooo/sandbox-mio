<script lang="ts">
    import WorkspaceSwitcher from "$lib/workspace/WorkspaceSwitcher.svelte";
    import WorkspaceDashboard from "$lib/workspace/WorkspaceDashboard.svelte";
    import ChatView from "$lib/agent-chat/ChatView.svelte";
    import type { AppPane } from "$lib/workspace/state.svelte";

    let {
        hasTabs,
        dashboardSandbox,
        chatPane,
    }: { hasTabs: boolean; dashboardSandbox: string | null; chatPane: AppPane | null } = $props();
</script>

<div class="flex flex-1 flex-col overflow-hidden">
    {#if hasTabs}
        <WorkspaceSwitcher />
    {/if}
    <div class="flex-1 overflow-hidden">
        {#if dashboardSandbox}
            {#key dashboardSandbox}
                <WorkspaceDashboard sandboxName={dashboardSandbox} />
            {/key}
        {:else if chatPane}
            {#key chatPane.sandbox}
                <ChatView sandbox={chatPane.sandbox} workspacePath={chatPane.workspace_path} />
            {/key}
        {/if}
    </div>
</div>
