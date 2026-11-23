import { invoke } from "@tauri-apps/api/core";
import { parseError, type AppErrorKind } from "$lib/errors";
import type { ActivityHeadline } from "$lib/activity/types";
import { miseKey } from "$lib/agents/catalogue";
import { installAgentInSandbox } from "$lib/agents/install";
import { agentsById } from "$lib/agents/preinstall";
import { reprovisionInstalledAgents } from "$lib/agents/reprovision";
import { MODELS, testModelFromEnv } from "$lib/models/catalogue";
import { downloadModel, modelsState, wakeModel } from "$lib/models/model.svelte";
import { recommendModels } from "$lib/models/recommend";
import type { MachineSpecs, ModelDescriptor } from "$lib/models/types";
import { recordMilestone } from "./milestones";
import { createSandbox, dismissCreate, pendingCreates } from "$lib/sandbox/lifecycle.svelte";
import { refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
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
 * First-run setup as one shared run of four jobs: machine, base image, model and the first sandbox.
 *
 * Module-scoped so the run survives the welcome flow's step changes: it is started in the
 * background while the user reads the story, and the cards of `SetupProgress.svelte` render this
 * same state instead of starting a second run of their own.
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
    /** The id the running server reports for the model, which provisioning points agents at. */
    servedModelId: string | null;
    error: string;
}

let model = $state<ModelRun>({ choice: null, downloadPct: null, downloading: false, starting: false, ready: false, skipped: false, servedModelId: null, error: "" });
let specs = $state<MachineSpecs | null>(null);
// True once the probe settled, success or failure, so a failed probe cannot hold the heads-up back.
let specsSettled = $state(false);
let testModel = $state<ModelDescriptor | null>(null);
let modelJob: Promise<void> | null = null;
// Reactive twin of `modelJob`, for the headline's "checking" state.
let modelJobActive = $state(false);

function trackModelJob(run: Promise<void>): Promise<void> {
    modelJobActive = true;
    return run.finally(() => {
        modelJob = null;
        modelJobActive = false;
    });
}

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
        specsSettled = true;
        return;
    }
    try {
        testModel = testModelFromEnv((await invoke<string | null>("test_model")) ?? undefined);
    } catch {
        testModel = null;
    }
    specsSettled = true;
}

/** Whether the machine probe has finished (even if it failed), so the offer is final. */
export function specsProbed(): boolean { return specsSettled; }

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
    modelJob ??= trackModelJob((async () => {
        // The probe may still be in flight or may have failed; re-running it is cheap and idempotent.
        await probeMachineSpecs();
        const offer = modelOffer();
        if (offer) return runModelSetup(offer.descriptor);
        // No offer must not dead-end the progress page: an error surfaces its skip path.
        model = { ...model, error: "Your computer could not be checked for a Model that fits." };
    })());
    return modelJob;
}

/**
 * Goes on without a model: never blocks onboarding (docs/features/fastest-time-to-wow/02_dev.md,
 * OQ-no-local-model). A running download keeps going; only the wait for it ends.
 */
export function skipModelSetup(): void {
    if (model.ready) return;
    model = { ...model, skipped: true, error: "" };
}

/** Retries a failed model job; the choice is kept, and with none the offer is probed again. */
export function retryModelSetup(): Promise<void> {
    if (!model.error) return Promise.resolve();
    if (!model.choice) {
        model = { ...model, error: "" };
        return startModelSetup();
    }
    modelJob ??= trackModelJob(runModelSetup(model.choice));
    return modelJob;
}

async function runModelSetup(descriptor: ModelDescriptor): Promise<void> {
    model = { choice: descriptor, downloadPct: null, downloading: false, starting: false, ready: false, skipped: false, servedModelId: null, error: "" };
    try {
        const onDisk = await downloadModel(descriptor, (pct) => {
            model.downloading = true;
            model.downloadPct = pct;
        });
        model.downloading = false;
        model.downloadPct = null;
        if (!onDisk) throw { kind: "Backend", message: modelsState().error };
        recordMilestone("model-downloaded");
        model.starting = true;
        await wakeModel(descriptor, "llama");
        recordMilestone("server-started");
        model.servedModelId = modelsState().status.modelId;
        model.ready = true;
    } catch (e) {
        model.error = parseError(e).message;
    } finally {
        model.downloading = false;
        model.starting = false;
    }
}

