/** VM port for the inference reverse-forward; must equal Rust `INFERENCE_VM_PORT` in
 * `src-tauri/src/sync/mod.rs`. */
export const INFERENCE_VM_PORT = 51199;

/** In-container base URL, stable across inference restarts: the fixed VM port absorbs the
 * changing host port. */
export const LOCAL_LLM_BASE_URL = `http://host.containers.internal:${INFERENCE_VM_PORT}/v1`;

/** Provider id of the local-LLM provider provisioned into wired agents. */
export const LOCAL_LLM_PROVIDER_ID = "local-llm";
