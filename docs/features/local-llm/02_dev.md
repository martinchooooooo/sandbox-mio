---
type: dev-plan
title: "Local LLM Models Build Plan"
feature: local-llm
---
# Local LLM Models Build Plan

Build plan for the local-LLM feature described in `docs/features/local-llm/01_ideas.md`. It continues the ideas document's host-sidecar architecture and supersedes its single-engine framing: both inference engines (bundled llama.cpp+Vulkan and Foundry Local) are built and selected at runtime via a new advanced-settings surface, per the explicit product decision to A/B them. The feature is **informative by design**: each engine exposes its own ordered, human-readable setup checklist (`InferenceEngine::setup_steps`) that the UI renders both as a static explainer in `/settings` and as a live, ticking checklist during start in `/models`, so users see the "magic" each runtime performs under the hood (see Phase F2). The build adds one new Rust module (`src-tauri/src/inference/`), one new settings module (`src-tauri/src/settings/`), new dependencies (`sysinfo`, `reqwest`, `sha2`, `futures-util`), a frontend model catalogue (`src/lib/models/`), a top-level `/models` route, and a `/settings` route with an advanced section.

**Status (implemented):** Phases A–F and F2 are implemented and compile/build/typecheck clean (`cargo check`, `pnpm build`, e2e `tsc --noEmit`). The host inference process is spawned with `tokio::process` rather than the shell plugin, because the shell plugin's execute scope (`capabilities/default.json`) is pinned to the Mutagen sidecar — this mirrors how `agents::start_auth` spawns `podman` directly. The shipped GPU/NPU/winget probe uses a one-shot PowerShell query on Windows (the `wgpu` crate was avoided to keep the dependency footprint small); CPU/RAM use `sysinfo` cross-platform. The `download_gguf` checksum check is **skipped while a catalogue `sha256` is empty** so the first bring-up download that establishes the real hash succeeds; the placeholder hashes in `catalogue.ts` must be filled afterward. A `test_model` command exposes `MIO_TEST_MODEL` to the webview (which cannot read process env directly). **Phase G remains unimplemented**, gated on its spike as planned.

## Concrete Model

Inference runs on the Windows **host**, never in the sandbox container (the Podman machine VM is 2 GiB; see `01_ideas.md` Validation). A single app-wide inference server is active at a time, owned by Tauri state and exposing an OpenAI-compatible HTTP endpoint on a host loopback port.

Two engines sit behind one `InferenceEngine` trait in `src-tauri/src/inference/types.rs`:

- `LlamaServerEngine` (`src-tauri/src/inference/llama.rs`) — spawns a bundled `llama-server` Vulkan build, passing the downloaded GGUF path and port. **Bundling caveat (spike 2026-06-29):** the llama.cpp Vulkan release is not a single binary like Mutagen — it is `llama-server.exe` plus ~50 DLLs including a 48 MB `ggml-vulkan.dll`. The whole directory must be shipped as Tauri bundle `resources` (with `llama-server.exe` launched from that resource dir so its DLLs resolve), not as a single `externalBin` sidecar file. Models are GGUF files the app downloads from Hugging Face.
- `FoundryLocalEngine` (`src-tauri/src/inference/foundry.rs`) — drives the `foundry` CLI (Foundry Local, a first-party Windows component installed via winget `Microsoft.FoundryLocal`; spike confirmed CLI v0.8.119 at `%LOCALAPPDATA%\Microsoft\WindowsApps\foundry.exe`). Real command surface (verified): `foundry service start|stop|status`, `foundry model download|load|unload <alias>`, `foundry model list`. The non-interactive load path is `download` then `load` (not `model run`, which opens an interactive REPL). The service binds a **dynamic** port (e.g. `http://127.0.0.1:64154/`) the engine must parse from `service start`/`service status`. Foundry auto-selects an OpenVINO execution-provider variant per device (NPU > GPU > CPU); on the spike machine `qwen2.5-0.5b` resolved to `qwen2.5-0.5b-instruct-openvino-npu:5` on the NPU.

