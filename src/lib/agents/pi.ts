import { MODELS } from "$lib/models/catalogue";
import { LOCAL_LLM_BASE_URL, LOCAL_LLM_PROVIDER_ID } from "./local-llm";

export const PI_CONFIG_DIR_CONTAINER = "/root/work/.mio/pi-agent";
export const PI_CONFIG_ENV = "PI_CODING_AGENT_DIR";
export const PI_CONFIG_REL_PATH = ".mio/pi-agent/models.json";
export const PI_DEFAULT_MODEL = `${LOCAL_LLM_PROVIDER_ID}/phi-4-mini-reasoning`;

/** Generates the provider config to keep pi's model map in sync with the inference server.
 * The dummy `apiKey` is required — OpenAI-compatible clients always send an Authorization
 * header; llama-server ignores its value. pi wants `models` as an array, not a map. */
export function buildPiLocalLlmProvider(): { providerId: string; providerJson: unknown } {
  return {
    providerId: LOCAL_LLM_PROVIDER_ID,
    providerJson: {
      baseUrl: LOCAL_LLM_BASE_URL,
      api: "openai-completions",
      apiKey: "sk-local",
      models: MODELS.map((model) => ({ id: model.id, name: model.label })),
    },
  };
}
