# Local LLM Models Ideas

This document covers the ideation for running local LLM inference inside the sandbox-mio app: a guided flow that detects the user's machine specs, recommends a lightweight model appropriate to that hardware (e.g. Phi-3 on a modest Windows laptop), downloads the model weights to the user's machine, and configures an inference server the app and its sandboxed agents can call. The feature is entirely unimplemented today; no inference runtime, model catalogue, hardware probe, or download flow exists in the codebase. It builds on three existing seams: the Tauri sidecar pattern used for Mutagen (`src-tauri/src/sync/mod.rs`), the streamed-progress command pattern used for host provisioning and agent install (`provision_sandbox_host` / `install_agent` in `src-tauri/src/lib.rs`), and the static frontend catalogue pattern proposed for agents (`src/lib/agents/`). The focus of this first document is Windows; macOS is flagged where its choices would diverge but is deferred to a later iteration.

## Current State

Nothing in the codebase concerns local model inference. The app's compute model is that every tool runs *inside* a Podman container: software is installed via mise (`RuntimeManager` in `src-tauri/src/runtime/types.rs`), and coding agents such as Claude Code authenticate against cloud APIs from within the container (`src-tauri/src/agents/mod.rs`). There is no host-side compute path beyond the Tauri process itself.

The seams the feature will build on:

- `src-tauri/src/sync/mod.rs` — the Mutagen sidecar: one self-contained binary bundled via `bundle.externalBin` in `tauri.conf.json`, resolved at runtime by target triple (`mutagen-<target-triple>`), invoked through the shell plugin, with an app-private data directory pinned via an env var (`MUTAGEN_DATA_DIRECTORY`). A local-inference engine binary would be bundled and driven the same way.
- `src-tauri/src/lib.rs` — IPC command registration and the two streamed-progress precedents: `provision_sandbox_host` (line 351) and `install_agent` (line 389), both of which take a `Channel<serde_json::Value>` and emit typed progress events. A model download command will follow the same shape.
- `src-tauri/src/agents/types.rs` — the `AgentInstallEvent` / `AgentAuthEvent` typed event enums; a `ModelDownloadEvent` enum will mirror them.
- `src/lib/registry.svelte.ts` and the proposed `src/lib/agents/` catalogue — the precedent for a static, app-bundled descriptor list rendered by the frontend. A model catalogue will live alongside.
- `dirs` crate (already a dependency in `src-tauri/Cargo.toml`) — resolves platform data directories; model weights need a stable on-disk home.

## Proposed Model

### Hardware Probe (`detect_machine_specs` command)

A new Tauri command reads the host's CPU core count, total physical RAM, GPU vendor/model, and architecture, and returns a `MachineSpecs` struct to the frontend. The recommendation engine consumes the struct. The probe runs on the **host**, not in a container, because the container VM's resources are deliberately small and unrepresentative of what the model can actually use (see Validation: the Podman machine VM is capped at 2 GiB). The seam is a new `src-tauri/src/inference/` module registered in `lib.rs` alongside the other command groups.

### Model Catalogue (Static, App-Bundled)

A static list of model descriptors compiled into the frontend, mirroring the agent catalogue. Each descriptor names the model, the download source (a Hugging Face repo + GGUF file), the quantizations offered, and the minimum/recommended hardware envelope per quantization (RAM floor, whether a GPU helps, approximate on-disk size). The catalogue is versioned with the app binary; there is no server-side catalogue fetch. Launch catalogue (Windows): Phi-3-mini (3.8B) as the default lightweight pick, with one smaller fallback (e.g. Qwen2.5-1.5B or Llama-3.2-1B) for low-RAM machines and one larger optional pick for capable machines.

### Recommendation Engine (`recommendModel` in frontend)