The active engine is chosen by a persisted setting (`engine: "llama" | "foundry"`), read from the new settings store (`src-tauri/src/settings/`). Both engines present the same OpenAI-compatible endpoint, so every consumer (the `/models` test surface, and later an in-sandbox agent such as OpenCode) is engine-agnostic.

The frontend model catalogue (`src/lib/models/catalogue.ts`) is static and per-model carries *both* a llama GGUF descriptor (HF repo, file, sha256, byte size) and a Foundry alias, plus a numeric RAM floor. A pure `recommendModels(specs)` function ranks the catalogue against probed host specs from the `detect_machine_specs` command.

## Resolved Decisions

**Acceleration tier strategy**: Build **both** engines behind the `InferenceEngine` trait and let the user pick the active one in advanced settings (`/settings`), per the explicit decision to test both. Foundry Local is *used when present* and the settings UI offers a winget-install action when it is absent (detected by `detect_machine_specs`); the app does not silently install it.

**Model launch catalogue**: Three Windows models, each with a numeric RAM floor (free host RAM required for the "recommended" rating) and a GGUF + Foundry pairing. Foundry aliases are corrected to ones verified present in `foundry model list` on 2026-06-29 (the earlier `phi-3.5-mini`/`qwen2.5-7b` guesses were wrong — the catalog has no Phi-3.5, and Phi is represented by `phi-4-mini-reasoning`):
- Qwen2.5-1.5B-Instruct — GGUF `Q4_K_M` (~1.1 GB), Foundry alias `qwen2.5-1.5b`; floor 4 GB. Low-RAM fallback.
- Phi-4-mini-reasoning — GGUF `Q4_K_M` (~2.5 GB), Foundry alias `phi-4-mini-reasoning`; floor 8 GB. **Default recommendation.**
- Qwen2.5-7B-Instruct — GGUF `Q4_K_M` (~4.7 GB), Foundry alias *(none in Foundry catalog as of the spike — llama engine only)*; floor 16 GB. Capable-machine pick.
Ratings: "recommended" when free RAM ≥ floor; "will run slowly" when free RAM ≥ floor − 2 GB; "won't fit" otherwise. Exact HF repo paths and sha256 values are filled in Phase C when each file is first downloaded and hashed.

**Agent/sandbox endpoint wiring**: A reverse Mutagen forward (new `forward_create_reverse` helper in `src-tauri/src/sync/mod.rs`) exposes the host inference port into the sandbox, and an `OPENAI_BASE_URL` (+ dummy `OPENAI_API_KEY`) env var is written into the workspace `mise.toml`/agent config so OpenCode points at it. OpenCode is the first confirmed consumer (it accepts a configurable OpenAI-compatible base URL). The container→host reachability of the reverse forward is **not yet validated** and is the gating spike of Phase G.

**Model storage location and sharing**: One shared store under the Tauri app data dir (`app_data_dir()/models/<model_id>/<file>.gguf`), shared across all sandboxes — the inference server is app-wide, so per-sandbox copies would waste gigabytes. Foundry-managed models live in Foundry's own store, not under the app data dir.

**UI entry point**: A top-level `/models` route (one app-wide server), not a per-`SandboxItem` panel. The host-sidecar architecture means a single shared server, which a per-card panel would misrepresent. Engine selection and Foundry install live under `/settings`.

**Concurrency and memory pressure**: Exactly one model loaded at a time. `start_inference` stops any running server before starting the next. A memory guard in `recommendModels` and in the `/models` UI warns (does not hard-block) when a model's RAM floor exceeds currently-free host RAM.

**macOS divergence**: Deferred, but the `InferenceEngine` trait and the dual-descriptor catalogue are the seams that absorb it: a future `MetalLlamaEngine` (or MLX engine) is a third trait impl, and macOS models add a third descriptor variant. No Windows-specific assumption leaks into `inference/types.rs`.

## Implemented: Phase A — Rust inference module and engine trait

New module `src-tauri/src/inference/` with `types.rs`, `mod.rs`, `llama.rs`, `foundry.rs`. Wired into `src-tauri/src/lib.rs` via `mod inference`.

