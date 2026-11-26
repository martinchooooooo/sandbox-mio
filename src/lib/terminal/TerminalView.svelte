<script lang="ts">
    import { Channel } from "@tauri-apps/api/core";
    import { terminalAttach, terminalInput, terminalResize, type TerminalEvent } from "$lib/api/terminal";
    import { Button } from "$lib/components/ui/button/index.js";
    import { Spinner } from "$lib/components/ui/spinner/index.js";
    import { parseError } from "$lib/errors";
    import WtermRenderer from "./WtermRenderer.svelte";

    type Status = "loading" | "connecting" | "attached" | "ended" | "failed";

    let { sandbox, active }: { sandbox: string; active: boolean } = $props();

    let renderer = $state<WtermRenderer | null>(null);
    let status = $state<Status>("loading");
    let error = $state<string | null>(null);
    // The live attach's channel; output and events from a replaced attach are dropped.
    let current: Channel<ArrayBuffer | number[]> | null = null;
    let size = { cols: 0, rows: 0 };
    let sent = { cols: 0, rows: 0 };
    let frame: number | null = null;
    // Bumped by the renderer-failure Retry to remount `WtermRenderer`, since `onReady` fires once.
    let renderKey = $state(0);
    let rendererFailed = $state(false);

    async function attach() {
        if (size.cols === 0) return;
        status = "connecting";
        error = null;
        const output = new Channel<ArrayBuffer | number[]>();
        const events = new Channel<TerminalEvent>();
        current = output;
        output.onmessage = (bytes) => {
            if (current !== output) return;
            renderer?.write(bytes instanceof ArrayBuffer ? new Uint8Array(bytes) : Uint8Array.from(bytes));
        };
        events.onmessage = (event) => {
            if (current !== output || event.type !== "exited") return;
            current = null;
            status = "ended";
        };
        const { cols, rows } = size;
        try {
            await terminalAttach(sandbox, cols, rows, output, events);
            if (current !== output) return;
            sent = { cols, rows };
            status = "attached";
            if (active) {
                syncSize();
                renderer?.focus();
            }
        } catch (e) {
            if (current !== output) return;
            current = null;
            status = "failed";
            error = parseError(e).message;
        }
    }

    // Tauri runs async commands concurrently, so independent invokes can reorder keystrokes. One
    // chain per view serialises every input and resize send; queued input coalesces into one send.
    let chain: Promise<unknown> = Promise.resolve();
    let queuedInput = "";

    function enqueue(send: () => Promise<void>) {
        chain = chain
            .then(send)
            .catch((e) => console.error(parseError(e).message));
    }

    function onInput(data: string) {
        if (status !== "attached") return;
        if (queuedInput !== "") {
            queuedInput += data;
            return;
        }
        queuedInput = data;
        chain = chain
            .then(() => {
                const batch = queuedInput;
                queuedInput = "";
                return terminalInput(sandbox, batch);
            })
            .catch((e) => console.error(parseError(e).message));
    }

    // A hidden terminal's size is never sent, so Herdr keeps the visible layout
    // (docs/features/native-terminal/02_dev.md, Common Gotchas).
    function syncSize() {
        if (frame !== null || !active || status !== "attached") return;
        frame = requestAnimationFrame(() => {
            frame = null;
            if (!active || status !== "attached") return;
            if (size.cols === sent.cols && size.rows === sent.rows) return;
            sent = { ...size };
            const { cols, rows } = sent;
            enqueue(() => terminalResize(sandbox, cols, rows));
        });
    }

    function onResize(cols: number, rows: number) {
        size = { cols, rows };
        syncSize();
    }

    function onReady(cols: number, rows: number) {
        size = { cols, rows };
        attach();
    }

    function onClipboardWrite(text: string) {
        navigator.clipboard.writeText(text).catch((e) => console.error(e));
    }

    function retryRenderer() {
        rendererFailed = false;
        status = "loading";
        error = null;
        renderKey += 1;
    }

    function onError(e: unknown) {
        rendererFailed = true;
        status = "failed";
        error = parseError(e).message;
    }

    $effect(() => {
        if (active && status === "attached") {
            syncSize();
            renderer?.focus();
        }
    });

    $effect(() => () => {
        current = null;
        if (frame !== null) cancelAnimationFrame(frame);
    });
</script>

<div class="relative h-full w-full bg-[#1e1e1e]" data-terminal={sandbox} data-status={status}>
    {#key renderKey}
        <WtermRenderer bind:this={renderer} paused={!active} {onInput} {onResize} {onReady} {onClipboardWrite} {onError} />
    {/key}
    {#if status === "loading" || status === "connecting"}
        <div class="absolute inset-0 flex items-center justify-center gap-2 bg-[#1e1e1e] text-sm text-neutral-300">
            <Spinner /> Connecting to the terminal… <!-- TODO(i18n) -->
        </div>
    {:else if status === "ended"}
        <div class="absolute inset-x-0 bottom-0 flex items-center justify-center gap-3 bg-[#1e1e1e]/90 p-3 text-sm text-neutral-300">
            The terminal session ended. <!-- TODO(i18n) -->
            <Button size="sm" variant="outline" onclick={attach} data-terminal-reconnect>Reconnect</Button> <!-- TODO(i18n) -->
        </div>
    {:else if status === "failed"}
        <div class="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-[#1e1e1e] p-6 text-center text-sm text-neutral-300">
            <p>Could not connect to the terminal. <!-- TODO(i18n) --></p>
            {#if error}<p class="max-w-xl break-words text-neutral-400" data-terminal-error>{error}</p>{/if}
            <Button size="sm" onclick={rendererFailed ? retryRenderer : attach} data-terminal-retry>Retry</Button> <!-- TODO(i18n) -->
        </div>
    {/if}
</div>
