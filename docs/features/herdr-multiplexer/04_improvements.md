---
type: improvements
title: "Herdr Multiplexer Improvements"
feature: herdr-multiplexer
---
# Herdr Multiplexer Improvements

Worthwhile ideas kept out of the first build. The first build swaps the multiplexer and nothing else.

## Surface agent state in the app chrome

Herdr's socket API (`herdr agent list`, `herdr agent wait`) reports each agent's working, blocked, done or idle state. The app could poll it through the engine exec and show a blocked or done indicator on the sandbox in the sidebar and a toast through the activity center. Deferred because it needs a polling or subscription seam in Rust, a UI design, and a decision on how often to exec into a sandbox, none of which the user's request requires; Herdr already shows the state inside its own sidebar.

## Install agent lifecycle hooks

`herdr integration install claude` (and its siblings for opencode and pi) writes hooks that give Herdr exact agent state instead of screen-matching. Deferred because the hooks would be written into each agent's own config, which the agent catalogue and the credential flow own (`docs/architecture/agents.md`), and screen matching may be good enough.

## Launch agents from Manage into a Herdr pane

The Manage tab's coding-agent launchers could start an agent in a named Herdr pane through `herdr agent start`, so a launched agent lands in the terminal already tracked. Deferred because it changes the launcher flow and depends on the agent-state improvements above.

## Install Herdr through mise

The base image downloads the pinned Herdr release asset and checks its sha256 because mise 2026.6.14 fails the artifact attestation check on the Herdr release. After the next mise bump in `src-tauri/resources/Containerfile`, retry `mise install herdr@<version>` and, if attestation passes, move the install there like the other tools. Deferred because bumping mise changes an unrelated pin.

## Pre-warm the Herdr session

Open the SSH session when a sandbox is selected. Deferred for the reason native-terminal Q5 gives: the `embedded-workspace` spec asserts the terminal is not started eagerly.
