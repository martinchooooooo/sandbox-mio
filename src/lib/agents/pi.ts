import { MODELS } from "$lib/models/catalogue";
import { LOCAL_LLM_BASE_URL, LOCAL_LLM_PROVIDER_ID } from "./local-llm";

export const PI_CONFIG_DIR_CONTAINER = "/root/work/.mio/pi-agent";
export const PI_CONFIG_ENV = "PI_CODING_AGENT_DIR";
export const PI_CONFIG_REL_PATH = ".mio/pi-agent/models.json";

/** The pi release the catalogue installs and the native chat's RPC adapter is written against
 * (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 6). Bumping it means re-recording
 * the replay fixtures in `src-tauri/src/agent_host/fixtures/`. */
export const PI_PINNED_VERSION = "1.5.0";

/** Mirrors `DEFAULT_LLAMA_CTX_SIZE` in `src-tauri/src/inference/llama.rs`; keep the two equal. */
export const DEFAULT_LLAMA_CTX_SIZE = 32_768;

/** Generates the provider config to keep pi's model map in sync with the inference server.
 * The dummy `apiKey` is required — OpenAI-compatible clients always send an Authorization
 * header; llama-server ignores its value. pi wants `models` as an array, not a map.
 * `servedContext` caps each model's `contextWindow` at the server's context size; `null` (the
 * Foundry engine, whose served size is not probed) writes the catalogue value. */
export function buildPiLocalLlmProvider(defaultModelId: string, servedContext: number | null): {
  providerId: string;
  providerJson: unknown;
  defaultModel: string;
} {
  return {
    providerId: LOCAL_LLM_PROVIDER_ID,
    providerJson: {
      baseUrl: LOCAL_LLM_BASE_URL,
      api: "openai-completions",
      apiKey: "sk-local",
      models: MODELS.map((model) => ({
        id: model.id,
        name: model.label,
        contextWindow: servedContext === null ? model.contextWindow : Math.min(model.contextWindow, servedContext),
        reasoning: model.reasoning,
      })),
    },
    defaultModel: defaultModelId,
  };
}