### `src-tauri/src/inference/types.rs`

```rust
#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    Sidecar(String),          // llama-server sidecar could not be resolved/spawned
    FoundryUnavailable,       // foundry CLI not installed
    CommandFailed(String),
    ServerNotReady(String),   // health check did not pass within the timeout
    Download(String),
    ChecksumMismatch { expected: String, actual: String },
    ModelNotFound(String),
}

// Host capability probe returned by detect_machine_specs.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MachineSpecs {
    pub cpu_cores: usize,
    pub total_ram_bytes: u64,
    pub free_ram_bytes: u64,
    pub gpu_names: Vec<String>,   // all video controllers
    pub has_npu: bool,            // Copilot+-class accelerator present
    pub arch: String,             // e.g. "x86_64"
    pub foundry_installed: bool,
    pub foundry_installable: bool, // winget package resolvable
}

#[derive(serde::Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind { Llama, Foundry }

// Streamed from download_model to the frontend Channel.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind")]
pub enum ModelDownloadEvent {
    Started { total_bytes: u64 },
    Progress { downloaded_bytes: u64, total_bytes: u64 },
    Verifying,
    Done,
}

// Live server description; backs the /models status surface.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InferenceStatus {
    pub running: bool,
    pub engine: Option<EngineKind>,
    pub model_id: Option<String>,
    pub base_url: Option<String>,   // OpenAI-compatible, e.g. http://127.0.0.1:PORT/v1
}

// One running server. Held in Tauri state behind a Mutex<Option<ServerHandle>>.
pub struct ServerHandle {
    pub engine: EngineKind,
    pub model_id: String,
    pub port: u16,
    pub child: Option<tauri_plugin_shell::process::CommandChild>, // None for Foundry (service-owned)
}

// One stage of bringing a runtime online. The frontend renders a fixed checklist per
// engine (from `InferenceEngine::setup_steps`) and updates each row from these events,
// so the user sees the "magic" happening under the hood rather than one opaque spinner.
// Rust owns only the stable step `id` (the behaviour/ordering); the human-readable title
// and explanation live in Paraglide message files keyed by id, so all copy stays
// translatable (AGENTS.md mandates Paraglide for UI strings). No prose lives in Rust.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetupStep {
    pub id: String,        // stable key, e.g. "detect-device"; maps to Paraglide keys
                           // runtime_step_<id>_title / runtime_step_<id>_explain (with '-'→'_')
}

#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuntimeSetupEvent {
    StepStart { id: String },
    StepDone { id: String, detail: Option<String> }, // detail surfaces the observed fact, e.g. "Intel Arc 140V, 17.6 GB free"
    StepSkipped { id: String, reason: String },       // e.g. engine already installed / model already downloaded
    StepFailed { id: String, error: String },
    Ready { base_url: String, model_id: String },
}

#[async_trait::async_trait]
pub trait InferenceEngine: Send + Sync {
    fn kind(&self) -> EngineKind;
    // The ordered list of step ids this engine walks through on start. Pure metadata (no side
    // effects) so the UI can render the full plan before start() runs the first step. Copy for
    // each id lives in Paraglide, not here.
    fn setup_steps(&self) -> Vec<SetupStep>;
    // Start serving model_id on a chosen loopback port; emit a RuntimeSetupEvent per step to
    // `on_step` as each setup_steps() stage begins/completes; return a handle once health passes.
    async fn start(
        &self,
        model_id: &str,
        port: u16,
        on_step: &Channel<serde_json::Value>,
    ) -> Result<ServerHandle, InferenceError>;
    // Stop the server owned by handle.
    async fn stop(&self, handle: ServerHandle) -> Result<(), InferenceError>;
}

pub type ActiveServer = std::sync::Arc<tokio::sync::Mutex<Option<ServerHandle>>>;
```

Add `InferenceError` to `AppError` in `src-tauri/src/error.rs` via `#[from]`, with `kind()` arm `"Inference"`. Add `"Inference"` to the `AppErrorKind` union in `src/lib/errors.ts`.

