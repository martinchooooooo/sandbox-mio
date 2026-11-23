import { invoke, Channel } from "@tauri-apps/api/core";
import { parseError } from "$lib/errors";
import { reprovisionInstalledAgents } from "$lib/agents/reprovision";
import { testModelFromEnv } from "./catalogue";
import { describeModel, type ModelScales } from "./describe";
import { engineServes, fitFor, pickFor, recommendModels, type Fit } from "./recommend";
import type {
    EngineKind,
    InferenceStatus,
    MachineSpecs,
    ModelDescriptor,
    ModelDownloadEvent,
    RuntimeSetupEvent,
    SetupStep,
} from "./types";

// Module-scoped so progress survives navigation (docs/features/local-models-ux/02_dev.md).
type Busy = "download" | "wake" | "sleep" | "redetect";

/** The one Model job (download, wake, sleep) shared by the Models page and first-run onboarding. */
interface ModelsState {
    specs: MachineSpecs | null;
    testModel: ModelDescriptor | null;
    downloaded: string[];
    status: InferenceStatus;
    engine: EngineKind;
    /** Download progress 0-100 by model id; absent while not downloading. */
    downloadPct: Record<string, number>;
    wake: { modelId: string | null; steps: SetupStep[]; events: RuntimeSetupEvent[] };
    busy: Busy | null;
    /** The Model the current or last job acts on; null for a job on no Model. */
    jobModelId: string | null;
    /** Parsed message of the last failed job. */
    error: string;
}

const s = $state<ModelsState>({
    specs: null,
    testModel: null,
    downloaded: [],
    status: { running: false, engine: null, modelId: null, baseUrl: null },
    engine: "llama",
    downloadPct: {},
    wake: { modelId: null, steps: [], events: [] },
    busy: null,
    jobModelId: null,
    error: "",
});

const BUSY_MESSAGE = "Another Model job is still running.";

export function modelsState(): ModelsState { return s; }

/** Catalogue in ranking order, the test model first. */
export function entries(): ModelDescriptor[] {
    const ranked = s.specs ? recommendModels(s.specs).map((r) => r.descriptor) : [];
    return s.testModel ? [s.testModel, ...ranked] : ranked;
}

export interface ModelInfo {
    fit: Fit;
    scales: ModelScales;
}

/** The fit verdict and plain scales for a card; null until the machine probe has answered. */
export function infoOf(d: ModelDescriptor): ModelInfo | null {
    if (!s.specs) return null;
    const fit = fitFor(s.specs, d, s.engine);
    return { fit, scales: describeModel(d, s.specs, fit) };
}

export const isDownloaded = (d: ModelDescriptor) => s.downloaded.includes(d.id);

/** Only the built-in engine reads the downloaded file; Foundry fetches its own. */
export const needsDownload = (d: ModelDescriptor) => s.engine === "llama" && d.llama !== null && !isDownloaded(d);

// Foundry loads by its catalog alias; the llama engine by the catalogue id.
const targetOf = (d: ModelDescriptor, engine: EngineKind) =>
    engine === "foundry" ? (d.foundryAlias ?? d.id) : d.id;

export function isServing(d: ModelDescriptor): boolean {
    return s.status.running && s.status.modelId === targetOf(d, s.status.engine ?? s.engine);
}

export function pick(): ModelDescriptor | null {
    return s.specs ? pickFor(s.specs, s.engine, s.testModel) : null;
}

/** The Model the Your Model card shows: mid-job, else serving, else downloaded, else the pick, else the smallest. */
export function currentEntry(): ModelDescriptor | null {
    const all = entries();
    const jobId = s.busy ? s.jobModelId : null;
    return (
        all.find((d) => d.id === jobId) ??
        all.find(isServing) ??
        all.find((d) => isDownloaded(d) && engineServes(d, s.engine)) ??
        pick() ??
        all.filter((d) => engineServes(d, s.engine)).sort((a, b) => a.ramFloorBytes - b.ramFloorBytes)[0] ??
        null
    );
}

async function job(kind: Busy, modelId: string | null, run: () => Promise<void>): Promise<boolean> {
    if (s.busy) {
        s.error = BUSY_MESSAGE;
        return false;
    }
    s.busy = kind;
    s.jobModelId = modelId;
    s.error = "";
    try {
        await run();
        s.error = "";
        return true;
    } catch (e) {
        s.error = parseError(e).message;
        return false;
    } finally {
        s.busy = null;
    }
}

