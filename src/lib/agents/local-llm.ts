// Constants shared by every agent this app pre-wires to the local-LLM host inference server
// (OpenCode, pi). The provider-block generators live in each agent's own module (`opencode.ts`,
// `pi.ts`); only the address constants are shared here so the fixed VM port has one source.

/**
 * App-owned VM port the inference reverse-forward listens on. Containers reach the host inference
 * server at `http://host.containers.internal:51199/v1`. **Must equal** the Rust `INFERENCE_VM_PORT`
 * in `src-tauri/src/sync/mod.rs`.
 */
export const INFERENCE_VM_PORT = 51199;

/** Constant in-container base URL written into each agent's provider config. Stable across
 * inference restarts because the reverse forward absorbs the changing host port behind the fixed
 * VM port. */
export const LOCAL_LLM_BASE_URL = `http://host.containers.internal:${INFERENCE_VM_PORT}/v1`;

/** Provider id of the custom local-LLM provider this app provisions into every wired agent. */
export const LOCAL_LLM_PROVIDER_ID = "local-llm";
