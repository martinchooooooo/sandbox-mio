<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Textarea } from "$lib/components/ui/textarea";
    import { invoke, Channel } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { slide } from "svelte/transition";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { LoaderPinwheelIcon, RefreshIcon } from "@hugeicons/core-free-icons";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import RuntimeSteps from "$lib/models/RuntimeSteps.svelte";
    import { recommendModels, type RankedModel } from "$lib/models/recommend";
    import { testModelFromEnv } from "$lib/models/catalogue";
    import { reprovisionInstalledAgents } from "$lib/agents/reprovision";
    import type {
        EngineKind,
        InferenceStatus,
        MachineSpecs,
        ModelDescriptor,
        ModelDownloadEvent,
        ModelRating,
        RuntimeSetupEvent,
        SetupStep,
    } from "$lib/models/types";

    const action = createActionRunner();
    const GB = 1024 * 1024 * 1024;

    let specs = $state<MachineSpecs | null>(null);
    let ranked = $state<RankedModel[]>([]);
    let downloaded = $state<string[]>([]);
    let engine = $state<EngineKind>("llama");
    let status = $state<InferenceStatus>({ running: false, engine: null, modelId: null, baseUrl: null });

    // Per-model download progress (0–100), keyed by model id; absent while not downloading.
    let downloadPct = $state<Record<string, number>>({});

    let startingModel = $state<string | null>(null);
    // Catalogue id for the setup checklist; persists after startingModel clears.
    let setupModel = $state<string | null>(null);
    let setupSteps = $state<SetupStep[]>([]);
    let setupEvents = $state<RuntimeSetupEvent[]>([]);
    // After a start completes the checklist auto-collapses behind a "view details" toggle.
    let stepsCollapsed = $state(false);
    // Gates the toggle button: only appears once the initial auto-collapse has fired.
    let collapsedOnce = $state(false);
    let collapseTimer: ReturnType<typeof setTimeout> | null = null;

    // Chat test box.
    let prompt = $state("");
    let reply = $state("");
    let chatting = $state(false);

    function ratingLabel(r: ModelRating): string {
        return r === "recommended"
            ? m.models_rating_recommended()
            : r === "slow"
              ? m.models_rating_slow()
              : m.models_rating_wont_fit();
    }
    function ratingClass(r: ModelRating): string {
        return r === "recommended"
            ? "text-emerald-600 dark:text-emerald-400"
            : r === "slow"
              ? "text-amber-600 dark:text-amber-400"
              : "text-destructive";
    }
    function gb(bytes: number): string {
        return (bytes / GB).toFixed(1);
    }
    const isDownloaded = (id: string) => downloaded.includes(id);

    // The id `start_inference` was called with for this descriptor: the catalogue id for llama,
    // the Foundry alias for foundry. Matched against `status.modelId` to find the running row.
    function startedTarget(d: ModelDescriptor): string {
        return (status.engine ?? engine) === "foundry" ? (d.foundryAlias ?? d.id) : d.id;
    }
    function isRunning(d: ModelDescriptor): boolean {
        return status.running && status.modelId !== null && status.modelId === startedTarget(d);
    }

    // Catalogue rows with downloaded models floated to the top; stable sort preserves the
    // recommend-first / params order from `recommendModels` within each group.
    const displayModels = $derived(
        [...ranked].sort(
            (a, b) => Number(isDownloaded(b.descriptor.id)) - Number(isDownloaded(a.descriptor.id)),
        ),
    );

    async function refreshStatus() {
        status = await invoke<InferenceStatus>("inference_status");
    }

    async function redetectSpecs() {
        await action.run("redetect", async () => {
            specs = await invoke<MachineSpecs>("redetect_machine_specs");
            ranked = recommendModels(specs);
        });
    }

    async function downloadModel(model: ModelDescriptor) {
        if (!model.llama) return;
        const src = model.llama;
        downloadPct[model.id] = 0;
        await action.run("download", async () => {
            const channel = new Channel<ModelDownloadEvent>();
            channel.onmessage = (e) => {
                if (e.kind === "Progress" && e.total_bytes > 0) {
                    downloadPct[model.id] = Math.floor((e.downloaded_bytes / e.total_bytes) * 100);
                }
            };
            await invoke("download_model", {
                modelId: model.id,
                repo: src.repo,
                file: src.file,
                sha256: src.sha256,
                onProgress: channel,
            });
        });
        delete downloadPct[model.id];
        downloaded = await invoke<string[]>("downloaded_models");
    }

    async function startModel(model: ModelDescriptor) {
        if (collapseTimer) clearTimeout(collapseTimer);
        stepsCollapsed = false;
        collapsedOnce = false;
        startingModel = model.id;
        setupModel = model.id;
        setupEvents = [];
        reply = "";
        try {
            setupSteps = await invoke<SetupStep[]>("inference_setup_steps", { engine });
        } catch {
            setupSteps = [];
        }
        await action.run("start", async () => {
            const channel = new Channel<RuntimeSetupEvent>();
            channel.onmessage = (e) => {
                setupEvents = [...setupEvents, e];
            };
            // Foundry loads by its catalog alias; the llama engine loads by the catalogue model id.
            const target = engine === "foundry" ? (model.foundryAlias ?? model.id) : model.id;
            status = await invoke<InferenceStatus>("start_inference", {
                engine,
                modelId: target,
                onProgress: channel,
            });
        });
        startingModel = null;
        // Let the finished checklist linger briefly, then slide it away behind "view details".
        if (status.running) {
            collapseTimer = setTimeout(() => {
                stepsCollapsed = true;
                collapsedOnce = true;
            }, 2000);
            // Keep every installed agent's default model in sync with whatever's now serving, so
            // e.g. OpenCode doesn't keep targeting a model the server no longer runs.
            if (status.modelId) reprovisionInstalledAgents(status.modelId);
        }
    }

    async function stop() {
        await action.run("stop", () => invoke("stop_inference"));
        await refreshStatus();
        if (collapseTimer) clearTimeout(collapseTimer);
        setupEvents = [];
        setupModel = null;
        stepsCollapsed = false;
        collapsedOnce = false;
    }

    async function sendPrompt() {
        if (!status.baseUrl || !status.modelId || !prompt.trim()) return;
        chatting = true;
        reply = "";
        try {
            const res = await fetch(`${status.baseUrl}/chat/completions`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    model: status.modelId,
                    messages: [{ role: "user", content: prompt }],
                    max_tokens: 256,
                }),
            });
            const body = await res.json();
            reply = body?.choices?.[0]?.message?.content?.trim() || m.models_test_empty();
        } catch (e) {
            reply = String(e);
        }
        chatting = false;
    }

    onMount(async () => {
        try {
            const s = await invoke<{ inferenceEngine: EngineKind }>("get_settings");
            engine = s.inferenceEngine;
        } catch {
            /* default llama */
        }
        try {
            specs = await invoke<MachineSpecs>("detect_machine_specs");
            ranked = recommendModels(specs);
        } catch (e) {
            action.run("detect", () => Promise.reject(e));
        }
        // A test-only model (e2e) is prepended to the list when MIO_TEST_MODEL is set.
        try {
            const testModel = testModelFromEnv(await invoke<string | null>("test_model") ?? undefined);
            if (testModel && specs) {
                ranked = [{ descriptor: testModel, rating: "recommended" }, ...ranked];
            }
        } catch {
            /* no test model */
        }
        try {
            downloaded = await invoke<string[]>("downloaded_models");
        } catch {
            downloaded = [];
        }
        await refreshStatus();
    });
