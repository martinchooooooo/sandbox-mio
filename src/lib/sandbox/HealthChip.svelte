<script lang="ts">
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { DashboardSpeed01Icon, RefreshIcon } from "@hugeicons/core-free-icons";
    import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
    import { cn } from "$lib/utils";
    import { formatBytes, formatCpu } from "$lib/sandbox";
    import { healthFor, latestSampleFor } from "$lib/sandbox/resources.svelte";
    import type { Health } from "$lib/sandbox/health";

    let {
        sandbox,
        restarting,
        onRestart,
    }: { sandbox: string; restarting: boolean; onRestart: () => void } = $props();

    const health = $derived(healthFor(sandbox));
    const sample = $derived(latestSampleFor(sandbox));

    const label: Record<Health, string> = {
        unknown: "",
        ok: "",
        busy: "Busy", // TODO(i18n)
        strained: "Low memory", // TODO(i18n)
    };
    const headline: Record<Health, string> = {
        unknown: "",
        ok: "Running smoothly", // TODO(i18n)
        busy: "Working hard", // TODO(i18n)
        strained: "Running low on memory — restarting may help", // TODO(i18n)
    };
    const tone: Record<Health, string> = {
        unknown: "",
        ok: "text-green-600 dark:text-green-500",
        busy: "border-yellow-500/60 bg-yellow-400/15 text-yellow-700 dark:text-yellow-400",
        strained: "border-destructive/60 bg-destructive/10 text-destructive",
    };
    const memoryText = $derived(
        sample ? `${formatBytes(sample.memoryUsed)} of ${formatBytes(sample.memoryLimit)}` : "", // TODO(i18n)
    );
</script>

{#if health !== "unknown" && sample}
    <DropdownMenu.Root>
        <DropdownMenu.Trigger>
            {#snippet child({ props })}
                <button
                    {...props}
                    type="button"
                    data-health-chip
                    data-health={health}
                    title={headline[health]}
                    aria-label={headline[health]}
                    class={cn(
                        "flex h-6 shrink-0 items-center gap-1 rounded-md border border-transparent px-1.5 text-xs font-medium",
                        tone[health],
                    )}
                >
                    <HugeiconsIcon icon={DashboardSpeed01Icon} size={14} />
                    {#if label[health]}<span>{label[health]}</span>{/if}
                </button>
            {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="start" class="w-72" data-health-popover>
            <DropdownMenu.Label class="whitespace-normal">{headline[health]}</DropdownMenu.Label>
            <dl class="text-muted-foreground grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 px-2 pb-2 text-xs">
                <dt>Memory</dt><!-- TODO(i18n) -->
                <dd data-health-memory>{memoryText}</dd>
                <dt>CPU</dt><!-- TODO(i18n) -->
                <dd data-health-cpu>{sample.cpu === null ? "…" : formatCpu(sample.cpu)}</dd>
            </dl>
            {#if health === "strained"}
                <DropdownMenu.Separator />
                <DropdownMenu.Item disabled={restarting} onSelect={onRestart} data-health-restart>
                    <HugeiconsIcon icon={RefreshIcon} size={16} />
                    Restart<!-- TODO(i18n) -->
                </DropdownMenu.Item>
            {/if}
        </DropdownMenu.Content>
    </DropdownMenu.Root>
{/if}
