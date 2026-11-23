<script lang="ts">
    import type { Snippet } from "svelte";
    import AgentAvatar from "./AgentAvatar.svelte";
    import RoleExplainer from "./RoleExplainer.svelte";
    import type { Frame, Role, ToolKind } from "./anatomy";

    // Without a role the row has no avatar or name and only keeps the content aligned with rows that do.
    let {
        role,
        frame = null,
        live = false,
        label = "",
        tool = null,
        children,
    }: {
        role?: Role;
        frame?: Frame | null;
        live?: boolean;
        label?: string;
        tool?: ToolKind | null;
        children: Snippet;
    } = $props();
</script>

<!-- The label line's h-7 equals the avatar's size, which centres the name on the avatar. -->
<div
    class="grid grid-cols-[1.75rem_minmax(0,1fr)] gap-x-2.5"
    data-part-row={role ? "" : undefined}
    data-row-role={role}
    data-row-frame={frame}
    data-row-live={role ? live : undefined}
>
    {#if role}
        <div class="flex flex-col items-center gap-1">
            <AgentAvatar {role} {frame} {tool} {live} />
            <span class="w-0.5 flex-1 rounded-full {role === 'arms-act' ? 'bg-arms/30' : 'bg-brain/30'}" aria-hidden="true"></span>
        </div>
    {:else}
        <div></div>
    {/if}
    <div class="min-w-0 pb-2">
        {#if role}
            <p class="m-0 flex h-7 items-center gap-1 text-xs text-muted-foreground" data-agent-label>
                <span class="truncate">{label}</span>
                <RoleExplainer {role} />
            </p>
        {/if}
        {@render children()}
    </div>
</div>
