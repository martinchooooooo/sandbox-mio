<script lang="ts">
    import { activeAppLabel, overlayRect, workspaceApps } from "$lib/workspace/state.svelte";
    import TerminalView from "./TerminalView.svelte";

    // Every registered terminal stays mounted, hidden ones with painting paused, so output written
    // while hidden is on screen when the pane shows again
    // (docs/features/native-terminal/02_dev.md, Resolved Decision 2).
    const terminals = $derived(workspaceApps().filter((p) => p.kind === "terminal"));
    const overlay = $derived(overlayRect());
</script>

<!-- The pane area always reaches the window's right and bottom edges, so only Rust's origin is
     needed; the window size follows from CSS without a state event per resize. -->
<div
    class="pointer-events-none fixed right-0 bottom-0 z-10"
    style:left="{overlay.x}px"
    style:top="{overlay.y}px"
    data-terminal-layer
>
    {#each terminals as pane (pane.label)}
        {@const active = activeAppLabel() === pane.label}
        <div
            class="absolute inset-0"
            class:pointer-events-auto={active}
            style:visibility={active ? "visible" : "hidden"}
        >
            <TerminalView sandbox={pane.sandbox} {active} />
        </div>
    {/each}
</div>
