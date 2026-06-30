// OpenCode-specific constants and the `local-llm` provider-config generator, kept out of the
// generic agent `catalogue.ts`. Consumed by `AgentManager.svelte`, which calls
// `buildLocalLlmProvider` and passes the result to the `provision_opencode` Tauri command.
import { MODELS } from "$lib/models/catalogue";

/**
 * App-owned VM port the inference reverse-forward listens on. Containers reach the host inference
 * server at `http://host.containers.internal:51199/v1`. **Must equal** the Rust `INFERENCE_VM_PORT`
 * in `src-tauri/src/sync/mod.rs`.
 */
export const INFERENCE_VM_PORT = 51199;

/** Constant in-container base URL written into `opencode.json`. Stable across inference restarts
 * because the reverse forward absorbs the changing host port behind the fixed VM port. */
export const LOCAL_LLM_BASE_URL = `http://host.containers.internal:${INFERENCE_VM_PORT}/v1`;

/** Provider id of the custom OpenCode provider this app provisions. */
export const LOCAL_LLM_PROVIDER_ID = "local-llm";

/** Catalogue id of the recommended default pick (matches `recommendModels`); the provider-id
 * prefix that forms OpenCode's `provider/model` default is added in `buildLocalLlmProvider`. */
const DEFAULT_MODEL_ID = "phi-4-mini-reasoning";

/**
 * Generate the OpenCode `local-llm` provider block from the model catalogue, so the OpenCode model
 * map and the inference server's served catalogue cannot drift. The dummy `apiKey` is required
 * because `@ai-sdk/openai-compatible` always sends an `Authorization` header; llama-server ignores
 * its value (spiked 2026-06-29 with `sk-local`).
 */
export function buildLocalLlmProvider(): {
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
    defaultModel: `${LOCAL_LLM_PROVIDER_ID}/${DEFAULT_MODEL_ID}`,
  };
}
