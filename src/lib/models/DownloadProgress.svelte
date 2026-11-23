<script lang="ts">
    import { Progress } from "$lib/components/ui/progress";
    import { PAGE } from "./copy";

    let { pct }: { pct: number } = $props();
</script>

<div class="flex flex-col gap-1">
    <p class="m-0 text-sm text-muted-foreground">{PAGE.downloading(pct)}</p>
    <Progress value={pct} aria-label={PAGE.downloading(pct)} />
</div>
