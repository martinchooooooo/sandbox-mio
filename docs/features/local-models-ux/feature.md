---
type: feature
title: "local-models-ux"
description: "Rework the Models page and its teaching steps to match the onboarding's voice and components, and make the model list meaningful to non-technical users: plain descriptions, human-scale tradeoffs, and an explanation for every model this computer cannot run."
status: implemented
depends_on: [local-llm, fastest-time-to-wow, onboarding-story, chat-agent-anatomy]
architecture: [docs/architecture/inference.md]
---

# local-models-ux

Rework the Models page and its teaching steps to match the onboarding's voice and components, and make the model list meaningful to non-technical users: plain descriptions, human-scale tradeoffs, and an explanation for every model this computer cannot run.

## Status note

All phases are implemented: catalogue metadata and helpers, shared Explainer, shared model store with onboarding narration, the Models page rework, the Settings re-home, and the copy moved into Paraglide with German and Russian translations. Frontend-only: no new Tauri command, host-global state or Rust change. The first-run card's own page-wide `TODO(i18n)` literals predate this feature and remain with fastest-time-to-wow.
