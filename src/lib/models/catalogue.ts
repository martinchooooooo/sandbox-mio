import type { ModelDescriptor } from "./types";

const GB = 1024 * 1024 * 1024;

/**
 * Windows v1 model catalogue. Each entry pairs a llama GGUF source with a Foundry alias and a
 * numeric RAM floor (free host RAM required for a "recommended" rating). Co-located with the
 * `recommendModels` engine that ranks it.
 *
 * TODO(bring-up): `sha256` values are placeholders until each GGUF is first downloaded and hashed
 * (Phase D). `download_gguf` skips the checksum check while `sha256` is empty so the bring-up
 * download that establishes the real hash succeeds; fill them in afterwards.
 */
export const MODELS: ModelDescriptor[] = [
  {
    id: "qwen2.5-1.5b-instruct",
    label: "Qwen2.5 1.5B Instruct",
    paramsB: 1.5,
    ramFloorBytes: 4 * GB,
    llama: {
      repo: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
      file: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
      sha256: "",
      sizeBytes: 1_100_000_000,
    },
    foundryAlias: "qwen2.5-1.5b",
  },
  {
    id: "phi-4-mini-reasoning",
    label: "Phi-4 mini reasoning",
    paramsB: 3.8,
    ramFloorBytes: 8 * GB,
    llama: {
      repo: "bartowski/microsoft_Phi-4-mini-reasoning-GGUF",
      file: "microsoft_Phi-4-mini-reasoning-Q4_K_M.gguf",
      sha256: "",
      sizeBytes: 2_500_000_000,
    },
    foundryAlias: "phi-4-mini-reasoning",
  },
  {
    id: "qwen2.5-coder-7b-instruct",
    label: "Qwen2.5 Coder 7B Instruct",
    paramsB: 7,
    ramFloorBytes: 16 * GB,
    llama: {
      repo: "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF",
      file: "qwen2.5-coder-7b-instruct-q4_k_m.gguf",
      sha256: "",
      sizeBytes: 4_680_000_000,
    },
    // No Foundry catalog entry as of the 2026-06-29 spike — llama engine only.
    foundryAlias: null,
  },
  {
    id: "qwen3-4b-instruct",
    label: "Qwen3 4B Instruct",
    paramsB: 4,
    ramFloorBytes: 8 * GB,
    llama: {
      repo: "unsloth/Qwen3-4B-Instruct-2507-GGUF",
      file: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
      sha256: "",
      sizeBytes: 2_500_000_000,
    },
    // No Foundry catalog entry — llama engine only.
    foundryAlias: null,
  },
  {
    id: "gpt-oss-20b",
    label: "GPT-OSS 20B",
    paramsB: 20,
    ramFloorBytes: 24 * GB,
    llama: {
      repo: "ggml-org/gpt-oss-20b-GGUF",
      file: "gpt-oss-20b-mxfp4.gguf",
      sha256: "",
      sizeBytes: 12_100_000_000,
    },
    // No Foundry catalog entry — llama engine only.
    foundryAlias: null,
  },
  {
    id: "devstral-small-2-24b",
    label: "Devstral Small 2 24B",
    paramsB: 24,
    ramFloorBytes: 32 * GB,
    llama: {
      repo: "bartowski/mistralai_Devstral-Small-2-24B-Instruct-2512-GGUF",
      file: "mistralai_Devstral-Small-2-24B-Instruct-2512-Q4_K_M.gguf",
      sha256: "",
      sizeBytes: 14_330_000_000,
    },
    // No Foundry catalog entry — llama engine only.
    foundryAlias: null,
  },
  {
    id: "qwen3.6-27b",
    label: "Qwen3.6 27B",
    paramsB: 27,
    ramFloorBytes: 32 * GB,
    llama: {
      repo: "unsloth/Qwen3.6-27B-GGUF",
      file: "Qwen3.6-27B-Q4_K_M.gguf",
      sha256: "",
      sizeBytes: 16_800_000_000,
    },
    // No Foundry catalog entry — llama engine only.
    foundryAlias: null,
  },
];

/**
 * A test-only model injected via the `MIO_TEST_MODEL` env var (parsed by the e2e harness and
 * passed to the route through a global), kept out of the shipped catalogue so a ~100 MB test GGUF
 * never leaks into the product. Returns `null` when no test model is configured.
 */
export function testModelFromEnv(raw: string | undefined): ModelDescriptor | null {
  if (!raw) return null;
  try {
    const m = JSON.parse(raw) as {
      id?: string;
      label?: string;
      repo: string;
      file: string;
      sha256: string;
      sizeBytes: number;
    };
    return {
      id: m.id ?? "test-model",
      label: m.label ?? "Test model",
      paramsB: 0.1,
      ramFloorBytes: GB,
      llama: { repo: m.repo, file: m.file, sha256: m.sha256, sizeBytes: m.sizeBytes },
      foundryAlias: null,
    };
  } catch {
    return null;
  }
}
