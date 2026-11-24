<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Field from "$lib/components/ui/field";
    import { Text } from "$lib/components/ui/text";
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
    import AdvancedSection from "$lib/components/AdvancedSection.svelte";
    import ResourceUsageCard from "$lib/settings/ResourceUsageCard.svelte";
    import { Switch } from "$lib/components/ui/switch";
    import { restartOnboarding } from "$lib/settings/onboarding.svelte";
    import { destroySandbox } from "$lib/sandbox/lifecycle.svelte";
    import { refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
    import { FIRST_SANDBOX } from "$lib/onboarding/handoff.svelte";
    import { closeSandbox } from "$lib/workspace/navigation.svelte";
    import { ENGINE_NAMES, PAGE } from "$lib/models/copy";

    const ENGINE_CHOICES: { id: EngineKind; label: string; detail: string }[] = [
        { id: "llama", label: m.settings_engine_llama_label(), detail: ENGINE_NAMES.llama },
        { id: "foundry", label: m.settings_engine_foundry_label(), detail: "Foundry Local" },
    ];

    function cycleLocale() {
        const current = getLocale();
        const idx = locales.indexOf(current);
        setLocale(locales[(idx + 1) % locales.length]);
    }

    interface AppSettings {
        inferenceEngine: EngineKind;
        llamaGpuLayers: number | null;
        llamaCtxSize: number | null;
        // `set_settings` replaces the whole file, and any field this page omits comes back as its
        // serde default — which would silently un-onboard the user (`onboardingCompleted`).
        [key: string]: unknown;
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

    // The last full object read from `get_settings`, so `save()` can round-trip the fields this
    // page does not edit instead of dropping them.
    let loaded: AppSettings = {
        inferenceEngine: "llama",
        llamaGpuLayers: null,
        llamaCtxSize: null,
    };

    async function loadSteps(forEngine: EngineKind) {
        try {
            steps = await invoke<SetupStep[]>("inference_setup_steps", { engine: forEngine });
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
            ...loaded,
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
            loaded = s;
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

    let machineReset = $state(false);

    async function resetPodmanMachine() {
        machineReset = false;
        if (await action.run("reset-podman-machine", () => invoke("reset_stuck_podman_machine"))) {
            machineReset = true;
        }
    }

    // Off by default: the sandbox may hold the user's work.
    let deleteFirstSandbox = $state(false);

    async function restartOnboardingFlow() {
        await action.run("restart-onboarding", async () => {
            if (deleteFirstSandbox) {
                await refreshSandboxState();
                // Without the sandbox, the handoff creates it afresh instead of opening the old one.
                if (sandboxesList().some((p) => p.name === FIRST_SANDBOX)) {
                    const confirmForce = (message: string) => confirm(m.settings_debugging_restart_onboarding_force_confirm({ message }));
                    if (!(await destroySandbox(FIRST_SANDBOX, { confirmForce }))) return;
                    // The reload below resets webview state, but the sandbox's native panes live on the Rust side.
                    await closeSandbox(FIRST_SANDBOX);
                }
            }
            await restartOnboarding();
        });
    }
</script>

<main class="mx-auto w-full max-w-3xl p-6 pb-16 @max-[420px]:p-3">
    <Text as="h1" class="mb-8">{m.settings_title()}</Text>

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

    <AdvancedSection data-advanced-section="tuning">
    <Field.Set>
        <Field.Legend>{m.settings_advanced()}</Field.Legend>
        <Field.Group>
            <Field.Field>
                <Field.Label>{m.settings_engine_label()}</Field.Label>
                <div class="flex flex-col gap-2" data-active-engine={engine}>
                    {#each ENGINE_CHOICES as choice (choice.id)}
                        <Button
                            data-engine={choice.id}
                            variant={engine === choice.id ? "default" : "outline"}
                            onclick={() => selectEngine(choice.id)}
                            class="h-auto flex-col items-start justify-start gap-0 py-2 text-left"
                        >
                            {choice.label}
                            <span class="text-xs font-normal opacity-80">{choice.detail}</span>
                        </Button>
                    {/each}
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
                    <Input type="number" min="0" bind:value={gpuLayers} placeholder={m.settings_llama_gpu_layers_placeholder()} />
                </Field.Field>
                <Field.Field>
                    <Field.Label>{m.settings_llama_ctx_size()}</Field.Label>
                    <Input type="number" min="0" bind:value={ctxSize} placeholder={m.settings_llama_ctx_size_placeholder()} />
                </Field.Field>
            {/if}

            <Field.Field>
                <Field.Label>{PAGE.wakeSteps}</Field.Label>
                <RuntimeSteps {steps} />
            </Field.Field>
        </Field.Group>
    </Field.Set>
    </AdvancedSection>

    <AdvancedSection label={m.settings_troubleshooting_label()} data-advanced-section="troubleshooting" class="mt-4">
    <Field.Set>
        <Field.Legend>{m.settings_debugging()}</Field.Legend>
        <Field.Group>
            <Field.Field>
                <Field.Label>{m.settings_debugging_reset_machine_label()}</Field.Label>
                <p class="text-xs text-muted-foreground">{m.settings_debugging_reset_machine_description()}</p>
                <Button
                    data-reset-podman-machine
                    size="sm"
                    variant="outline"
                    loading={action.busy === "reset-podman-machine"}
                    disabled={action.busy !== null}
                    onclick={resetPodmanMachine}
                >{m.settings_debugging_reset_machine_button()}</Button>
                {#if machineReset}<p data-reset-podman-machine-done class="text-xs text-emerald-600 dark:text-emerald-400">{m.settings_debugging_reset_machine_done()}</p>{/if}
            </Field.Field>
            <Field.Field>
                <Field.Label>{m.settings_debugging_restart_onboarding_label()}</Field.Label>
                <p class="text-xs text-muted-foreground">{m.settings_debugging_restart_onboarding_description()}</p>
                <Field.Field orientation="horizontal">
                    <Switch
                        id="restart-onboarding-delete-sandbox"
                        data-restart-onboarding-delete-sandbox
                        checked={deleteFirstSandbox}
                        onCheckedChange={(v) => (deleteFirstSandbox = v)}
                    />
                    <Field.Label for="restart-onboarding-delete-sandbox">
                        {m.settings_debugging_restart_onboarding_delete_sandbox({ name: FIRST_SANDBOX })}
                    </Field.Label>
                </Field.Field>
                <Button
                    data-settings-restart-onboarding
                    size="sm"
                    variant="outline"
                    loading={action.busy === "restart-onboarding"}
                    disabled={action.busy !== null}
                    onclick={restartOnboardingFlow}
                >{m.settings_debugging_restart_onboarding_button()}</Button>
            </Field.Field>
        </Field.Group>
    </Field.Set>
    </AdvancedSection>

    <div class="mt-4 flex items-center gap-3">
        <Button data-save loading={action.busy === "save"} disabled={action.busy !== null} onclick={save}>
            {m.settings_save()}
        </Button>
        {#if saved}<span class="text-sm text-emerald-600 dark:text-emerald-400" data-saved>{m.settings_saved()}</span>{/if}
    </div>
    {#if action.error}<p class="text-destructive mt-3 text-sm">{action.error}</p>{/if}

    <section class="mt-8">
        <ResourceUsageCard />
    </section>
</main>
