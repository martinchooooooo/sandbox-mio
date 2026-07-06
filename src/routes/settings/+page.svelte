<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Field from "$lib/components/ui/field";
    import { Input } from "$lib/components/ui/input";
    import { invoke, Channel } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { parseError } from "$lib/errors";
    import { createActionRunner } from "$lib/actions.svelte";
    import RuntimeSteps from "$lib/models/RuntimeSteps.svelte";
    import type { EngineKind, MachineSpecs, SetupStep } from "$lib/models/types";
    import { toggleMode } from "mode-watcher";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { SunIcon, MoonIcon } from "@hugeicons/core-free-icons";
    import { locales, getLocale, setLocale } from "$lib/paraglide/runtime.js";

    function cycleLocale() {
        const current = getLocale();
        const idx = locales.indexOf(current);
        setLocale(locales[(idx + 1) % locales.length]);
    }

    interface AppSettings {
        inferenceEngine: EngineKind;
        llamaGpuLayers: number | null;
        llamaCtxSize: number | null;
    }

    const action = createActionRunner();

    let engine = $state<EngineKind>("llama");
    let gpuLayers = $state("");
    let ctxSize = $state("");
    let specs = $state<MachineSpecs | null>(null);
    let steps = $state<SetupStep[]>([]);
    let installLog = $state<string[]>([]);
    let installing = $state(false);
    let saved = $state(false);

    async function loadSteps(forEngine: EngineKind) {
        try {
            steps = await invoke<SetupStep[]>("runtime_setup_steps", { engine: forEngine });
        } catch {
            steps = [];
        }
    }

    async function selectEngine(next: EngineKind) {
        engine = next;
        saved = false;
        await loadSteps(next);
    }

    async function save() {
        const settings: AppSettings = {
            inferenceEngine: engine,
            llamaGpuLayers: gpuLayers.trim() === "" ? null : Number(gpuLayers),
            llamaCtxSize: ctxSize.trim() === "" ? null : Number(ctxSize),
        };
        if (await action.run("save", () => invoke("set_settings", { settings }))) {
            saved = true;
        }
    }

    async function installFoundry() {
        installing = true;
        installLog = [];
        await action.run("install-foundry", async () => {
            const channel = new Channel<{ kind: string; line: string }>();
            channel.onmessage = (e) => {
                if (e.kind === "Output") installLog = [...installLog, e.line];
            };
            await invoke("install_foundry_local", { onProgress: channel });
        });
        installing = false;
        // Re-probe so the install button disappears once Foundry is present.
        try {
            specs = await invoke<MachineSpecs>("detect_machine_specs");
        } catch (e) {
            action.run("ignore", () => Promise.reject(e));
        }
    }

    onMount(async () => {
        try {
            const s = await invoke<AppSettings>("get_settings");
            engine = s.inferenceEngine;
            gpuLayers = s.llamaGpuLayers === null ? "" : String(s.llamaGpuLayers);
            ctxSize = s.llamaCtxSize === null ? "" : String(s.llamaCtxSize);
        } catch {
            /* defaults */
        }
        await loadSteps(engine);
        try {
            specs = await invoke<MachineSpecs>("detect_machine_specs");
        } catch {
            specs = null;
        }
    });

    const showFoundryInstall = $derived(
        specs !== null && !specs.foundryInstalled && specs.foundryInstallable,
    );
</script>

<main class="p-3 pb-6 @max-[420px]:p-2">
    <h1 class="text-3xl mb-8">{m.settings_title()}</h1>

    <Field.Set class="mb-8">
        <Field.Legend>{m.settings_appearance()}</Field.Legend>
        <Field.Group>
            <Field.Field orientation="horizontal">
                <Field.Label>{m.settings_theme_label()}</Field.Label>
                <Button onclick={toggleMode} variant="outline">
                    <HugeiconsIcon icon={SunIcon}></HugeiconsIcon>
                    /
                    <HugeiconsIcon icon={MoonIcon}></HugeiconsIcon>
                </Button>
            </Field.Field>
            <Field.Field orientation="horizontal">
                <Field.Label>{m.settings_language_label()}</Field.Label>
                <Button onclick={cycleLocale} variant="outline">
                    {getLocale().toUpperCase()}
                </Button>
            </Field.Field>
        </Field.Group>
    </Field.Set>

    <Field.Set>
        <Field.Legend>{m.settings_advanced()}</Field.Legend>
        <Field.Group>
            <Field.Field>
                <Field.Label>{m.settings_engine_label()}</Field.Label>
                <div class="flex flex-col gap-2" data-active-engine={engine}>
                    <Button
                        data-engine="llama"
                        variant={engine === "llama" ? "default" : "outline"}
                        onclick={() => selectEngine("llama")}
                        class="justify-start"
                    >{m.settings_engine_llama()}</Button>
                    <Button
                        data-engine="foundry"
                        variant={engine === "foundry" ? "default" : "outline"}
                        onclick={() => selectEngine("foundry")}
                        class="justify-start"
                    >{m.settings_engine_foundry()}</Button>
                </div>
            </Field.Field>

            {#if engine === "foundry"}
                {#if specs && specs.foundryInstalled}
                    <p class="text-xs text-emerald-600 dark:text-emerald-400">{m.settings_foundry_installed()}</p>
                {:else if showFoundryInstall}
                    <Field.Field>
                        <p class="text-xs text-muted-foreground @max-[420px]:hidden">{m.settings_foundry_unavailable()}</p>
                        <Button
                            size="sm"
                            loading={installing}
                            disabled={installing}
                            onclick={installFoundry}
                        >{installing ? m.settings_foundry_installing() : m.settings_foundry_install()}</Button>
                    </Field.Field>
                    {#if installLog.length > 0}
                        <div class="text-xs text-muted-foreground font-mono max-h-32 overflow-auto">
                            {#each installLog as line}<div>{line}</div>{/each}
                        </div>
                    {/if}
                {:else}
                    <p class="text-xs text-amber-600 dark:text-amber-400">{m.settings_foundry_unavailable()}</p>
                {/if}
            {/if}

            {#if engine === "llama"}
                <Field.Field>
                    <Field.Label>{m.settings_llama_gpu_layers()}</Field.Label>
                    <Input type="number" min="0" bind:value={gpuLayers} placeholder="auto" />
                </Field.Field>
                <Field.Field>
                    <Field.Label>{m.settings_llama_ctx_size()}</Field.Label>
                    <Input type="number" min="0" bind:value={ctxSize} placeholder="default" />
                </Field.Field>
            {/if}
        </Field.Group>
    </Field.Set>

    <div class="mt-4 flex items-center gap-3">
        <Button data-save loading={action.busy === "save"} disabled={action.busy !== null} onclick={save}>
            {m.settings_save()}
        </Button>
        {#if saved}<span class="text-sm text-emerald-600 dark:text-emerald-400" data-saved>{m.settings_saved()}</span>{/if}
    </div>
    {#if action.error}<p class="text-destructive mt-3 text-sm">{action.error}</p>{/if}

    <section class="mt-8 border-t pt-6">
        <h2 class="text-sm font-medium mb-3">{m.settings_runtime_explainer()}</h2>
        <RuntimeSteps {steps} />
    </section>
</main>
