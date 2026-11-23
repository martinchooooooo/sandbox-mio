---
type: feature
title: "local-models-ux"
description: "Rework the Models page and its teaching steps to match the onboarding's voice and components, and make the model list meaningful to non-technical users: plain descriptions, human-scale tradeoffs, and an explanation for every model this computer cannot run."
status: in-progress
depends_on: [local-llm, fastest-time-to-wow, onboarding-story, chat-agent-anatomy]
architecture: [docs/architecture/inference.md]
---

# local-models-ux

Rework the Models page and its teaching steps to match the onboarding's voice and components, and make the model list meaningful to non-technical users: plain descriptions, human-scale tradeoffs, and an explanation for every model this computer cannot run.

## Status note

Phases A–E are implemented (catalogue metadata and helpers, shared Explainer, shared model store with onboarding narration, the Models page rework, the Settings re-home). Phase F remains: the user's copy review, then the Paraglide pass. The Open Questions were resolved with the ideation's recommendations, pending the user's copy review in Phase F. Frontend-only: no new Tauri command, host-global state or Rust change.