/** The first sandbox's name; the dash is required by `validate_name`, so "hello world" is copy only. */
export const FIRST_SANDBOX = "hello-world";

interface SandboxRun {
    busy: boolean;
    /** Current step text while the sandbox is created and its agent installed. */
    step: string;
    /** `hello-world` exists and its agent install (if any) has ended. */
    ready: boolean;
    /** pi is installed in the sandbox, by this run or an earlier one. */
    installed: boolean;
    /** An existing `hello-world` was found and is used as it is. */
    reused: boolean;
    error: string;
}

let sandbox = $state<SandboxRun>({ busy: false, step: "", ready: false, installed: false, reused: false, error: "" });
// Set by the heads-up's two choices; a replay never sets it, so a replay creates nothing.
let sandboxWanted = $state(false);
let sandboxJob: Promise<void> | null = null;

export function sandboxRun(): SandboxRun { return sandbox; }

/** Marks the first sandbox as part of this run's setup. Call only from the heads-up's choices. */
export function requestFirstSandbox(): void { sandboxWanted = true; }
export function firstSandboxWanted(): boolean { return sandboxWanted; }

// Generous: the agent install can take minutes on a slow connection.
const CREATE_TIMEOUT_MS = 20 * 60 * 1000;

function creationSettled(): { done: boolean; error: string } {
    const placeholder = pendingCreates().find((p) => p.name === FIRST_SANDBOX);
    if (placeholder) {
        sandbox.step = placeholder.step;
        return { done: false, error: placeholder.error };
    }
    return { done: sandboxesList().some((p) => p.name === FIRST_SANDBOX), error: "" };
}

/** Resolves with "" once `hello-world` is listed, or with the reason it will not be. */
function waitForCreate(): Promise<string> {
    return new Promise((resolve) => {
        const deadline = Date.now() + CREATE_TIMEOUT_MS;
        const timer = setInterval(() => {
            const state = creationSettled();
            if (state.error || state.done) {
                clearInterval(timer);
                resolve(state.error);
            } else if (Date.now() > deadline) {
                clearInterval(timer);
                resolve("Creating the sandbox took too long.");
            }
        }, 400);
    });
}

/**
 * Creates `hello-world` and installs pi into it, in parallel with the model job. Needs machine and
 * image ready; call it once they are. An existing `hello-world` is reused, never recreated
 * (docs/features/fastest-time-to-wow/02_dev.md, OQ-hello-world-special). With no model, no agent is
 * installed (OQ-no-local-model). A call while a run is in flight joins it; after a success or a
 * failure it does nothing, and `retryFirstSandbox` runs it again.
 */
export function startFirstSandbox(): Promise<void> {
    if (sandbox.ready || sandbox.error) return Promise.resolve();
    sandboxJob ??= runFirstSandbox().finally(() => {
        sandboxJob = null;
        sandbox.busy = false;
    });
    return sandboxJob;
}

/** Dismisses a failed create's placeholder and runs the job again. */
export function retryFirstSandbox(): Promise<void> {
    if (!sandbox.error || sandboxJob) return sandboxJob ?? Promise.resolve();
    dismissCreate(FIRST_SANDBOX);
    sandbox = { busy: false, step: "", ready: false, installed: false, reused: false, error: "" };
    return startFirstSandbox();
}

/** Gives a reused `hello-world` pi when it lacks it; a failure leaves `installed` false, never fails the job. */
async function ensurePiInstalled(): Promise<void> {
    const workspacePath = sandboxesList().find((p) => p.name === FIRST_SANDBOX)?.workspace_path;
    const pi = agentsById(["pi"])[0];
    if (!workspacePath || !pi) return;
    sandbox.step = "Installing pi"; // TODO(i18n)
    try {
        const packages = await invoke<string[]>("list_packages", { workspacePath });
        if (!pi.misePackages.every((p) => packages.includes(miseKey(p)))) {
            await installAgentInSandbox(FIRST_SANDBOX, workspacePath, pi);
            recordMilestone("pi-installed");
        }
        sandbox.installed = true;
    } catch {
        // The Manage tab still offers the manual install.
    }
}

