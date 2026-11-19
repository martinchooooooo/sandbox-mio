import { invoke, Channel } from "@tauri-apps/api/core";
import { parseError, type AppErrorKind } from "$lib/errors";
import type { ActivityHeadline } from "$lib/activity/types";
import { reprovisionInstalledAgents } from "$lib/agents/reprovision";
import { MODELS, testModelFromEnv } from "$lib/models/catalogue";
import { recommendModels } from "$lib/models/recommend";
import type { InferenceStatus, MachineSpecs, ModelDescriptor, ModelDownloadEvent } from "$lib/models/types";
import { recordMilestone } from "./milestones";
import { prepareMachine } from "$lib/machine/readiness";
import { ensureMachineReadiness, machineReadiness } from "$lib/machine/readiness.svelte";
import type { MachineSetupEvent, ProviderId } from "$lib/machine/types";
import {
    checkPodman,
    podmanNeedsSetup,
    podmanRebootRequired,
    podmanUnavailable,
    setupError,
    setupPodman,
    startError,
    startPodman,
} from "$lib/podman/setup.svelte";

/**
 * First-run setup (machine, then base image) as one shared run.
 *
 * Module-scoped so the run survives the welcome flow's step changes: it is started in the
 * background while the user reads the story, and the two setup cards render this same state
 * instead of starting a second run of their own.
 */
interface MachineRun {
    busy: boolean;
    events: MachineSetupEvent[];
    error: string;
    rebootMessage: string;
}

interface ImageRun {
    probing: boolean;
    ready: boolean;
    busy: boolean;
    error: string;
    probeError: string;
    probeErrorKind: AppErrorKind | null;
}

let machine = $state<MachineRun>({ busy: false, events: [], error: "", rebootMessage: "" });
let image = $state<ImageRun>({
    probing: true,
    ready: false,
    busy: false,
    error: "",
    probeError: "",
    probeErrorKind: null,
});

let background: Promise<void> | null = null;
// Reactive twin of `background`, for the headline's "checking" state before either step starts.
let backgroundActive = $state(false);

export function machineRun(): MachineRun { return machine; }
export function imageRun(): ImageRun { return image; }

interface ModelRun {
    /** The model the run downloads and serves; null until the heads-up confirms. */
    choice: ModelDescriptor | null;
    /** Download progress, 0-100; null while no download is running. */
    downloadPct: number | null;
    downloading: boolean;
    starting: boolean;
    /** The model is downloaded and the server is running it. */
    ready: boolean;
    /** The user chose to go on without a model; onboarding then creates the sandbox with no agent. */
    skipped: boolean;
    error: string;
}

let model = $state<ModelRun>({ choice: null, downloadPct: null, downloading: false, starting: false, ready: false, skipped: false, error: "" });
let specs = $state<MachineSpecs | null>(null);
let testModel = $state<ModelDescriptor | null>(null);
let modelJob: Promise<void> | null = null;

export function modelRun(): ModelRun { return model; }

/**
 * Probes the host once, so the heads-up has its model offer on the first paint and the probe runs
 * before any heavy job lowers free RAM (docs/features/fastest-time-to-wow/02_dev.md, Resolved
 * Decision on OQ-ram-basis). Safe to call repeatedly; failures leave the offer empty.
 */
export async function probeMachineSpecs(): Promise<void> {
    if (specs) return;
    try {
        specs = await invoke<MachineSpecs>("detect_machine_specs");
    } catch {
        return;
    }
    try {
        testModel = testModelFromEnv((await invoke<string | null>("test_model")) ?? undefined);
    } catch {
        testModel = null;
    }
}

export interface ModelOffer {
    descriptor: ModelDescriptor;
    /** True when no model rates "recommended", so the offer is the smallest one and needs "Try anyway". */
    lowRam: boolean;
}

/** The model first run offers; null while the probe has not finished or no model has a GGUF. */
export function modelOffer(): ModelOffer | null {
    if (!specs) return null;
    if (testModel) return { descriptor: testModel, lowRam: false };
    const top = recommendModels(specs).find((r) => r.descriptor.llama);
    if (top?.rating === "recommended") return { descriptor: top.descriptor, lowRam: false };
    const smallest = MODELS.filter((d) => d.llama).sort((a, b) => a.ramFloorBytes - b.ramFloorBytes)[0];
    return smallest ? { descriptor: smallest, lowRam: true } : null;
}