### `src-tauri/src/inference/llama.rs`

`LlamaServerEngine { app: AppHandle }`. `start` resolves the GGUF path under `app_data_dir()/models/<model_id>/`, picks a free loopback port (bind-and-release, same trick as `open_terminal` in `src-tauri/src/lib.rs` line 124), spawns the bundled `llama-server` from its resource directory with `--model <path> --port <port> --host 127.0.0.1 -ngl <layers>`, then polls `GET /health` until ready or times out (`ServerNotReady`). Each stage emits a `RuntimeSetupEvent` to `on_step` (see `setup_steps()` below). The engine is bundled as a resource *directory* (not a single `externalBin` sidecar) because the Vulkan build is ~50 DLLs incl. a 48 MB `ggml-vulkan.dll`; it is launched from that directory so the DLLs resolve. `base_url` is `http://127.0.0.1:<port>/v1`.

`setup_steps()` returns the llama checklist surfaced in the UI (see "Informative Runtime Setup UI"): `detect-device` → `engine-ready` → `model-ready` → `launch-server` → `health-check`.

### `src-tauri/src/inference/foundry.rs`

`FoundryLocalEngine`. `start` runs `foundry service start` (idempotent), parses the endpoint base URL from its stdout (`🟢 Service is Started on http://127.0.0.1:<port>/`), then `foundry model download <alias>` followed by `foundry model load <alias>` (not `model run`, which is an interactive REPL). `stop` calls `foundry model unload <alias>`, leaving the OS-managed service running. If `foundry` is absent, every call returns `FoundryUnavailable`. The `foundry` binary is invoked through the shell plugin (not a sidecar — it is a system component).

Each Foundry stage emits a `RuntimeSetupEvent` to `on_step`. `setup_steps()` returns the Foundry checklist (see "Informative Runtime Setup UI"): `check-foundry` → `register-eps` → `start-service` → `model-ready` → `load-model` → `resolve-id`.

**Critical integration quirk (spike 2026-06-29):** the `model` field in a `POST /v1/chat/completions` request must be the **full loaded model ID** (e.g. `qwen2.5-0.5b-instruct-openvino-npu:5`), not the catalog alias (`qwen2.5-0.5b`) — sending the alias returns HTTP 400. After loading, `FoundryLocalEngine::start` must `GET /v1/models` and read `data[0].id` to obtain the served ID, then report it as the model name consumers pass back in (this is the `resolve-id` step). The Foundry response JSON also carries non-standard extra fields (`Successful`, `HttpStatusCode`, `IsDelta`, and both `delta` and `message` per choice), but `choices[0].message.content` is present and OpenAI-shaped, so consumers are unaffected.

### `src-tauri/src/inference/mod.rs`

- `pub fn engine_for(kind: EngineKind, app: &AppHandle) -> Arc<dyn InferenceEngine>` — constructs the chosen engine.
- `pub fn detect_specs(app: &AppHandle) -> MachineSpecs` — uses `sysinfo` for CPU cores + RAM; enumerates GPUs and the NPU; sets `foundry_installed`/`foundry_installable`. GPU/NPU/winget detection is Windows-specific behind `#[cfg(windows)]` (the spike used `Get-CimInstance Win32_VideoController`, `Get-PnpDevice` for `Intel(R) AI Boost`, and `winget show Microsoft.FoundryLocal`); the shipped probe prefers the `wgpu`/`sysinfo` crates for GPU/RAM and falls back to a one-shot PowerShell query only for NPU presence and winget resolvability.
- `pub async fn download_gguf(...)` — streaming HTTP download with `ModelDownloadEvent` progress, sha256 verify, atomic rename into place. See Phase D.

`sysinfo` added to `src-tauri/Cargo.toml`.

## Implemented: Phase B — Tauri commands

All added to `src-tauri/src/lib.rs` and registered in `invoke_handler`.

