<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Card from "$lib/components/ui/card";
    import AdvancedSection from "$lib/components/AdvancedSection.svelte";
    import { sandboxesList } from "$lib/sandbox/state.svelte";
    import { FIRST_SANDBOX } from "$lib/onboarding/setup.svelte";
    import { openSandbox } from "$lib/workspace/navigation.svelte";
    import DownloadProgress from "./DownloadProgress.svelte";
    import ModelDetails from "./ModelDetails.svelte";
    import ModelFacts from "./ModelFacts.svelte";
    import RuntimeSteps from "./RuntimeSteps.svelte";
    import TryIt from "./TryIt.svelte";
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
    import { ENGINE_NAMES, PAGE } from "./copy";

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
        class="gap-4 p-5"
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
                <p class="m-0 text-sm text-muted-foreground">
                    {awake ? PAGE.awake : needsDownload(d) ? PAGE.notDownloaded : PAGE.asleep}
                </p>
            {/if}
            {#if failed}<p class="m-0 text-sm text-destructive">{PAGE.failed}</p>{/if}
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
            <div class="flex flex-col gap-3 border-t-2 pt-3">
                <p class="m-0 text-sm">
                    {PAGE.sandboxesUse}
                    {#if hello}
                        <Button variant="link" size="sm" class="px-1" onclick={() => openSandbox(hello.name, hello.workspace_path, { restore: true })}>
                            {PAGE.openSandbox}
                        </Button>
                    {/if}
                </p>
                <TryIt />
            </div>
        {/if}

        <AdvancedSection label={PAGE.forThoseWhoKnow} data-wake-details>
            <div class="flex flex-col gap-3">
                <ModelFacts {d}>
                    <dt class="text-muted-foreground">{PAGE.facts.engine}</dt>
                    <dd class="m-0">{ENGINE_NAMES[st.engine]}</dd>
                    {#if awake && st.status.baseUrl}
                        <dt class="text-muted-foreground">{PAGE.facts.endpoint}</dt>
                        <dd class="m-0"><code>{st.status.baseUrl}</code></dd>
                    {/if}
                </ModelFacts>
                <p class="m-0 text-xs"><a href="/settings" class="underline">{PAGE.engineLink}</a></p>
                {#if failed}<p class="m-0 text-xs text-foreground/80">{st.error}</p>{/if}
                {#if st.wake.steps.length > 0}
                    <p class="m-0 text-xs font-medium">{PAGE.wakeSteps}</p>
                    <RuntimeSteps steps={st.wake.steps} events={st.wake.events} />
                {/if}
            </div>
        </AdvancedSection>
    </Card.Root>
{/if}
