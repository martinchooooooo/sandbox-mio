// pi-specific constants and the `local-llm` provider-config generator, kept out of the generic
// agent `catalogue.ts`. Consumed by `AgentManager.svelte`, which calls `buildPiLocalLlmProvider`
// and passes the result to the `provision_pi` Tauri command.
//
// pi reads `models.json` from `$PI_CODING_AGENT_DIR` (default `~/.pi/agent`, not bind-mounted).
// Provisioning redirects that dir into the bind-mounted workspace via `PI_CODING_AGENT_DIR` set in
// the workspace `mise.toml [env]`, so the config write stays host-side. See docs/features/pi-agent.
import { MODELS } from "$lib/models/catalogue";
import { LOCAL_LLM_BASE_URL, LOCAL_LLM_PROVIDER_ID } from "./local-llm";

/** Container-absolute config dir written into `PI_CODING_AGENT_DIR`; parent of the redirected
 * `models.json`. Lives under the bind-mounted workspace (`/root/work`), beside `.mio/zellij.json`. */
export const PI_CONFIG_DIR_CONTAINER = "/root/work/.mio/pi-agent";

/** Env var pi honours to override its config directory (default `~/.pi/agent`). */
export const PI_CONFIG_ENV = "PI_CODING_AGENT_DIR";

/** Workspace-relative path of the redirected `models.json`; matches the descriptor's
 * `relativeConfigPath`. Nested (unlike OpenCode's root `opencode.json`), so its parent is created. */
export const PI_CONFIG_REL_PATH = ".mio/pi-agent/models.json";

/** Recommended `--model provider/id` launch argument surfaced as UI guidance. pi selects its model
 * with `--model`, not a config default, so this is never written into `models.json`. */
export const PI_DEFAULT_MODEL = `${LOCAL_LLM_PROVIDER_ID}/phi-4-mini-reasoning`;

/**
 * Generate pi's `local-llm` provider block from the model catalogue, so pi's model map and the
 * inference server's served catalogue cannot drift. `provision_pi` nests the returned `providerJson`
 * under `providers[providerId]`. The dummy `apiKey` is required because an OpenAI-compatible client
 * always sends an `Authorization` header; llama-server ignores its value.
 *
 * pi's schema differs from OpenCode's: `baseUrl` (camelCase), `api: "openai-completions"` instead
 * of an npm loader, and `models` as an **array** of `{ id, name }` (not a map). Minimal entries
 * suffice — pi supplies context/max-out defaults (spiked 2026-07-01).
 */
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