</script>

<main class="p-3 pb-6 @max-[420px]:p-2">
    <h1 class="text-3xl mb-2">{m.models_title()}</h1>
    <p class="text-sm text-muted-foreground mb-6">{m.models_engine_label({ engine })}</p>

    <!-- Step 1: probed host specs -->
    <section class="border rounded-lg p-4 mb-6" data-specs>
        <div class="flex items-center justify-between mb-2">
            <h2 class="text-sm font-medium">{m.models_specs_title()}</h2>
            {#if specs}
                <Button
                    size="icon"
                    variant="ghost"
                    data-redetect
                    loading={action.busy === "redetect"}
                    disabled={action.busy !== null}
                    onclick={redetectSpecs}
                    title={m.models_redetect()}
                    aria-label={m.models_redetect()}
                >
                    <HugeiconsIcon icon={RefreshIcon} class="size-3.5" />
                </Button>
            {/if}
        </div>
        {#if specs}
            <ul class="text-sm text-muted-foreground space-y-0.5" data-specs-ready>
                <li>{m.models_cpu_cores({ cores: specs.cpuCores })}</li>
                <li>{m.models_ram({ free: gb(specs.freeRamBytes), total: gb(specs.totalRamBytes) })}</li>
                <li>
                    {m.models_gpu()}:
                    {specs.gpuNames.length > 0 ? specs.gpuNames.join(", ") : m.models_no_gpu()}
                    {#if specs.hasNpu}· {m.models_npu()}{/if}
                </li>
            </ul>
        {:else}
            <div role="status" aria-live="polite" class="flex items-center gap-1.5 text-sm text-muted-foreground">
                <HugeiconsIcon icon={LoaderPinwheelIcon} class="animate-spin size-3.5" />
                <span>{m.models_detecting()}</span>
            </div>
        {/if}
    </section>

    <!-- Step 2 & 3: ranked catalogue with download / start -->
    {#if ranked.length === 0 && specs}
        <p class="text-sm text-muted-foreground">{m.models_no_models()}</p>
    {/if}
    <ul class="space-y-3">
        {#each displayModels as { descriptor, rating } (descriptor.id)}
            <li class="border rounded-lg p-4" data-model-id={descriptor.id} data-running={isRunning(descriptor) ? "" : undefined}>
                <div class="flex items-center justify-between gap-3">
                    <div class="flex-1 min-w-0 flex items-center gap-2">
                        <span class="font-medium truncate">{descriptor.label}</span>
                        <span class={"shrink-0 text-xs " + ratingClass(rating)} data-rating>{ratingLabel(rating)}</span>
                        {#if isRunning(descriptor)}
                            <div class="text-xs mt-0.5">
                                <span class="text-emerald-600 dark:text-emerald-400 font-medium">{m.models_running()}</span>
                                <span class="text-muted-foreground ml-1 @max-[420px]:hidden">{m.models_base_url()}:</span>
                                <code data-base-url class="@max-[420px]:hidden">{status.baseUrl}</code>
                            </div>
                        {/if}
                    </div>
                    <div class="flex items-center gap-2">
                        {#if descriptor.llama}
                            {#if isDownloaded(descriptor.id)}
                                <span class="text-xs text-emerald-600 dark:text-emerald-400" data-downloaded>{m.models_downloaded()}</span>
                            {:else if downloadPct[descriptor.id] !== undefined}
                                <span class="text-xs text-muted-foreground">{m.models_downloading({ percent: downloadPct[descriptor.id] })}</span>
                            {:else}
                                <Button size="sm" variant="outline"
                                    disabled={action.busy !== null}
                                    onclick={() => downloadModel(descriptor)}>{m.models_download()}</Button>
                            {/if}
                        {:else}
                            <span class="text-xs text-muted-foreground">{m.models_foundry_first_run()}</span>
                        {/if}
                        {#if isRunning(descriptor)}
                            <Button size="sm" variant="outline"
                                disabled={action.busy !== null}
                                onclick={stop}>{m.models_stop()}</Button>
                        {:else}
                            <Button size="sm"
                                data-start
                                loading={startingModel === descriptor.id}
                                disabled={action.busy !== null || (engine === "llama" && descriptor.llama !== null && !isDownloaded(descriptor.id))}
                                onclick={() => startModel(descriptor)}>{m.models_start()}</Button>
                        {/if}
                    </div>
                </div>
                {#if rating !== "recommended"}
                    <p class="text-xs text-amber-600 dark:text-amber-400 mt-2">{m.models_memory_warning()}</p>
                {/if}
                {#if engine === "foundry" && descriptor.foundryAlias === null}
                    <p class="text-xs text-muted-foreground mt-1">— (llama only)</p>
                {/if}

                <!-- Step 4: live runtime-setup checklist for the starting model -->
                {#if startingModel === descriptor.id || (setupModel === descriptor.id && setupEvents.length > 0)}
                    <div class="mt-3 border-t pt-3">
                        {#if collapsedOnce}
                            <button type="button" class="text-xs text-muted-foreground hover:text-foreground underline mb-2"
                                onclick={() => (stepsCollapsed = !stepsCollapsed)}>
                                {stepsCollapsed ? m.models_view_details() : m.models_hide_details()}
                            </button>
                        {/if}
                        {#if !stepsCollapsed}
                            <div transition:slide={{ duration: 300 }}>
                                <RuntimeSteps steps={setupSteps} events={setupEvents} />
                            </div>
                        {/if}
                    </div>
                {/if}
            </li>
        {/each}
    </ul>

    {#if action.error}<p class="text-destructive mt-4 text-sm">{action.error}</p>{/if}

    <!-- Chat test surface for the running endpoint -->
    {#if status.running}
        <section class="border rounded-lg p-4 mt-6" data-chat>
            <div>
                <h3 class="text-sm font-medium mb-2">{m.models_test_title()}</h3>
                <Textarea
                    value={prompt}
                    oninput={(e) => (prompt = e.currentTarget.value)}
                    placeholder={m.models_test_placeholder()}
                    rows={2}
                />
                <div class="flex items-center gap-3 mt-2">
                    <Button size="sm" loading={chatting} disabled={chatting} onclick={sendPrompt}>{m.models_test_send()}</Button>
                    {#if chatting}<span class="text-xs text-muted-foreground">{m.models_test_thinking()}</span>{/if}
                </div>
                {#if reply}<p class="text-sm mt-3 whitespace-pre-wrap" data-reply>{reply}</p>{/if}
            </div>
        </section>
    {/if}
</main>
