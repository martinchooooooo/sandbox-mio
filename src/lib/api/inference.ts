import { Channel, invoke } from "@tauri-apps/api/core";
import type {
    EngineKind,
    InferenceStatus,
    MachineSpecs,
    ModelDownloadEvent,
    RuntimeSetupEvent,
    SetupStep,
} from "$lib/models/types";

/** Streamed `install_foundry_local` output: the backend sends one `Output` event per CLI line. */
export type FoundryInstallEvent = { kind: string; line: string };

/** Probes once per app run; the backend caches the answer. */
export function detectMachineSpecs(): Promise<MachineSpecs> {
    return invoke<MachineSpecs>("detect_machine_specs");
}

export function redetectMachineSpecs(): Promise<MachineSpecs> {
    return invoke<MachineSpecs>("redetect_machine_specs");
}

export interface DownloadModelArgs {
    modelId: string;
    repo: string;
    file: string;
    sha256: string;
    onProgress: Channel<ModelDownloadEvent>;
}

export function downloadModel(args: DownloadModelArgs): Promise<void> {
    return invoke("download_model", { ...args });
}

export function downloadedModels(): Promise<string[]> {
    return invoke<string[]>("downloaded_models");
}

export function inferenceSetupSteps(engine: EngineKind): Promise<SetupStep[]> {
    return invoke<SetupStep[]>("inference_setup_steps", { engine });
}

export function startInference(
    engine: EngineKind,
    modelId: string,
    onProgress: Channel<RuntimeSetupEvent>,
): Promise<InferenceStatus> {
    return invoke<InferenceStatus>("start_inference", { engine, modelId, onProgress });
}

export function stopInference(): Promise<void> {
    return invoke("stop_inference");
}

export function inferenceStatus(): Promise<InferenceStatus> {
    return invoke<InferenceStatus>("inference_status");
}

export function installFoundryLocal(onProgress: Channel<FoundryInstallEvent>): Promise<void> {
    return invoke("install_foundry_local", { onProgress });
}

/** The model an e2e run pins via `MIO_TEST_MODEL`, else null. */
export function getTestModel(): Promise<string | null> {
    return invoke<string | null>("test_model");
}
