---
type: feature
title: "chat-agent-anatomy"
description: "A learning mode for the native chat that outlines each part of a live agent run as Brain or Arms and plays an animated brain-and-arms stage beside the messages, reusing the onboarding book's stage engine."
status: implemented
depends_on: [native-agent-ui, onboarding-story, fastest-time-to-wow]
architecture: [docs/architecture/workspace-ui.md]
---

# chat-agent-anatomy

A learning mode for the native chat that outlines each part of a live agent run as Brain or Arms and plays an animated brain-and-arms stage beside the messages, reusing the onboarding book's stage engine.

## Status note

Phases A to F are implemented: the stage engine moved to `src/lib/story-stage/`, the mock agent host replays two tool-call traces, the adapter opens tool parts at `toolcall_start`, and learning mode annotates messages, shows the companion panel and plays the live stage. Phase G (Paraglide translations) is deferred until the user approves the working feature; every new string carries a `TODO(i18n)` marker.
