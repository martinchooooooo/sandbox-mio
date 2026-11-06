---
type: architecture
title: "Testing"
description: "The verification layers, the process-level mock seam behind the fast e2e tier, and how concurrent e2e runs stay isolated; spec-writing rules are in e2e/AGENTS.md."
generated:
  by: claude-code/opus-5-5
  at: 2026-11-05T23:55:00+11:00
stale_after: 2027-02-05T00:00:00+11:00
code:
  - e2e/wdio.conf.ts
  - e2e/lib/
  - src-tauri/src/mockctl/
  - src-tauri/src/process/
  - docs_check.rs
sources:
  - resource: docs/features/e2e-tiering/02_dev.md
  - resource: docs/features/scriptable-mock-backend/02_dev.md
  - resource: docs/features/parallel-worktree-e2e/02_dev.md
---
# Testing

Sandbox-mio is tested mainly end to end: WebdriverIO drives the real debug build of the app against three tiers of real infrastructure. Rust unit tests cover pure logic only, and the frontend is type-checked. This document explains the ideas that make that work. The commands and their cadence are in the root `AGENTS.md`; the rules for writing specs are in `e2e/AGENTS.md`.

## Layers and Tiers

- **Static checks:** the frontend type-check, the Rust compile, and the docs check.
- **Rust unit tests:** pure functions only, such as argument rendering, policy validation, parsers and the pane registry.
- **e2e tiers**, chosen by where a spec lives:
  - `light`: no Podman; the app runs against a scripted process layer. Seconds. Proves UI, routing, stores and command round trips.
  - `standard`: real sandboxes on a dedicated test Podman machine, plus a second machine as a deploy target. Minutes. The default tier and the source of regression truth.
  - `heavy`: real model downloads and inference, and deploy-forward chains. Many minutes.
- No CI runs any tier.

## The Mock Seam

- The `light` tier replaces what sits below the subsystems, not the subsystems themselves. Every one-shot `podman`, `msb` and Mutagen sync call goes through a process runner, and one environment switch swaps the real runners for scripted ones. The real backend, machine manager, sync engine and providers run unmodified, so their output parsing and error mapping are exercised.
- A shared director holds per-call overrides keyed by binary and subcommand, journals every call it answers, and otherwise falls back to small built-in defaults (an in-memory container list, a running machine). Specs drive the director through IPC commands that refuse to run outside mock mode.
- The runtime and network subsystems still use hand-written in-memory mocks.
- Streaming child processes cannot be scripted, so code that spawns one checks the mock switch directly.
- The mock control plane has no Rust unit tests by design: each scripted scenario is proven by a light spec ([scriptable-mock-backend plan](../features/scriptable-mock-backend/02_dev.md), Resolved Decision 9).

## Run Isolation

Several worktrees can run `light` at once, alongside at most one `standard` or `heavy` run on the host. Isolation hangs off one run slug derived from the worktree's path:

- App data, settings, credentials, workspaces and msb state move to slug-keyed directories through environment variables. This works only because the app derives every path from its app-data root.
- Each run takes its own port window, and stale-process cleanup matches the worktree's own build path, never a process name.
- A worktree lock stops two runs in one worktree. A host lock stops two real-infrastructure runs, because the test machines and the deploy flow's `~/.ssh` files cannot be split per worktree.

## Harness Lifecycle

- Before a run, the harness takes its locks, resets the run's directories, seeds settings past onboarding, provisions the test machines and base image (skipped for `light`), and builds the debug app.
- Each session launches the WebDriver with the run's environment, which the app inherits. A session gets mock mode only when every spec in that session is a `light` spec.
- After a run, the harness removes the run's workspaces and, for real-infrastructure runs, the managed SSH config, and stops the remote test machine.

## Invariants

- A spec's tier is decided by where the spec lives, not by command-line flags.
- Every wait uses a named budget, because UI state arrives through events and polling rather than command responses.
- Specs never hard-code the base image reference; they derive it from the backend's declaration.
