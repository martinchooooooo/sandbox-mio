<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Card from "$lib/components/ui/card";
    import { sandboxesList } from "$lib/sandbox/state.svelte";
    import { FIRST_SANDBOX } from "$lib/onboarding/setup.svelte";
    import { openSandbox } from "$lib/workspace/navigation.svelte";
    import DownloadProgress from "./DownloadProgress.svelte";
    import ModelDetails from "./ModelDetails.svelte";
    import WakeUpStatus from "./WakeUpStatus.svelte";
    import {
        currentEntry,
        downloadAndWake,
        infoOf,
        isDownloaded,
        isServing,
        modelsState,
        needsDownload,
        pick,
        sleep,
    } from "./model.svelte";
    import { PAGE } from "./copy";

    const st = modelsState();
    const d = $derived(currentEntry());
    const info = $derived(d ? infoOf(d) : null);
    const awake = $derived(d !== null && isServing(d));
    const pct = $derived(d ? st.downloadPct[d.id] : undefined);
    const waking = $derived(st.busy === "wake" && st.wake.modelId === d?.id);
    const failed = $derived(st.error !== "" && !st.busy && st.jobModelId === d?.id);
    const hello = $derived(sandboxesList().find((p) => p.name === FIRST_SANDBOX));

    const startLabel = $derived(
        info && info.fit.verdict !== "fits"
            ? PAGE.tryAnyway
            : d && needsDownload(d)
              ? PAGE.downloadAndWake
              : PAGE.wake,
    );
</script>

{#if d && info}
    <Card.Root
        class={["gap-4 p-5", awake && "border-emerald-600 dark:border-emerald-400"]}
        data-model-id={d.id}
        data-fit={info.fit.verdict}
        data-running={awake ? "" : undefined}
        data-base-url={awake ? st.status.baseUrl : undefined}
    >
        <ModelDetails {d} {info} isPick={pick()?.id === d.id} />

        <div class="flex flex-col gap-2" aria-live="polite">
            {#if waking}
                <WakeUpStatus />
            {:else if pct !== undefined}
                <DownloadProgress {pct} />
            {:else}
                <p class={["m-0 flex items-center gap-2 text-sm", awake ? "font-medium text-emerald-600 dark:text-emerald-400" : "text-muted-foreground"]}>
                    {#if awake}<span class="size-2 rounded-full bg-emerald-600 dark:bg-emerald-400"></span>{/if}
                    {awake ? PAGE.awake : needsDownload(d) ? PAGE.notDownloaded : PAGE.asleep}
                </p>
            {/if}
            {#if failed}
                <p class="m-0 text-sm text-destructive">{PAGE.failed}</p>
                <p class="m-0 text-xs text-muted-foreground">{st.error}</p>
            {/if}
        </div>
        {#if isDownloaded(d)}<span hidden data-downloaded></span>{/if}

        <div class="flex flex-wrap items-center gap-3">
            {#if awake}
                <Button variant="outline" data-stop disabled={st.busy !== null} loading={st.busy === "sleep"} onclick={sleep}>
                    {PAGE.sleep}
                </Button>
            {:else if info.fit.verdict !== "engine"}
                <Button data-start disabled={st.busy !== null} loading={st.busy === "download" || st.busy === "wake"} onclick={() => downloadAndWake(d)}>
                    {startLabel}
                </Button>
            {/if}
        </div>

        {#if awake}
            <p class="m-0 text-sm">
                {PAGE.sandboxesUse}
                {#if hello}
                    <Button variant="link" size="sm" class="px-1" onclick={() => openSandbox(hello.name, hello.workspace_path, { restore: true })}>
                        {PAGE.openSandbox}
                    </Button>
                {/if}
            </p>
        {/if}
    </Card.Root>
{/if}
