<script lang="ts">
    // The only file that imports `@wterm/*`: swapping the renderer means replacing this file
    // (docs/features/native-terminal/02_dev.md, Resolved Decision 1).
    import { onMount } from "svelte";
    import { Terminal, type WTerm } from "@wterm/svelte";
    import { GhosttyCore } from "@wterm/ghostty";
    import "@wterm/svelte/css";

    let {
        paused = false,
        onInput,
        onResize,
        onReady,
        onClipboardWrite,
        onError,
    }: {
        /** Stops painting while the terminal is hidden; output is still parsed. */
        paused?: boolean;
        onInput: (data: string) => void;
        onResize: (cols: number, rows: number) => void;
        /** Fires once, with the size measured from the container. */
        onReady: (cols: number, rows: number) => void;
        onClipboardWrite?: (text: string) => void;
        onError: (error: unknown) => void;
    } = $props();

    let core = $state<GhosttyCore | null>(null);
    let instance = $state<WTerm | null>(null);

    onMount(() => {
        let disposed = false;
        let loaded: GhosttyCore | null = null;
        GhosttyCore.load()
            .then((c) => {
                if (disposed) {
                    c.dispose();
                    return;
                }
                loaded = c;
                core = c;
            })
            .catch((e) => {
                if (!disposed) onError(e);
            });
        return () => {
            disposed = true;
            loaded?.dispose();
        };
    });

    export function write(bytes: Uint8Array): void {
        instance?.write(bytes);
    }

    export function focus(): void {
        instance?.focus();
    }

    /** The retained history and screen as text, for e2e specs whichever renderer is active. */
    export async function readText(): Promise<string> {
        return (await instance?.readText()) ?? "";
    }
</script>

{#if core}
    <Terminal
        {core}
        bind:instance
        autoResize
        renderer="dom"
        renderingPaused={paused}
        onData={onInput}
        {onResize}
        {onClipboardWrite}
        onReady={(wt: WTerm) => onReady(wt.cols, wt.rows)}
        {onError}
        style="width: 100%; height: 100%; border-radius: 0; box-shadow: none;"
    />
{/if}