async function runFirstSandbox(): Promise<void> {
    sandbox = { busy: true, step: "", ready: false, installed: false, reused: false, error: "" };
    try {
        await refreshSandboxState();
        if (sandboxesList().some((p) => p.name === FIRST_SANDBOX)) {
            sandbox.reused = true;
            if (!model.skipped) await ensurePiInstalled();
        } else {
            // Pre-install is explicit: first run never plans from host state, which could add Claude Code.
            // pi does not wait for the model: it is pointed at the model once both are done.
            const started = createSandbox({
                name: FIRST_SANDBOX,
                agents: model.skipped ? [] : ["pi"],
                // The handoff owns the open, so a cancel can really prevent it.
                autoOpen: false,
                onCreated: () => recordMilestone("sandbox-created"),
                onInstalled: (installed) => {
                    sandbox.installed = installed.includes("pi");
                    if (sandbox.installed) recordMilestone("pi-installed");
                },
            });
            // A refused start is fine only when a create or the sandbox itself is already there to wait on.
            if (!started && !creationSettled().done && !pendingCreates().some((p) => p.name === FIRST_SANDBOX)) {
                throw { kind: "Backend", message: "The sandbox could not be started." };
            }
            const failure = await waitForCreate();
            if (failure) throw { kind: "Backend", message: failure };
        }
        sandbox.ready = true;
    } catch (e) {
        sandbox.error = parseError(e).message;
        return;
    }
    // The model may have become ready while pi was installing, and its own re-pointing ran before
    // pi's packages were listed; doing it again here covers whichever finished first.
    if (sandbox.installed && model.ready && model.servedModelId) {
        try {
            await reprovisionInstalledAgents(model.servedModelId);
        } catch {
            // Agents are re-pointed again at the next launch.
        }
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
    return setupComplete(provider) && (model.ready || model.skipped) && sandbox.ready;
}

/**
 * The setup run in one line, for the activity toggle, in the tour's vocabulary. `null` while there
 * is nothing to report. `requireModel` is false in a replay, where the model is not part of setup.
 * TODO(i18n): every string below.
 */
export function setupHeadline(provider: ProviderId = "podman", requireModel = true): ActivityHeadline | null {
    const status = machineReadiness(provider);
    const machineDone = !status.probing && status.readiness.ready;

    // A replay never starts a model job, so it is finished once machine and image are.
    if (requireModel ? firstSandboxReady(provider) : setupComplete(provider)) {
        return { state: "done", title: "Everything is ready", detail: "Your first sandbox is next." };
    }
    if (machine.rebootMessage || podmanRebootRequired()) {
        return {
            state: "attention",
            title: "Your computer needs a restart",
            detail: "Restart it to finish getting things ready. You can keep reading first.",
        };
    }
    const anyError = Boolean(
        machine.error || status.error || image.error || (model.error && !model.skipped) || sandbox.error || (machineDone && image.probeError),
    );
    const doneCount = Number(machineDone) + Number(!image.probing && image.ready) + Number(model.ready || model.skipped) + Number(sandbox.ready);
    const progress = `${doneCount} of 4 ready`;
    // Checked before the running branches: a failure must not stay hidden behind a job that is still busy.
    if (anyError) {
        return {
            state: "attention",
            title: "Something needs a hand",
            detail: `${progress} · Something did not go through. You can try again on the setup page.`,
        };
    }
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
    if (sandbox.busy) {
        return { state: "running", title: "Getting things ready", detail: `${progress} · Preparing your first sandbox` };
    }
    if (backgroundActive || modelJobActive || (setupComplete(provider) && sandboxWanted && !sandbox.ready)) {
        return { state: "running", title: "Getting things ready", detail: "Checking your computer…" };
    }
    // Something is unfinished and nothing is running: a step waits for a click on the progress page.
    if (requireModel && sandboxWanted) {
        return { state: "attention", title: "Something needs a hand", detail: "A step is waiting for you on the setup page." };
    }
    return null;
}
