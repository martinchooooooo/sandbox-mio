<script lang="ts">
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { LoaderPinwheelIcon } from "@hugeicons/core-free-icons";

    let { busy, message, error, status = "" }: {
        busy: string | null;
        message: string;
        error: string;
        status?: string;
    } = $props();
</script>

{#if busy !== null && message}
    <div role="status" aria-live="polite" class="flex items-center gap-1.5 text-xs text-muted-foreground">
        <HugeiconsIcon icon={LoaderPinwheelIcon} class="animate-spin size-3.5" />
        <span>{message}</span>
    </div>
{:else if error}
    <div role="status" aria-live="polite" class="text-xs text-destructive">{error}</div>
{:else if status}
    <div class="text-xs text-muted-foreground">{status}</div>
{/if}
