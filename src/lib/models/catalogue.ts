import type { ModelDescriptor } from "./types";

const GB = 1024 * 1024 * 1024;

// Every GGUF below declares an architecture (`qwen35`, `qwen35moe`, `gemma4`, `mistral3`,
// `gpt-oss`) the bundled llama.cpp (`LLAMA_VERSION` in build.rs) can load. Check the GGUF's
// `general.architecture` against that build before adding a model; bump llama.cpp if it is missing.
// Foundry aliases were checked against `foundry model list` on 2026-11-09.
export const MODELS: ModelDescriptor[] = [
  {
    id: "qwen3.5-2b",
    label: "Qwen3.5 2B",
    paramsB: 2,
    ramFloorBytes: 4 * GB,
    llama: {
      repo: "unsloth/Qwen3.5-2B-GGUF",
      file: "Qwen3.5-2B-Q4_K_M.gguf",
      sha256: "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223",
      sizeBytes: 1_280_835_840,
    },
    // The text-only Foundry variant; GPU and CPU builds only, no NPU build.
    foundryAlias: "qwen3.5-2b-text",
    contextWindow: 262_144,
    reasoning: true,
  },
  {
    id: "qwen3.5-4b",
    label: "Qwen3.5 4B",
    paramsB: 4,
    ramFloorBytes: 8 * GB,
    llama: {
      repo: "unsloth/Qwen3.5-4B-GGUF",
      file: "Qwen3.5-4B-Q4_K_M.gguf",
      sha256: "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4",
      sizeBytes: 2_740_937_888,
    },
    foundryAlias: "qwen3.5-4b",
    contextWindow: 262_144,
    reasoning: true,
  },
  {
    id: "qwen3.5-9b",
    label: "Qwen3.5 9B",
    paramsB: 9,
    ramFloorBytes: 16 * GB,
    llama: {
      repo: "unsloth/Qwen3.5-9B-GGUF",
      file: "Qwen3.5-9B-Q4_K_M.gguf",
      sha256: "03b74727a860a56338e042c4420bb3f04b2fec5734175f4cb9fa853daf52b7e8",
      sizeBytes: 5_680_522_464,
    },
    foundryAlias: null,
    contextWindow: 262_144,
    reasoning: true,
  },
  {
    id: "gemma-4-12b-it",
    label: "Gemma 4 12B",
    paramsB: 12,
    ramFloorBytes: 16 * GB,
    llama: {
      repo: "unsloth/gemma-4-12b-it-GGUF",
      file: "gemma-4-12b-it-Q4_K_M.gguf",
      sha256: "0a270ec9fe6b34f4a0d33992b6135117b484ebc4766ab76b51d4ae8c457e4c42",
      sizeBytes: 7_121_861_440,
    },
    // Foundry ships only the smaller gemma-4-e2b-it.
    foundryAlias: null,
    contextWindow: 131_072,
    reasoning: true,
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
    // Foundry lists gpt-oss-20b for chat only, without tool calling, which coding agents need.
    foundryAlias: null,
    contextWindow: 131_072,
    reasoning: true,
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
    foundryAlias: null,
    contextWindow: 262_144,
    reasoning: false,
  },
  {
    id: "qwen3.8-27b",
    label: "Qwen3.8 27B",
    paramsB: 27,
    ramFloorBytes: 32 * GB,
    llama: {
      repo: "unsloth/Qwen3.8-27B-GGUF",
      file: "Qwen3.8-27B-UD-Q4_K_M.gguf",
      sha256: "322e194ff79741c7baa497c240f677f54b201b0efab44ca8e50f122b39123482",
      sizeBytes: 16_464_440_224,
    },
    foundryAlias: null,
    contextWindow: 262_144,
    reasoning: true,
  },
  {
    id: "qwen3.6-35b-a3b",
    label: "Qwen3.6 35B-A3B",
    paramsB: 35,
    ramFloorBytes: 32 * GB,
    llama: {
      repo: "unsloth/Qwen3.6-35B-A3B-GGUF",
      file: "Qwen3.6-35B-A3B-UD-Q4_K_M.gguf",
      sha256: "ac0e2c1189e055faa36eff361580e79c5bd6f8e76bffb4ce547f167d53e31a61",
      sizeBytes: 22_134_528_992,
    },
    // Mixture of experts with ~3B active parameters: larger download, but faster per token than
    // the dense 27B on CPU and integrated GPUs.
    foundryAlias: null,
    contextWindow: 262_144,
    reasoning: true,
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
      contextWindow: 32_768,
      reasoning: false,
    };
  } catch {
    return null;
  }
}
