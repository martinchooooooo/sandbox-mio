/** Probe result from the `detect_machine_specs` command (mirrors Rust `MachineSpecs`). */
export interface MachineSpecs {
  cpuCores: number;
  totalRamBytes: number;
  freeRamBytes: number;
  gpuNames: string[];
  hasNpu: boolean;
  arch: string;
  foundryInstalled: boolean;
  foundryInstallable: boolean;
}

/** Which inference engine serves a model (mirrors Rust `EngineKind`). */
export type EngineKind = "llama" | "foundry";

/** GGUF download source for the llama engine. */
export interface LlamaModelSource {
  repo: string; // HF repo id
  file: string; // GGUF filename
  sha256: string;
  sizeBytes: number;
}

/** One catalogue entry, carrying both a llama GGUF descriptor and a Foundry alias. */
export interface ModelDescriptor {
  id: string;
  label: string;
  paramsB: number; // billions of parameters
  ramFloorBytes: number; // free RAM required for a "recommended" rating
  /** GGUF source for the llama engine. `null` when no GGUF ships for this model. */
  llama: LlamaModelSource | null;
  /** Foundry catalog alias, or `null` when Foundry has no matching model. */
  foundryAlias: string | null;
}

export type ModelRating = "recommended" | "slow" | "wont-fit";

/** Live runtime-setup step list returned by `runtime_setup_steps` (mirrors Rust `SetupStep`). */
export interface SetupStep {
  id: string;
}

/** Streamed download progress (mirrors Rust `ModelDownloadEvent`). */
export type ModelDownloadEvent =
  | { kind: "Started"; total_bytes: number }
  | { kind: "Progress"; downloaded_bytes: number; total_bytes: number }
  | { kind: "Verifying" }
  | { kind: "Done" };

/** Streamed runtime-setup progress (mirrors Rust `RuntimeSetupEvent`). */
export type RuntimeSetupEvent =
  | { kind: "stepStart"; id: string }
  | { kind: "stepProgress"; id: string; detail: string }
  | { kind: "stepDone"; id: string; detail: string | null }
  | { kind: "stepSkipped"; id: string; reason: string }
  | { kind: "stepFailed"; id: string; error: string }
  | { kind: "ready"; base_url: string; model_id: string };

/** Live server status (mirrors Rust `InferenceStatus`). */
export interface InferenceStatus {
  running: boolean;
  engine: EngineKind | null;
  modelId: string | null;
  baseUrl: string | null;
}