/**
 * Downloads (unless already on disk) and serves the offered model, in parallel with machine and
 * image setup. Call only from the heads-up's confirm action: a skip, a replay or the tour's first
 * page must never start a download. A call while a run is in flight joins that run.
 */
export function startModelSetup(): Promise<void> {
    modelJob ??= (async () => {
        // The probe may still be in flight or may have failed; re-running it is cheap and idempotent.
        await probeMachineSpecs();
        const offer = modelOffer();
        if (offer) return runModelSetup(offer.descriptor);
        // No offer must not dead-end the progress page: an error surfaces its skip path.
        model = { ...model, error: "Your computer could not be checked for a Model that fits." };
    })().finally(() => {
        modelJob = null;
    });
    return modelJob;
}

/** Goes on without a model: never blocks onboarding (docs/features/fastest-time-to-wow/02_dev.md, OQ-no-local-model). */
export function skipModelSetup(): void {
    if (model.ready || model.downloading || model.starting) return;
    model = { ...model, skipped: true, error: "" };
}

/** Retries a failed model job; the choice is kept, and with none the offer is probed again. */
export function retryModelSetup(): Promise<void> {
    if (!model.error) return Promise.resolve();
    if (!model.choice) {
        model = { ...model, error: "" };
        return startModelSetup();
    }
    modelJob ??= runModelSetup(model.choice).finally(() => {
        modelJob = null;
    });
    return modelJob;
}

async function runModelSetup(descriptor: ModelDescriptor): Promise<void> {
    model = { choice: descriptor, downloadPct: null, downloading: false, starting: false, ready: false, skipped: false, error: "" };
    try {
        const source = descriptor.llama;
        if (!source) throw { kind: "Backend", message: "This model has no download for the built-in engine." };
        const onDisk = await invoke<string[]>("downloaded_models");
        if (!onDisk.includes(descriptor.id)) {
            model.downloading = true;
            model.downloadPct = 0;
            const channel = new Channel<ModelDownloadEvent>();
            channel.onmessage = (e) => {
                if (e.kind === "Progress" && e.total_bytes > 0) {
                    model.downloadPct = Math.floor((e.downloaded_bytes / e.total_bytes) * 100);
                }
            };
            await invoke("download_model", {
                modelId: descriptor.id,
                repo: source.repo,
                file: source.file,
                sha256: source.sha256,
                onProgress: channel,
            });
            model.downloading = false;
            model.downloadPct = null;
        }
        recordMilestone("model-downloaded");
        model.starting = true;
        const status = await invoke<InferenceStatus>("start_inference", {
            engine: "llama",
            modelId: descriptor.id,
            onProgress: new Channel(),
        });
        if (!status.running) throw { kind: "Backend", message: "The model did not start." };
        recordMilestone("server-started");
        if (status.modelId) await reprovisionInstalledAgents(status.modelId);
        model.ready = true;
    } catch (e) {
        model.error = parseError(e).message;
    } finally {
        model.downloading = false;
        model.starting = false;
    }
}

/** Brings `provider`'s machine online. Joins nothing: a call while one is running is ignored. */
export async function runMachineSetup(provider: ProviderId): Promise<void> {
    if (machine.busy) return;
    machine = { busy: true, events: [], error: "", rebootMessage: "" };
    const result = await prepareMachine(provider, (e) => {
        machine.events = [...machine.events, e];
        if (e.kind === "rebootRequired") machine.rebootMessage = e.message;
    });
    if (result.ok) {
        await ensureMachineReadiness(provider, true);
    } else if (!result.rebootRequired) {
        machine.error = result.message;
    }
    machine.busy = false;
}

