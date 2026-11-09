---
type: architecture
title: "Local Inference"
description: "Running a language model on the host (hardware probe, recommendation, downloads, the llama.cpp and Foundry Local engines) and offering it to sandboxes."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-09T12:00:00+11:00
stale_after: 2027-02-05T00:00:00+11:00
code:
  - src-tauri/src/inference/
  - src-tauri/src/commands/inference.rs
  - src/lib/models/
  - src/routes/models/
sources:
  - resource: docs/features/local-llm/02_dev.md
  - resource: docs/features/models-page-caching/02_dev.md
---
# Local Inference

The app can run a language model on the user's machine and offer it to sandboxes, so OpenCode and pi work without a cloud API. The host's hardware is probed, a model recommended and downloaded, and an OpenAI-compatible server started through one of two engines.

## Probe, Recommend, Download

- The hardware probe (RAM, GPU, NPU) is slow, so its result is cached in memory for the life of the process and refreshed only on request. The probe result is never written to disk.
- A frontend model catalogue is ranked against the probed specs, rating each model by free RAM against the model's requirement.
- Model weights download into the app-data root, reporting progress as an activity.

## Engines

| Engine | How it ships | Notes |
|---|---|---|
| llama.cpp (Vulkan) | Bundled `llama-server`, staged as a resource directory | Tuning knobs come from app settings |
| Foundry Local | Microsoft's `foundry` CLI, installed through winget on request | Picks an NPU, GPU or CPU variant per device; loads models non-interactively |

- Each engine declares its setup steps as data, so the UI can show the whole plan before running it.
- Starting picks a free host loopback port, starts the engine, and returns once the server's health check passes. One server runs at a time.

## Reaching the Server from Sandboxes

- Once the server is up, a reverse Mutagen forward makes the Podman machine listen on a fixed port and relay to the server; sandboxes reach it at `host.containers.internal` on that port ([Transport](transport.md)).
- The forward runs through the Podman machine whichever provider a sandbox uses. If the forward fails, usually because the machine is down, the server still works from the host and the failure is logged rather than raised.
- Proxied sandboxes send this traffic around their proxy ([Network](network.md)).

## Invariants

- The reverse forward listens on all interfaces of the Podman machine; a loopback-only listener is unreachable from containers.
- The machine-side port is one value shared by the backend and the frontend's local-model wiring.
- Every catalogue model's GGUF declares an architecture the pinned llama.cpp release can load; adding a model of a newer architecture means bumping that release too.
