---
type: improvements
title: "Local LLM Reachability Improvements"
feature: local-llm-reachability
---
# Local LLM Reachability Improvements

Work deferred out of `docs/features/local-llm-reachability/01_ideas.md`, with the reason each was deferred.

## Stable Host Relay Port

An in-app TCP relay on a host loopback port that stays fixed for the app's lifetime would let per-sandbox inference forwards survive engine restarts. Without it, every forward is recreated on each server start. Deferred because server restarts are user-initiated and rare, and recreating a few Mutagen sessions is cheap.

## Local Model for Deployed Sandboxes

A sandbox deployed to a remote Podman host could reach the laptop's inference server through a reverse forward over its remote alias. Deferred because the request latency and the laptop-must-stay-awake dependency need their own design. The old `host.containers.internal` URL never worked for deployed sandboxes either.

## Existing Forwards on Stopped microsandbox Sandboxes

The 2026-11-10 probe showed that a Mutagen dial against a stopped microsandbox sandbox makes `msb exec` try to boot the VM without the app's start environment (`docs/features/local-llm-reachability/spike/01_mutagen-reverse-msb.md`). Terminal and port forwards stay registered across a plain stop (`docs/architecture/transport.md`), so their reconnect loops may do the same. Deferred because it predates this feature; Open Question 1 of `01_ideas.md` settles it for inference forwards only.

## Foundry Served Model Ids

Provisioning writes the catalogue's model ids and uses the running model's id as the agents' default. `start_inference` receives a Foundry catalogue alias (`foundryAlias` in `src/lib/models/catalogue.ts`), while Foundry may serve the model under a device-specific variant id. An agent that sends the alias may then be rejected by Foundry. llama-server ignores the model id, so the llama.cpp engine is unaffected. Deferred because it predates this feature and needs a Foundry spike to read the served id from `foundry service status` or `/v1/models`.

## Port Poller Boots a Stopped microsandbox Sandbox

The 2026-11-10 spike stopped `rrr` from the CLI. The app's port poller (`scan_listening_ports` in `src-tauri/src/network/mod.rs`) kept running `msb exec <name> -- sh -c cat /proc/net/tcp ...` every 2 s, and each call made microsandbox try to boot the VM. The sandbox flipped between `stopped` and `starting` until the app was told to start or stop it. A stop the app initiates presumably ends the poller first; a stop made outside the app (the CLI, a crash, the host sleeping) does not. A fix would have the poller check the instance state through `msb inspect` before calling `exec`, or use an exec mode that never starts a VM. Deferred because the poller predates this feature.

## Sandbox Stop Racing a Spawned Inference Ensure

`stop_sandbox` closes the sandbox's inference forward before stopping it, but an `ensure_for_sandbox` spawned just earlier (by `watch_workspace_ports` or `start_sandbox`) can pass its running check before the stop and create the forward after the close. The forward then dials the stopped sandbox, which for microsandbox repeats the boot attempt from the spike, until the next server stop, sandbox stop or app exit. A fix would serialise ensure and close per sandbox inside `src-tauri/src/services/inference_reach.rs`. Deferred because the window is a few hundred milliseconds around a user-initiated stop, and the plan accepted the same class of race for server stop (`02_dev.md`, Lifecycle, Lock discipline).