A pure function mapping `MachineSpecs` → a ranked list of catalogue entries with a single "recommended" highlight. It reasons over RAM headroom first (the model's resident set plus OS overhead must fit comfortably in host RAM), then GPU acceleration availability (which affects tokens/sec but not whether the model runs at all), then disk. The engine lives in the frontend next to the catalogue so its logic is co-located with the data it ranks; it issues no IPC of its own.

### Inference Runtime (Host Sidecar)

The model server runs on the Windows **host** as a bundled sidecar, exposing an OpenAI-compatible HTTP endpoint on a loopback port. This deliberately breaks the app's "everything runs in the container" convention because the container VM is too small (2 GiB) and has no practical GPU passthrough (see Validation). The sidecar is bundled and resolved exactly like Mutagen (`src-tauri/src/sync/mod.rs`): one binary per target triple via `bundle.externalBin`, invoked through the shell plugin, with the model file path and port passed as args. A new `src-tauri/src/inference/` module owns the lifecycle (spawn, health-check, shutdown on app exit — the same `RunEvent::Exit` hook in `lib.rs` that shuts down the Mutagen daemon).

### Model Download (`download_model` streamed command)

A Tauri command that streams the chosen GGUF file from Hugging Face to the host data directory, emitting `ModelDownloadEvent` progress (bytes downloaded / total, then a checksum-verify step, then done) over a `Channel`, mirroring `install_agent`. Downloaded weights live under the app data dir (resolved via the `dirs` crate), keyed by model id + quantization, so re-selecting an already-downloaded model is a no-op. A checksum from the catalogue descriptor guards against truncated or tampered downloads.

### Wiring to Sandboxes and Agents (loopback endpoint injection)

The host inference server's loopback endpoint is reachable from the Tauri UI directly. To make it reachable from *inside* a sandbox container, the app reuses the Mutagen forwarding primitive already used for the web terminal (`sync::forward_create` in `src-tauri/src/lib.rs`, line 137) — but in reverse, exposing a host port to the container rather than a container port to the host. Agents configured to use an OpenAI-compatible base URL (e.g. via an env var written into the workspace) then point at the forwarded endpoint. Exact env-var wiring per agent is an open question.

### UI Flow

A guided, stepped surface: (1) "Detect my machine" shows the probed specs; (2) the recommendation engine presents the suggested model with smaller/larger alternatives and a one-line rationale ("recommended for 32 GB RAM, Intel Arc GPU"); (3) the user confirms and the download runs with a progress bar; (4) the model is started and a smoke-test prompt confirms it responds. Entry point placement (top-level route vs. per-sandbox panel) is an open question.

## Technology Choices with Tradeoffs

### Inference location: host sidecar (not the sandbox container)

Chosen: run the model server on the Windows host as a bundled sidecar.

Alternatives evaluated:

- **Inside the sandbox container (consistent with every other tool)** — rejected on hard evidence. The Podman machine VM is allocated only 2 GiB of RAM (`podman machine list`, Validation below); a Phi-3-mini Q4 GGUF needs ~2.3 GB resident plus runtime overhead, so it does not fit in the VM at all, and larger models are categorically impossible. Raising the VM memory is possible but takes RAM permanently away from the host even when no model is loaded, and still leaves the GPU problem unsolved.
- **GPU passthrough into the container** — rejected. The host GPU is an Intel Arc iGPU (Validation), and WSL2 (the Podman machine's VM type is `wsl`) offers no reliable Intel-Arc compute passthrough for this workload. Even on Nvidia, GPU passthrough into the rootless Podman machine is fragile and not a baseline we can assume across user hardware.

### Inference engine: llama.cpp with the Vulkan backend (not CUDA, not Ollama, not OpenVINO)

Chosen: bundle a llama.cpp `llama-server` build with the Vulkan backend, exposing its OpenAI-compatible endpoint.

Alternatives evaluated:

- **llama.cpp CUDA build** — rejected for the baseline. The validation machine has an Intel Arc iGPU with no CUDA (`nvidia-smi` absent); a CUDA-only build would fall back to CPU on a large fraction of target hardware. Vulkan accelerates on Intel Arc, AMD, and Nvidia alike from a single build, making it the portable acceleration choice on Windows.
- **Ollama** — rejected as the bundled engine. Ollama is a full background service with its own model store, daemon, and update mechanism; bundling it duplicates download/storage management the app wants to own, and its default install is a separate system service rather than an app-private sidecar (contrast the app's deliberately app-private Mutagen daemon). It remains a reasonable *fallback* the catalogue could target if a user already has it installed.
- **Intel OpenVINO / OpenVINO GenAI** — rejected for v1, and now subsumed by the Windows-acceleration-tier decision below. OpenVINO is the highest-performance path on this exact CPU (Lunar Lake) because it can target the NPU and Arc GPU, but it requires model conversion to OpenVINO IR rather than GGUF, a separate runtime, and Intel-specific code paths that do not generalise to AMD/Nvidia users. On Windows it is reachable *through* Windows ML's OpenVINO execution provider rather than as a standalone integration, so the standalone OpenVINO option folds into the Foundry Local tier below.

- **Windows Copilot Runtime (Foundry Local)** — rejected as the baseline, retained as the Windows acceleration tier candidate (see the dedicated section below). "Windows Copilot Runtime" is three distinct pieces: **Phi Silica** (an NPU-resident Phi model exposed only as a Windows App SDK native WinRT API — *not* an HTTP server, so it does not fit the OpenAI-compatible-endpoint contract OpenCode and other agents need without writing a custom shim); **Windows ML** (an ONNX Runtime layer that auto-selects an execution provider — Intel OpenVINO, AMD, Nvidia, Qualcomm — but is an inference layer, not an endpoint); and **Foundry Local** (a host model runner built on Windows ML that *does* expose an OpenAI-compatible REST endpoint). Only Foundry Local is a genuine alternative to a bundled llama.cpp server. It is rejected as the portable baseline because it is Windows-only (the deferred macOS target cannot use it), serves ONNX models rather than GGUF (a smaller catalogue for the lightweight-chat tier), and is a first-party *system component* distributed via winget/Store rather than an app-private bundled binary — the same managed-service concern that rejected Ollama, plus preview-stage API/CLI churn. Its decisive advantage is that it reaches the NPU (confirmed present on the validation machine) and GPU/CPU automatically via execution providers.
- **ONNX Runtime GenAI** — rejected for v1. Comparable portability story to Vulkan-llama.cpp but a smaller ready-to-bundle ecosystem of lightweight chat models in its native format; GGUF has far wider model availability for the lightweight tier this feature targets.

### Model format: GGUF

Chosen: GGUF, the llama.cpp-native quantized format. Alternative: safetensors / raw HF weights — rejected because they are unquantized (multiples larger on disk and in RAM, defeating the "lightweight, fits the machine" goal) and require a conversion+quantization step the app would have to run. GGUF files are pre-quantized and download-ready.

### Windows acceleration tier: llama.cpp+Vulkan baseline, Foundry Local as the optional NPU/GPU tier

Chosen: ship the portable llama.cpp+Vulkan engine as the universal baseline on every platform, and treat **Foundry Local** as a single optional Windows acceleration tier that, when present, runs models on the NPU/GPU via Windows ML execution providers. This collapses the earlier separate "OpenVINO tier" and "Foundry Local" candidates into one decision, because Windows ML reaches OpenVINO (and the Intel NPU) on Intel silicon internally.

Tier-selection seam: the `detect_machine_specs` probe additionally reports whether a Copilot+-class NPU is present (the validation machine exposes an `Intel(R) AI Boost` device) and whether Foundry Local is installed/installable (`Microsoft.FoundryLocal` via winget). When both hold, the recommendation engine may surface the Foundry tier; otherwise it falls back to the bundled Vulkan engine. Both tiers present the same OpenAI-compatible endpoint to consumers (OpenCode and other agents), so the wiring in "Wiring to Sandboxes and Agents" is identical regardless of tier.

Rejected for the tier: making Foundry Local the *only* Windows engine (loses the no-Copilot+-PC fallback and the GGUF catalogue), and bundling OpenVINO directly instead of via Windows ML (duplicates an EP the OS already manages, and pins us to Intel-only code).

### Model source: Hugging Face direct download

Chosen: stream GGUF files directly from Hugging Face repos named in the catalogue. Alternatives: (a) **bundle weights in the installer** — rejected because it bloats the app download by gigabytes for a model many users will not pick, and ties model updates to app releases; (b) **a self-hosted mirror** — rejected for v1 because it adds hosting/bandwidth cost and a server dependency for what Hugging Face already serves with stable URLs and checksums.

### Hardware probe: Rust-side via the `dirs`/`sysinfo` path (not frontend, not raw WMI shelling)

Chosen: probe from Rust in the new `inference` module. The validation used PowerShell CIM queries (`Get-CimInstance`) to confirm the signals are available, but the shipped probe should use a Rust crate (`sysinfo` for CPU/RAM, a GPU-enumeration approach for vendor/model) so it is cross-platform-ready for the deferred macOS work and avoids shelling to PowerShell. Alternative: **probe from the frontend via a JS API** — rejected because the browser/webview exposes neither total physical RAM nor GPU vendor reliably, and the data must cross into Rust anyway to drive the sidecar.

### Catalogue definition: static frontend TypeScript object

Chosen: a TypeScript `const` array, co-located with the recommendation engine that ranks it, mirroring the agent catalogue decision. Alternative: **a Rust-side struct serialized to the frontend** — rejected because the catalogue is purely descriptive metadata with no runtime-variable fields; the only Rust-side need is the download URL + checksum, which the frontend passes into the `download_model` command.

## Validation

Spike run on 2026-06-29 on the developer's Windows 11 machine to clear the central risk — *where can inference actually run, and with what acceleration* — before committing to an architecture.

Cleared risks:

- **Host hardware signals are available.** `Get-CimInstance Win32_Processor`, `Win32_ComputerSystem`, and `Win32_VideoController` returned CPU model + cores, total RAM, and GPU model in one call. Result: Intel Core Ultra 7 258V (8 cores), 31.5 GB RAM, Intel Arc 140V iGPU (16 GB shared), AMD64. Confirms the recommendation engine has real inputs to reason over.
- **No Nvidia / CUDA on the baseline machine.** `nvidia-smi` is absent and the only GPU is an Intel Arc iGPU. Command: `nvidia-smi --query-gpu=name,memory.total --format=csv` (not found). This directly drove rejecting a CUDA-only engine in favour of the Vulkan backend.
- **No LLM runtime is pre-installed.** `Get-Command ollama` and `Get-Command llama-server` both returned nothing; `podman` is present. Confirms the app must bundle its own engine rather than assume one exists.
- **The sandbox container VM cannot host the model.** `podman machine list --format json` shows every machine (`podman-machine-default`, `mio-tests`, `mio-tests-remote`) allocated `Memory: 2147483648` (2 GiB) with `VMType: wsl`. A Phi-3-mini Q4 GGUF (~2.3 GB resident) does not fit. This is the decisive evidence for the host-sidecar architecture over the in-container default.

- **A Copilot+-class NPU is present and the Windows acceleration tier is reachable.** The machine is Windows 11 25H2 (build 26200) and exposes an `Intel(R) AI Boost` device (`Get-PnpDevice`, Status OK) — the NPU the Windows ML / Foundry Local path targets. `winget` is present (`Get-Command winget`), and `winget show Microsoft.FoundryLocal` resolves the package (Foundry Local is not currently installed: `Get-Command foundry` is empty). This confirms the Foundry Local acceleration tier is a real option on this hardware, while the absence of a pre-installed runtime reconfirms the baseline must bundle its own engine.

Second spike on 2026-06-29 proved **both engines actually serve completions on this hardware**, clearing the central engine-viability risk before the build:

- **llama.cpp Vulkan runs on the Intel Arc GPU.** Downloaded the prebuilt `llama-b9835-bin-win-vulkan-x64.zip` release; `llama-server.exe --list-devices` reported `Vulkan0: Intel(R) Arc(TM) 140V GPU (16GB) (18413 MiB, 17645 MiB free)`. Started `llama-server --model <gguf> --port 18080 -ngl 99`, polled `GET /health` → 200, and `POST /v1/chat/completions` returned a valid `chat.completion` with `choices[0].message.content` = "Sunshine.". Confirms the Vulkan baseline engine works on Arc without CUDA.
- **Foundry Local runs on the Intel NPU.** `winget install Microsoft.FoundryLocal` installed CLI v0.8.119; `foundry service start` exposed an OpenAI endpoint on a dynamic port (`http://127.0.0.1:64154/`); `foundry model download qwen2.5-0.5b` + `foundry model load qwen2.5-0.5b` auto-selected the OpenVINO **NPU** variant (`qwen2.5-0.5b-instruct-openvino-npu:5`, 0.32 GB), and `POST /v1/chat/completions` returned `choices[0].message.content` = "Hello there!". Confirms the Foundry/NPU acceleration tier works end-to-end.
- **llama.cpp is a multi-file bundle, not a single binary.** The Vulkan release extracts to ~50 DLLs including a 48 MB `ggml-vulkan.dll`, unlike Mutagen's single self-contained binary. The sidecar bundling must ship the whole directory, not one `externalBin` file (recorded in `02_dev.md`).

Still not spiked (deferred to the dev plan): the reverse host→container port forward for in-sandbox agent access (the Phase G gating spike in `02_dev.md`).

## Open Questions

- **Acceleration tier strategy**: Ship only the portable Vulkan llama.cpp build for v1, or also wire the Foundry Local NPU/GPU tier (the validation machine has both an Intel AI Boost NPU and winget-installable Foundry Local)? The Foundry tier is materially faster on Copilot+ hardware but serves ONNX rather than GGUF, depends on a first-party system component, and means maintaining two engine code paths behind one OpenAI-compatible contract; deciding the v1 scope before the dev plan avoids building both at once. If the tier ships, decide whether the app installs Foundry Local on the user's behalf (via winget) or only uses it when already present.
- **Model launch catalogue**: Which exact models and quantizations ship in the Windows v1 catalogue (Phi-3-mini Q4 is the assumed default), and what RAM floor maps each to "recommended" vs "will run slowly" vs "won't fit"? The recommendation thresholds must be pinned numerically in `02_dev.md`.
- **Agent/sandbox endpoint wiring**: How does a sandboxed agent discover and use the host inference endpoint — a reverse Mutagen forward (host port → container) plus an `OPENAI_BASE_URL`-style env var written into the workspace, and which agents in the catalogue support a configurable base URL? Whether any launch agent can actually be pointed at a local OpenAI-compatible endpoint is unconfirmed.
- **Model storage location and sharing**: Store weights under the Tauri app data dir (per the `dirs` crate), and should they be shared across all sandboxes (one download, many consumers) or scoped per sandbox? Shared storage saves disk but couples model lifecycle to the app rather than to any one sandbox.
- **UI entry point**: A top-level "Local Models" route (one inference server shared app-wide) or a per-`SandboxItem` panel (consistent with the agents UX)? The host-sidecar architecture implies one app-wide server, which argues for a top-level route — but the per-sandbox panel matches the existing model.
- **Concurrency and memory pressure**: Should the app allow only one model loaded at a time (the host-sidecar holds gigabytes resident), and what happens when the user opens a heavy sandbox workload alongside a loaded model on a 16 GB machine? A loaded-model memory guard may be needed.
- **macOS divergence**: On Apple Silicon the natural engine is llama.cpp with the Metal backend (or MLX), and the recommended model tier differs from Windows. The catalogue and engine-selection seams must stay platform-parameterised so the deferred macOS work slots in without reshaping the architecture.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Model fails to load or OOM-kills when run inside a sandbox container | The Podman machine VM is allocated only 2 GiB of RAM (`podman machine list`), below a Phi-3-mini Q4 GGUF's ~2.3 GB resident footprint | Run the inference server as a host sidecar, not in the container; never assume container-VM RAM reflects host RAM |
| GPU acceleration silently absent despite a capable GPU | A CUDA-only llama.cpp build finds no Nvidia device on Intel Arc / AMD hosts and falls back to CPU | Bundle the Vulkan backend build, which accelerates on Intel Arc, AMD, and Nvidia from one binary |
| `nvidia-smi`-based GPU detection reports "no GPU" on machines that have one | `nvidia-smi` only exists for Nvidia GPUs; Intel/AMD GPUs are invisible to it | Enumerate GPUs via `Win32_VideoController` (or a cross-platform Rust GPU crate), not `nvidia-smi` |
| Recommendation engine suggests a model that then runs unusably slowly | Probing the container VM (2 GiB, 4 CPUs) instead of the host yields wrong specs; or RAM-floor thresholds set too low | Probe host specs from Rust (`sysinfo`), and pin per-model RAM floors numerically in the catalogue with headroom for OS overhead |
| Bundled engine binary not found at runtime | Tauri resolves sidecars by target triple (`<name>-<target-triple>`); a missing per-platform binary in `bundle.externalBin` fails the spawn | Bundle one engine binary per supported target triple, exactly as `src-tauri/src/sync/mod.rs` does for `mutagen-<target-triple>` |
| Orphaned inference server process after app exit | The host sidecar holds gigabytes resident and outlives the app if not stopped | Shut the server down in the `RunEvent::Exit` hook in `src-tauri/src/lib.rs`, mirroring the Mutagen daemon shutdown |
| Sandboxed agent cannot reach the local model | The inference server binds host loopback, which is not directly visible inside the container | Expose the host port into the container via a reverse Mutagen forward (the `sync::forward_create` primitive, used in reverse) and point the agent at it via a base-URL env var |
| Truncated or corrupted model download appears to succeed | Hugging Face downloads can be interrupted without an explicit error at the file level | Verify the downloaded GGUF against a catalogue-supplied checksum as an explicit step in the `download_model` flow before marking it ready |