async function refreshStatus() {
    s.status = await invoke<InferenceStatus>("inference_status");
}

/** Reads every source of page state; safe to call on each mount. */
export async function loadModels(): Promise<void> {
    try {
        s.engine = (await invoke<{ inferenceEngine: EngineKind }>("get_settings")).inferenceEngine;
    } catch {
        /* default llama */
    }
    try {
        s.specs = await invoke<MachineSpecs>("detect_machine_specs");
    } catch (e) {
        s.error = parseError(e).message;
    }
    try {
        s.testModel = testModelFromEnv((await invoke<string | null>("test_model")) ?? undefined);
    } catch {
        s.testModel = null;
    }
    try {
        s.downloaded = await invoke<string[]>("downloaded_models");
    } catch {
        s.downloaded = [];
    }
    try {
        await refreshStatus();
    } catch {
        /* keep the last status */
    }
    if (s.busy !== "wake") {
        try {
            s.wake.steps = await invoke<SetupStep[]>("inference_setup_steps", { engine: s.engine });
        } catch {
            /* the checklist stays empty */
        }
    }
}

export function redetect(): Promise<boolean> {
    return job("redetect", null, async () => {
        s.specs = await invoke<MachineSpecs>("redetect_machine_specs");
    });
}

async function fetchModel(d: ModelDescriptor, onProgress?: (pct: number) => void) {
    const src = d.llama;
    if (!src) throw { kind: "Backend", message: "This Model has no download for the built-in engine." };
    s.downloaded = await invoke<string[]>("downloaded_models");
    if (isDownloaded(d)) return;
    s.downloadPct[d.id] = 0;
    onProgress?.(0);
    try {
        const channel = new Channel<ModelDownloadEvent>();
        channel.onmessage = (e) => {
            if (e.kind !== "Progress" || e.total_bytes <= 0) return;
            const pct = Math.floor((e.downloaded_bytes / e.total_bytes) * 100);
            s.downloadPct[d.id] = pct;
            onProgress?.(pct);
        };
        await invoke("download_model", {
            modelId: d.id,
            repo: src.repo,
            file: src.file,
            sha256: src.sha256,
            onProgress: channel,
        });
    } finally {
        delete s.downloadPct[d.id];
        s.downloaded = await invoke<string[]>("downloaded_models");
    }
}

/** Resolves to whether the Model is now on disk; the reason for a failure lands in `error`. */
export async function downloadModel(d: ModelDescriptor, onProgress?: (pct: number) => void): Promise<boolean> {
    await job("download", d.id, () => fetchModel(d, onProgress));
    return isDownloaded(d);
}

async function startServer(d: ModelDescriptor, engine: EngineKind) {
    const target = targetOf(d, engine);
    s.wake = { modelId: d.id, steps: [], events: [] };
    try {
        s.wake.steps = await invoke<SetupStep[]>("inference_setup_steps", { engine });
    } catch {
        /* the checklist stays empty */
    }
    // Starting again would stop a healthy server and drop every sandbox's forward to it.
    await refreshStatus();
    if (!(s.status.running && s.status.modelId === target)) {
        const channel = new Channel<RuntimeSetupEvent>();
        channel.onmessage = (e) => (s.wake.events = [...s.wake.events, e]);
        await invoke<InferenceStatus>("start_inference", { engine, modelId: target, onProgress: channel });
        // Ready means the server answers as running now, not that the start's reply said so.
        await refreshStatus();
    }
    if (!s.status.running || s.status.modelId !== target) {
        throw { kind: "Backend", message: "The Model did not start." };
    }
    // Best effort after ready; agents are re-pointed again at the next launch.
    reprovisionInstalledAgents(s.status.modelId).catch(() => {});
}

/** Throws the failure's message, which is also kept in `error`. */
export async function wakeModel(d: ModelDescriptor, engine: EngineKind = s.engine): Promise<void> {
    if (!(await job("wake", d.id, () => startServer(d, engine)))) throw { kind: "Backend", message: s.error };
}

export function downloadAndWake(d: ModelDescriptor): Promise<boolean> {
    return job("download", d.id, async () => {
        if (needsDownload(d)) await fetchModel(d);
        s.busy = "wake";
        await startServer(d, s.engine);
    });
}

export function sleep(): Promise<boolean> {
    return job("sleep", null, async () => {
        await invoke("stop_inference");
        s.wake = { ...s.wake, modelId: null, events: [] };
        await refreshStatus();
    });
}