```rust
#[tauri::command]
async fn detect_machine_specs(app: AppHandle) -> Result<MachineSpecs, AppError>;

// Download a GGUF for the llama engine. No-op if already present and checksum matches.
#[tauri::command]
async fn download_model(
    model_id: String,
    repo: String,        // HF repo id
    file: String,        // GGUF filename in the repo
    sha256: String,
    on_progress: Channel<serde_json::Value>,
    app: AppHandle,
) -> Result<(), AppError>;

// Return which catalogue model_ids have a verified local GGUF.
#[tauri::command]
fn downloaded_models(app: AppHandle) -> Result<Vec<String>, AppError>;

// Start the app-wide server for `engine` + `model_id`, stopping any prior server first.
// Streams RuntimeSetupEvent to `on_progress` so the UI shows each setup step live.
#[tauri::command]
async fn start_inference(
    engine: EngineKind,
    model_id: String,
    on_progress: Channel<serde_json::Value>,
    active: State<'_, ActiveServer>,
    app: AppHandle,
) -> Result<InferenceStatus, AppError>;

// Return an engine's ordered setup checklist (SetupStep[]) without starting anything,
// so the UI can render the full plan up front.
#[tauri::command]
fn runtime_setup_steps(engine: EngineKind, app: AppHandle) -> Result<Vec<SetupStep>, AppError>;

#[tauri::command]
async fn stop_inference(active: State<'_, ActiveServer>, app: AppHandle) -> Result<(), AppError>;

#[tauri::command]
async fn inference_status(active: State<'_, ActiveServer>) -> Result<InferenceStatus, AppError>;

// Install Foundry Local via winget (advanced-settings action). Streams progress.
#[tauri::command]
async fn install_foundry_local(on_progress: Channel<serde_json::Value>) -> Result<(), AppError>;
```

`ActiveServer` managed in `app.setup()` alongside `ForwardPortMap`/`AuthSessionMap`: `app.manage(inference::types::ActiveServer::default())`.

## Implemented: Phase C — Frontend catalogue and recommendation engine

### `src/lib/models/types.ts`

```ts
export interface LlamaModelSource {
  repo: string;        // HF repo id
  file: string;        // GGUF filename
  sha256: string;
  sizeBytes: number;
}
export interface ModelDescriptor {
  id: string;
  label: string;
  paramsB: number;            // billions of parameters
  ramFloorBytes: number;      // free RAM for "recommended"
  llama: LlamaModelSource;    // GGUF source for the llama engine
  foundryAlias: string;       // Foundry catalog alias
}
export type ModelRating = "recommended" | "slow" | "wont-fit";
```

### `src/lib/models/catalogue.ts`

A `const MODELS: ModelDescriptor[]` with the three entries from Resolved Decisions. `sha256` values are placeholders until each file is downloaded and hashed during Phase D bring-up; the dev plan records them as TODO in the file.

### `src/lib/models/recommend.ts`

`recommendModels(specs: MachineSpecs): { descriptor: ModelDescriptor; rating: ModelRating }[]` — pure ranking by `freeRamBytes` against each `ramFloorBytes`, sorted recommended-first. Co-located with the catalogue it ranks; issues no IPC.

## Implemented: Phase D — Model download

`download_gguf` in `src-tauri/src/inference/mod.rs`: streams `https://huggingface.co/<repo>/resolve/main/<file>` with a streaming HTTP client (add `reqwest` with `stream` feature, or reuse `tauri-plugin-http` if preferred — decided at build time), writing to `<app_data>/models/<model_id>/<file>.part` while emitting `ModelDownloadEvent::Progress`. After the body completes, emits `Verifying`, computes sha256 (`sha2` crate), compares to the descriptor value (`ChecksumMismatch` on mismatch, deleting the partial), then atomically renames `.part` → final. `downloaded_models` reports a model as present only when the final file exists and its size matches `sizeBytes` (a cheap integrity proxy; full re-hash on demand only).

New deps in `src-tauri/Cargo.toml`: a streaming HTTP client and `sha2`.

## Implemented: Phase E — App settings store and advanced-settings UI

No app-global settings store exists today (the only persisted state is workspace-scoped `.mio/` files via `dismiss_image_update` in `src-tauri/src/lib.rs`). This phase adds one.

