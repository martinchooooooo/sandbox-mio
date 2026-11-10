import { MODELS } from "$lib/models/catalogue";
import { LOCAL_LLM_BASE_URL, LOCAL_LLM_PROVIDER_ID } from "./local-llm";

// Generates the provider config to keep OpenCode's model map in sync with the inference server.
export function buildLocalLlmProvider(defaultModelId: string): {
  providerId: string;
  providerJson: unknown;
  defaultModel: string;
} {
  const models: Record<string, { name: string }> = {};
  for (const model of MODELS) {
    models[model.id] = { name: model.label };
  }
  return {
    providerId: LOCAL_LLM_PROVIDER_ID,
    providerJson: {
      npm: "@ai-sdk/openai-compatible",
      name: "Local LLM",
      options: { baseURL: LOCAL_LLM_BASE_URL, apiKey: "sk-local" },
      models,
    },
    defaultModel: `${LOCAL_LLM_PROVIDER_ID}/${defaultModelId}`,
  };
}
