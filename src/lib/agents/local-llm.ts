/** Port the inference forward listens on inside every sandbox; must equal Rust
 * `INFERENCE_GUEST_PORT` in `src-tauri/src/inference/mod.rs`. */
export const INFERENCE_GUEST_PORT = 51199;

/** In-sandbox base URL: the sandbox's own loopback, where its inference forward listens. Stable
 * across engines, providers and server restarts. */
export const LOCAL_LLM_BASE_URL = `http://127.0.0.1:${INFERENCE_GUEST_PORT}/v1`;

/** Provider id of the local-LLM provider provisioned into wired agents. */
export const LOCAL_LLM_PROVIDER_ID = "local-llm";

/** Default model when no inference server is running (matches `recommendModels`'s pick). */
export const FALLBACK_MODEL_ID = "qwen3.5-4b";