/** Whether the base image already exists, without building it. */
export async function probeImage(): Promise<void> {
    try {
        image.ready = await invoke<boolean>("base_image_ready");
        image.probeError = "";
        image.probeErrorKind = null;
    } catch (e) {
        const err = parseError(e);
        image.probeError = err.message;
        image.probeErrorKind = err.kind;
    } finally {
        image.probing = false;
    }
}

/**
 * Builds the base image. Provisioning the machine is chained in rather than required first: the
 * image cannot build without it (docs/features/welcome-onboarding/02_dev.md, Resolved Decisions).
 */
export async function runImageDownload(): Promise<void> {
    if (image.busy) return;
    image.busy = true;
    image.error = "";
    try {
        await checkPodman();
        if (podmanUnavailable()) {
            const started = podmanNeedsSetup() ? await setupPodman() : await startPodman();
            // The reboot case is rendered from the podman store, which keeps the structured kind.
            if (!started) throw { kind: "Backend", message: setupError() || startError() };
        }
        await invoke("build_base_image");
        await probeImage();
    } catch (e) {
        image.error = parseError(e).message;
    } finally {
        image.busy = false;
    }
}

/**
 * Runs whatever setup is still missing, machine first, without waiting for a click. Stops at the
 * first step that needs the user (a failure or a restart), leaving it to the setup cards.
 * A call while a run is in flight joins that run.
 */
export function startBackgroundSetup(provider: ProviderId = "podman"): Promise<void> {
    backgroundActive = true;
    background ??= (async () => {
        await ensureMachineReadiness(provider);
        if (machineReadiness(provider).error) return;
        if (!machineReadiness(provider).readiness.ready) await runMachineSetup(provider);
        if (!machineReadiness(provider).readiness.ready) return;
        await probeImage();
        if (!image.ready && !image.probeError) await runImageDownload();
    })().finally(() => {
        background = null;
        backgroundActive = false;
    });
    return background;
}

/** Whether both steps are complete, so the user can leave onboarding. */
export function setupComplete(provider: ProviderId = "podman"): boolean {
    const status = machineReadiness(provider);
    return !status.probing && status.readiness.ready && !image.probing && image.ready;
}

/** Whether machine, image and model are all done, so the first sandbox can be built. */
export function firstSandboxReady(provider: ProviderId = "podman"): boolean {
    return setupComplete(provider) && (model.ready || model.skipped);
}

/**
 * The setup run in one line, for the activity toggle, in the tour's vocabulary. `null` while there
 * is nothing to report.
 * TODO(i18n): every string below.
 */
export function setupHeadline(provider: ProviderId = "podman"): ActivityHeadline | null {
    const status = machineReadiness(provider);
    const machineDone = !status.probing && status.readiness.ready;

    if (firstSandboxReady(provider)) {
        return { state: "done", title: "Everything is ready", detail: "Your first sandbox is next." };
    }
    if (machine.rebootMessage || podmanRebootRequired()) {
        return {
            state: "attention",
            title: "Your computer needs a restart",
            detail: "Restart it to finish getting things ready. You can keep reading first.",
        };
    }
    const doneCount = Number(machineDone) + Number(!image.probing && image.ready) + Number(model.ready || model.skipped);
    const progress = `${doneCount} of 3 ready`;
    if (machine.busy) {
        return { state: "running", title: "Getting things ready", detail: `${progress} · Preparing a pretend computer for your sandboxes` };
    }
    if (model.downloading) {
        const pct = model.downloadPct === null ? "" : ` · ${model.downloadPct}%`;
        return { state: "running", title: "Getting things ready", detail: `${progress} · Downloading the Model${pct}` };
    }
    if (model.starting) {
        return { state: "running", title: "Getting things ready", detail: `${progress} · Waking the Model up` };
    }
    if (image.busy) {
        return { state: "running", title: "Getting things ready", detail: `${progress} · Packing what sandboxes need` };
    }
    if (machine.error || status.error || image.error || model.error || (machineDone && image.probeError)) {
        return {
            state: "attention",
            title: "Something needs a hand",
            detail: "Something did not go through. You can try again at the end of the tour.",
        };
    }
    if (backgroundActive || modelJob) return { state: "running", title: "Getting things ready", detail: "Checking your computer…" };
    return null;
}
