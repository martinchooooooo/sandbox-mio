<script lang="ts">
    import { getResourceHistory } from "$lib/api/sandbox";
    import { onMount } from "svelte";
    import { toast } from "svelte-sonner";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { Copy01Icon } from "@hugeicons/core-free-icons";
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Card from "$lib/components/ui/card";
    import { parseError } from "$lib/errors";
    import { providerLabel } from "$lib/providers";
    import { formatBytes, formatCpu } from "$lib/sandbox";
    import { poolRow, sandboxRow, type UsageRow } from "$lib/settings/usage";

    const POOL_NAME = "Podman machine"; // TODO(i18n)

    let rows = $state<UsageRow[]>([]);
    let loaded = $state(false);
    let error = $state("");

    onMount(async () => {
        try {
            const history = await getResourceHistory("week");
            const sandboxes = history.sandboxes.flatMap((s) => sandboxRow(s.name, s.rollups) ?? []);
            const pool = poolRow(POOL_NAME, history.podmanPool);
            rows = pool ? [...sandboxes, pool] : sandboxes;
        } catch (e) {
            error = parseError(e).message;
        } finally {
            loaded = true;
        }
    });

    function providerText(row: UsageRow): string {
        return providerLabel(row.provider ?? "podman");
    }

    function cells(row: UsageRow): string[] {
        return [
            row.name,
            providerText(row),
            formatBytes(row.memoryLimit),
            formatBytes(row.peakMemory),
            formatBytes(row.p95Memory),
            String(row.minutesHigh),
            row.meanCpu === null ? "—" : formatCpu(row.meanCpu),
            String(row.minutes),
        ];
    }

    const headers = [
        "Sandbox", // TODO(i18n)
        "Engine", // TODO(i18n)
        "Memory limit", // TODO(i18n)
        "Peak", // TODO(i18n)
        "95th percentile", // TODO(i18n)
        "Minutes ≥ 90 %", // TODO(i18n)
        "Mean CPU", // TODO(i18n)
        "Minutes sampled", // TODO(i18n)
    ];

    function reportText(): string {
        const table = [headers, ...rows.map(cells)];
        const widths = headers.map((_, i) => Math.max(...table.map((r) => r[i].length)));
        const lines = table.map((r) => r.map((c, i) => c.padEnd(widths[i])).join("  ").trimEnd());
        return [
            "Sandbox resource usage, last 7 days", // TODO(i18n)
            "",
            ...lines,
            "",
            "The Podman machine's figures count page cache as used.", // TODO(i18n)
        ].join("\n");
    }

    async function copyReport() {
        try {
            await navigator.clipboard.writeText(reportText());
            toast.success("Report copied"); // TODO(i18n)
        } catch (e) {
            toast.error(parseError(e).message);
        }
    }
</script>

<Card.Root data-resource-usage>
    <Card.Header>
        <Card.Title>Resource usage</Card.Title><!-- TODO(i18n) -->
        <Card.Description>
            How much memory and CPU each sandbox used over the last 7 days. Nothing leaves this computer unless you copy the report and send it.<!-- TODO(i18n) -->
        </Card.Description>
        <Card.Action>
            <Button size="sm" variant="outline" disabled={rows.length === 0} onclick={copyReport} data-copy-report>
                <HugeiconsIcon icon={Copy01Icon} size={16} />
                Copy report<!-- TODO(i18n) -->
            </Button>
        </Card.Action>
    </Card.Header>
    <Card.Content>
        {#if error}
            <p class="text-destructive text-sm">{error}</p>
        {:else if loaded && rows.length === 0}
            <p class="text-muted-foreground text-sm" data-resource-usage-empty>
                No usage recorded yet. Usage is recorded while a sandbox runs.<!-- TODO(i18n) -->
            </p>
        {:else if rows.length > 0}
            <div class="overflow-x-auto">
                <table class="w-full text-left text-xs">
                    <thead class="text-muted-foreground">
                        <tr>
                            {#each headers as header (header)}<th class="py-1 pr-3 font-medium whitespace-nowrap">{header}</th>{/each}
                        </tr>
                    </thead>
                    <tbody>
                        {#each rows as row (row.name + (row.provider ?? "pool"))}
                            <tr class="border-t" data-usage-row={row.provider === null ? "podman-machine" : row.name}>
                                {#each cells(row) as cell, i (i)}<td class="py-1 pr-3 whitespace-nowrap">{cell}</td>{/each}
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
            <p class="text-muted-foreground mt-2 text-xs">
                The Podman machine's figures count page cache as used.<!-- TODO(i18n) -->
            </p>
        {/if}
    </Card.Content>
</Card.Root>
