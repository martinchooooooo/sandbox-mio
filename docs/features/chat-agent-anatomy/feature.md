---
type: feature
title: "chat-agent-anatomy"
description: "A learning mode for the native chat that labels each part of a live agent run as Brain or Arms with a small animated avatar beside it, using the onboarding book's vocabulary."
status: implemented
depends_on: [native-agent-ui, onboarding-story, fastest-time-to-wow]
architecture: [docs/architecture/workspace-ui.md]
---

# chat-agent-anatomy

A learning mode for the native chat that labels each part of a live agent run as Brain or Arms with a small animated avatar beside it, using the onboarding book's vocabulary.

## Status note

Phases A to D, H, I and J are implemented: the stage engine moved to `src/lib/story-stage/`, the mock agent host replays two tool-call traces, the adapter opens tool parts at `toolcall_start`, and learning mode shows each assistant part as a participant of a group chat: a small animated avatar in a left margin, a name line, and an info icon whose hover card explains the role. The companion panel and live stage of Phases E and F were superseded by that redesign. Phase G (Paraglide translations) is deferred until the user approves the working feature; every new string carries a `TODO(i18n)` marker.
