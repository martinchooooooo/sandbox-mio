---
type: architecture
title: "Local Inference"
description: "Running a language model on the host (hardware probe, recommendation, downloads, the llama.cpp and Foundry Local engines) and offering it to sandboxes."
generated:
  by: claude-code/sonnet-5-5
  at: 2026-11-23T12:00:00+11:00
stale_after: 2027-02-21T00:00:00+11:00
code:
  - src-tauri/src/inference/
  - src-tauri/src/commands/inference.rs
  - src/lib/models/
  - src/routes/models/
  - src-tauri/src/services/inference_reach.rs
sources:
  - resource: docs/features/local-llm/02_dev.md
  - resource: docs/features/models-page-caching/02_dev.md
  - resource: docs/features/local-llm-reachability/02_dev.md
  - resource: docs/features/native-agent-ui/02_dev.md
  - resource: docs/features/remove-microsandbox/02_dev.md
  - resource: docs/features/local-models-ux/02_dev.md
---
# Local Inference

The app can run a language model on the user's machine and offer it to sandboxes, so OpenCode and pi work without a cloud API. The host's hardware is probed, a model recommended and downloaded, and an OpenAI-compatible server started through one of two engines.

## Probe, Recommend, Download

- The hardware probe (RAM, GPU, NPU) is slow, so its result is cached in memory for the life of the process and refreshed only on request. The probe result is never written to disk.
- A frontend model catalogue is ranked against the probed specs, rating each model by free RAM against the model's requirement.
- Model weights download into the app-data root, reporting progress as an activity.

## One Model Job in the Frontend

- A single frontend store owns the model job (download, wake-up, serving status, last error). The Models page and the first-run onboarding both read it, so progress survives navigation and only one job runs at a time.
- Setup steps reach the user as plain narration: one line naming the step now running. The engine's technical plan stays available on demand, in the onboarding's "For those who know" disclosure and on the Settings page.

## Engines

| Engine | How it ships | Notes |
|---|---|---|
| llama.cpp (Vulkan) | Bundled `llama-server`, staged as a resource directory | Tuning knobs come from app settings; always started with an explicit context size |
| Foundry Local | Microsoft's `foundry` CLI, installed through winget on request | Picks an NPU, GPU or CPU variant per device; loads models non-interactively |

- Each engine declares its setup steps as data, so the UI can show the whole plan before running it.
- Agents learn each model's context window from the catalogue, capped at the engine's context size.
- Starting picks a free host loopback port, starts the engine, and returns once the server's health check passes. One server runs at a time.

## Reaching the Server from Sandboxes

- Each running local sandbox gets its own reverse Mutagen forward: a listener on the sandbox's own loopback, tunnelled over the sandbox's SSH alias to the server's host port ([Transport](transport.md)). Agents therefore use one loopback URL.
- A reach service in the backend owns the forward set. It ensures a sandbox's forward when a server starts, and when the sandbox is created, started, re-applied or its workspace viewed. It closes forwards when the server stops, before their sandbox stops, and at app exit.
- A server restart lands on a new host port, so every forward is recreated against it.
- Deployed sandboxes get no forward.
- A forward that cannot be created is logged, never raised: the server still works from the host.
- Guest loopback traffic never meets a sandbox's proxy or egress policy ([Network](network.md)).

## Invariants

- An inference listener binds sandbox loopback only; nothing outside its sandbox can reach it.
- A sandbox's inference forward is closed before the app stops the sandbox, and created only for running sandboxes: dialling a stopped microVM's alias tries to boot it without the app's start environment.
- The model job has one owner: no page keeps its own copy of download or wake-up state.
- The in-sandbox port is one value shared by the backend and the frontend's local-model wiring.
- Every catalogue model's GGUF declares an architecture the pinned llama.cpp release can load; adding a model of a newer architecture means bumping that release too.