### `src-tauri/src/settings/`

`types.rs` + `mod.rs`. Persists a JSON file at `app_config_dir()/settings.json`.

```rust
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub inference_engine: EngineKind,   // default Llama
    pub llama_gpu_layers: Option<u32>,  // advanced: offload N layers to GPU; None = auto
    pub llama_ctx_size: Option<u32>,    // advanced: context window
}
```

Commands `get_settings() -> AppSettings` and `set_settings(settings: AppSettings)` in `src-tauri/src/lib.rs`. `start_inference` reads the engine from settings if the caller passes none; the advanced llama knobs are forwarded to `llama-server` args by `LlamaServerEngine::start`.

### `src/routes/settings/+page.svelte`

A `/settings` route with an "Advanced" section: a radio/segmented control to pick `inference_engine` (Llama vs Foundry), the llama GPU-layers and context-size inputs, and — when `specs.foundryInstalled` is false but `foundryInstallable` is true — an "Install Foundry Local" button wired to `install_foundry_local`. Below the engine selector, the static `RuntimeSteps.svelte` explainer (Phase F2) renders the selected engine's `runtime_setup_steps`, so choosing a runtime shows the user exactly what bringing it online involves. Uses existing `field`, `switch`, `input`, `button` UI primitives (`src/lib/components/ui/`). New translation keys added to `messages/en.json` and mirrored to `de.json`/`ru.json`.

## Implemented: Phase F — `/models` route (selection, download, test)

### `src/routes/models/+page.svelte`

The guided flow from `01_ideas.md`:
1. On mount, calls `detect_machine_specs` and renders the probed specs.
2. Calls `recommendModels(specs)` and lists models with their rating badge; the default-recommended one is highlighted. The memory guard shows a warning row for `slow`/`wont-fit` ratings.
3. Per model: a "Download" button (llama engine) wired to `download_model` with a `Channel` driving a progress bar; Foundry models show "downloads on first run" instead.
4. A "Start" button calls `start_inference(engine, model_id, on_progress)` where `engine` comes from settings. The `on_progress` channel drives the live runtime-setup checklist (see "Informative Runtime Setup UI"); on the terminal `Ready` event the route shows the live `base_url` from `InferenceStatus`.
5. A minimal chat test box: a textarea + send button POSTing to `<base_url>/chat/completions` so the user can confirm the model responds. This is the A/B surface for comparing the two engines.

State mirrors the `actions.svelte.ts` busy-tracking pattern already used in `SandboxItem.svelte`.

## Implemented: Phase F2 — Informative Runtime Setup UI

The app is intended to be informative: instead of one opaque "Starting…" spinner, both the `/settings` advanced section and the `/models` start flow render a **per-runtime stepped checklist** that explains, step by step, what the app is doing under the hood to bring each engine online. The checklist is the same data in both places — `/settings` shows it as a static, always-visible explainer per engine ("here is what choosing this runtime entails"), and `/models` shows it live during `start_inference`, ticking each row as `RuntimeSetupEvent`s arrive.

### Shared component — `src/lib/models/RuntimeSteps.svelte`

