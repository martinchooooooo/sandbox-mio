import { MODELS } from "$lib/models/catalogue";
import { FALLBACK_MODEL_ID, LOCAL_LLM_BASE_URL, LOCAL_LLM_PROVIDER_ID } from "./local-llm";

/** Name pi reports for the local model; the chat header shows a translated label instead. */
export const LOCAL_LLM_MODEL_NAME = "Model";

export const PI_CONFIG_DIR_CONTAINER = "/root/work/.mio/pi-agent";
export const PI_CONFIG_ENV = "PI_CODING_AGENT_DIR";
export const PI_CONFIG_REL_PATH = ".mio/pi-agent/models.json";

/** The pi release the catalogue installs and the native chat's RPC adapter is written against
 * (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 6). Bumping it means re-recording
 * the replay fixtures in `src-tauri/src/agent_host/fixtures/`. */
export const PI_PINNED_VERSION = "1.5.0";

/** Mirrors `DEFAULT_LLAMA_CTX_SIZE` in `src-tauri/src/inference/llama.rs`; keep the two equal. */
export const DEFAULT_LLAMA_CTX_SIZE = 32_768;

/** The catalogue entry behind a served id; Foundry serves a device-specific id prefixed by the alias. */
function catalogueEntry(servedId: string) {
  return (
    MODELS.find((model) => model.id === servedId) ??
    MODELS.find((model) => model.foundryAlias !== null && servedId.startsWith(model.foundryAlias)) ??
    MODELS.find((model) => model.id === FALLBACK_MODEL_ID)
  );
}

/** pi's provider block: one "Model" entry keyed by the served id, since only one model is served.
 * The dummy `apiKey` is required because OpenAI-compatible clients always send an Authorization header.
 * `servedContext` caps `contextWindow`; `null` (Foundry, not probed) keeps the catalogue value. */
export function buildPiLocalLlmProvider(servedModelId: string, servedContext: number | null): {
  providerId: string;
  providerJson: unknown;
  defaultModel: string;
} {
  const entry = catalogueEntry(servedModelId);
  const catalogueContext = entry?.contextWindow ?? DEFAULT_LLAMA_CTX_SIZE;
  return {
    providerId: LOCAL_LLM_PROVIDER_ID,
    providerJson: {
      baseUrl: LOCAL_LLM_BASE_URL,
      api: "openai-completions",
      apiKey: "sk-local",
      models: [
        {
          id: servedModelId,
          name: LOCAL_LLM_MODEL_NAME,
          contextWindow: servedContext === null ? catalogueContext : Math.min(catalogueContext, servedContext),
          reasoning: entry?.reasoning ?? true,
        },
      ],
    },
    defaultModel: servedModelId,
  };
}
