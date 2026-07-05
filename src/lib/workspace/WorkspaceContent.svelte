<script lang="ts">
    import WorkspaceSwitcher from "$lib/workspace/WorkspaceSwitcher.svelte";
    import WorkspaceDashboard from "$lib/workspace/WorkspaceDashboard.svelte";

    let { hasTabs, dashboardSandbox }: { hasTabs: boolean; dashboardSandbox: string | null } = $props();
</script>

<div class="flex flex-1 flex-col overflow-hidden">
    {#if hasTabs}
        <WorkspaceSwitcher />
    {/if}
    <div class="flex-1 overflow-hidden">
        {#if dashboardSandbox}
            <WorkspaceDashboard sandboxName={dashboardSandbox} />
        {/if}
    </div>
</div>