Renders a `SetupStep[]` (from `runtime_setup_steps`) as an ordered list. For each step `id` it resolves the title and explanation through Paraglide — `m["runtime_step_" + id.replaceAll("-","_") + "_title"]()` and `..._explain()` — via a small id→message lookup object (Paraglide's generated `m` exposes one function per key, so the component holds an explicit `Record<string, () => string>` map rather than indexing `m` dynamically). Each row shows the resolved title, the explanation sentence, and a status icon. In live mode it consumes `RuntimeSetupEvent`s and transitions each row pending → running → done/skipped/failed, appending the event `detail` (e.g. the detected GPU, the resolved model ID) so the user sees the actual observed fact. The `detail` string is backend-produced data (a device name, a port, a model ID), not translatable copy, so it is shown verbatim. Used by both routes; the static `/settings` rendering omits the live event binding.

### llama.cpp (Vulkan) runtime — step ids and Paraglide keys

The `setup_steps()` for `LlamaServerEngine` returns these ids in order; the English copy below is the value to put in `messages/en.json` under `runtime_step_<id>_title` / `runtime_step_<id>_explain` (mirrored to `de.json`/`ru.json`):

1. **`detect-device`** — title "Detect GPU"; explain "Detecting a Vulkan-capable GPU. llama.cpp uses the cross-vendor Vulkan API, so your Intel, AMD, or NVIDIA GPU works without vendor-specific drivers." `detail` reports the device, e.g. "Intel Arc 140V GPU, 17.6 GB free".
2. **`engine-ready`** — title "Locate engine"; explain "Locating the bundled inference engine. The llama-server binary and its Vulkan libraries ship inside the app — nothing is installed on your system." Emits `StepSkipped` (already present); never downloads.
3. **`model-ready`** — title "Prepare model"; explain "Checking the model weights. The chosen GGUF file is downloaded once from Hugging Face and cached on disk; later starts reuse it." `StepSkipped` when the GGUF is already present and verified.
4. **`launch-server`** — title "Start server"; explain "Starting the local server. The model is loaded into memory and, where it fits, offloaded to the GPU for speed." `detail` reports the GPU layer count.
5. **`health-check`** — title "Go live"; explain "Waiting for the server to report healthy, then exposing an OpenAI-compatible endpoint on your machine." Terminal `Ready { base_url, model_id }` follows.

### Foundry Local runtime — step ids and Paraglide keys

The `setup_steps()` for `FoundryLocalEngine`, reflecting the verified 2026-06-29 flow; same `runtime_step_<id>_title`/`_explain` key convention:

1. **`check-foundry`** — title "Check Foundry"; explain "Checking for Foundry Local, Microsoft's on-device AI runtime. Unlike llama.cpp it is a system component; if missing, the app can install it for you via winget." `StepFailed` → the UI links to the `/settings` install action when absent.
2. **`register-eps`** — title "Register accelerators"; explain "Registering execution providers. Foundry picks the best accelerator on your PC — NPU, GPU, or CPU — through Intel OpenVINO and similar backends." Maps to `foundry service init` (idempotent).
3. **`start-service`** — title "Start service"; explain "Starting the Foundry inference service. It runs in the background and listens on a local port." `detail` reports the parsed port.
4. **`model-ready`** — title "Download model"; explain "Downloading the model. Foundry fetches the variant matched to your hardware (an NPU build here), not a generic file." `StepSkipped` when already cached. Maps to `foundry model download <alias>`.
5. **`load-model`** — title "Load model"; explain "Loading the model into the service so it is ready to answer requests." Maps to `foundry model load <alias>`.
6. **`resolve-id`** — title "Resolve model name"; explain "Reading back the exact served model name. Foundry serves a device-specific model ID that requests must use." `detail` reports the resolved ID, e.g. "qwen2.5-0.5b-instruct-openvino-npu:5". Terminal `Ready` follows.

The `_title`/`_explain` keys for every id above are added to `messages/en.json` first, then mirrored to every other locale file, per AGENTS.md.

### Why step ids are engine-owned but copy is in Paraglide

The ordered step *ids* come from `InferenceEngine::setup_steps()` (Rust), so the checklist always matches what `start()` actually does, and a future macOS Metal/MLX engine supplies its own ids and the same `RuntimeSteps.svelte` renders them. The *copy* lives in Paraglide so every word is translatable. The residual drift risk — a new step id with no matching `runtime_step_<id>_*` key renders blank — is bounded two ways: the id→message map in `RuntimeSteps.svelte` is the single place that must gain an entry per id, and the e2e checklist assertion (Phase F2 coverage in `03_e2e.md`) fails if a known step renders without text.

## Remaining: Phase G — In-sandbox wiring (OpenCode) — gated on a spike

Exposes the host endpoint inside a sandbox so an agent like OpenCode can use it.

1. New `forward_create_reverse` in `src-tauri/src/sync/mod.rs`: source `=<alias>:tcp:127.0.0.1:<vm_port>`, dest `=tcp:127.0.0.1:<host_inference_port>` — the mirror of `forward_create` (which is host-listen → remote-connect). Container reaches the forwarded port via the Podman machine VM; the exact in-container address (`host.containers.internal` vs a published VM port) is the spike subject.
2. A command `wire_sandbox_to_inference(name)` that creates the reverse forward and writes `OPENAI_BASE_URL` + a dummy `OPENAI_API_KEY` into the sandbox so OpenCode picks them up.
3. **Gating spike (run before building Phase G):** confirm a process inside a sandbox container can reach a host loopback HTTP server through a Mutagen reverse forward, and confirm OpenCode honours `OPENAI_BASE_URL` against a llama-server `/v1` endpoint. Record the result in this section. Until the spike clears, Phases A–F deliver a host-only, user-testable feature (the `/models` chat box), which satisfies the immediate goal of A/B-testing both engines.

## Lifecycle / Cross-Cutting Concerns

**Single active server**: `ActiveServer` is a `Mutex<Option<ServerHandle>>`. `start_inference` locks it, stops any existing handle via its engine, then starts the new one and stores the handle. This enforces the one-model-at-a-time decision.

**Shutdown on exit**: The `RunEvent::Exit` hook in `src-tauri/src/lib.rs` (currently only stops the Mutagen daemon) also stops the active inference server, so a llama-server sidecar holding gigabytes resident is never orphaned. Foundry's service is OS-managed and left running, but the loaded model is unloaded.

**Port allocation**: Inference ports use the same bind-and-release ephemeral-port trick as `open_terminal` (`src-tauri/src/lib.rs` line 124).

**Download resumption**: Not in v1 — an interrupted download deletes its `.part` and restarts. Resume can be added later via HTTP range requests without changing the command surface.

**Settings as engine source of truth**: The active engine is whatever `AppSettings.inference_engine` says at `start_inference` time; switching engines in `/settings` does not hot-swap a running server — the user stops and restarts from `/models`. Documented so a future agent does not add implicit restart-on-settings-change.

**Foundry absence is non-fatal**: Selecting the Foundry engine when `foundry` is absent surfaces `FoundryUnavailable` as a normal `AppError`; the `/settings` UI guides the user to install it rather than the app crashing or auto-installing.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `start_inference` returns `ServerNotReady` despite a valid model | The `llama-server` health endpoint is polled before the model finishes loading into RAM | Poll `GET /health` with a generous timeout proportional to model size; do not treat the first failed poll as fatal |
| Two models loaded at once exhaust host RAM | A second `start_inference` ran without stopping the first | `start_inference` must stop the existing `ServerHandle` under the `ActiveServer` mutex before starting a new server |
| Foundry engine selected but every call fails | `foundry` CLI not installed (it is a system component, not a bundled sidecar) | `detect_machine_specs` reports `foundryInstalled`; the `/settings` UI offers `install_foundry_local` (winget) before allowing the Foundry engine |
| Downloaded GGUF reported present but corrupt | Size-only presence check passed on a truncated file | `download_model` verifies sha256 before the atomic rename; never mark a `.part` file as present |
| Orphaned `llama-server` process after closing the app | The sidecar outlives the app if not stopped | Stop the active server in the `RunEvent::Exit` hook in `src-tauri/src/lib.rs`, alongside the Mutagen daemon shutdown |
| GPU/NPU detection wrong inside packaged build | `Get-PnpDevice`/`Get-CimInstance` probes assume PowerShell availability and Windows | Gate NPU/winget probes behind `#[cfg(windows)]`; use `sysinfo`/`wgpu` for cross-platform CPU/RAM/GPU so the macOS port does not break the probe |
| Sandbox agent cannot reach the host model | Host loopback is invisible inside the container; a forward in the wrong direction does nothing | Use `forward_create_reverse` (Phase G) and validate the in-container address via the gating spike before relying on it |
| Settings engine change seems to do nothing | Switching the engine in `/settings` does not restart a running server by design | Stop and restart inference from `/models` after changing the engine; do not expect a hot-swap |
