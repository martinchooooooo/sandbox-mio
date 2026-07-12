// OpenCode-specific constants and the `local-llm` provider-config generator, kept out of the
// generic agent `catalogue.ts`. Consumed by `AgentManager.svelte`, which calls
// `buildLocalLlmProvider` and passes the result to the `provision_opencode` Tauri command.
import { MODELS } from "$lib/models/catalogue";
import { LOCAL_LLM_BASE_URL, LOCAL_LLM_PROVIDER_ID } from "./local-llm";

/** Catalogue id used as OpenCode's default model when no inference server has been started yet
 * (matches `recommendModels`'s recommended pick). Once a model is running, callers should pass its
 * id instead — see `reprovisionInstalledAgents`. */
export const FALLBACK_MODEL_ID = "phi-4-mini-reasoning";

/**
 * Generate the OpenCode `local-llm` provider block from the model catalogue, so the OpenCode model
 * map and the inference server's served catalogue cannot drift. The dummy `apiKey` is required
 * because `@ai-sdk/openai-compatible` always sends an `Authorization` header; llama-server ignores
 * its value (spiked 2026-06-29 with `sk-local`).
 *
 * `defaultModelId` should be whichever catalogue id is actually running (or about to run) so
 * OpenCode's default doesn't drift from the active inference server.
 */
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
