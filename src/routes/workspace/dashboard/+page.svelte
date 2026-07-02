<script lang="ts">
    import { page } from "$app/state";
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { createActionRunner } from "$lib/actions.svelte";
    import StatusRow from "../../projects/StatusRow.svelte";
    import PackageManager from "../../projects/PackageManager.svelte";
    import AgentManager from "../../projects/AgentManager.svelte";
    import type { Project } from "$lib/sandbox";

    // Content pane for `AppKind::Dashboard` (docs/features/responsive-panel/02_dev.md Phase D):
    // an internal SvelteKit route embedded as a second content webview alongside a sandbox's
    // terminal pane (src-tauri/src/workspace/mod.rs, embed_dashboard). Reads which sandbox it
    // belongs to from the query string set at pane-creation time.
    const sandboxName = $derived(page.url.searchParams.get("sandbox") ?? "");

    let workspacePath = $state("");
    const action = createActionRunner();

    onMount(async () => {
        const projects = await invoke<Project[]>("list_sandboxes");
        workspacePath = projects.find((p) => p.name === sandboxName)?.workspace_path ?? "";
    });
</script>

<main class="h-screen overflow-y-auto p-6">
    <h1 class="text-xl font-medium mb-4">{sandboxName}</h1>

    <StatusRow busy={action.busy} message={action.message} error={action.error} />

    {#if workspacePath}
        <div class="flex flex-col gap-6 max-w-2xl">
            <PackageManager {sandboxName} {workspacePath} {action} />
            <AgentManager {sandboxName} {workspacePath} {action} />
        </div>
    {/if}
</main>
