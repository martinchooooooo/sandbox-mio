import type { ModelDescriptor } from "./types";

const GB = 1024 * 1024 * 1024;

export const MODELS: ModelDescriptor[] = [
  {
    id: "qwen2.5-1.5b-instruct",
    label: "Qwen2.5 1.5B Instruct",
    paramsB: 1.5,
    ramFloorBytes: 4 * GB,
    llama: {
      repo: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
      file: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
      sha256: "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e",
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
      sha256: "ce8becd58f350d8ae0ec3bbb201ab36f750ffab17ab6238f39292d12ab68ea06",
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
      sha256: "509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c",
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
      sha256: "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597",
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
      file: "gpt-oss-20b-MXFP4.gguf",
      sha256: "27cd6c432c7672cb812a92f611cf3ba7bbc35928262bb1e1253ff4ee6ae35901",
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
      sha256: "bfd11c8679c6b81eb43763505465d7dcfa72e460ab1c220ecc235a3efadd7f7f",
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
      sha256: "5ed60d0af4650a854b1755bd392f9aef4872643dc25a254bc68043fa638392a0",
      sizeBytes: 16_800_000_000,
    },
    // No Foundry catalog entry — llama engine only.
    foundryAlias: null,
  },
];

// Parses `MIO_TEST_MODEL` env var for e2e; returns null if not configured or invalid.
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
