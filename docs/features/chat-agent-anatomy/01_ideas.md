---
type: ideas
title: "Agent Anatomy in the Native Chat"
feature: chat-agent-anatomy
---
# Agent Anatomy in the Native Chat

This document scopes a hands-on follow-up to the onboarding book: the native chat, which drives pi in RPC mode inside a sandbox, shows which parts of a live agent run are the Brain (the model) and which are the Arms (the agent harness, pi). Every part of an assistant message gets a role outline and badge, and a companion panel to the left of the messages plays an animated brain-and-arms stage that follows the run as it happens. The onboarding book teaches the Brain and Arms vocabulary by reading (`src/lib/onboarding/chapters/agents-and-sandboxes/`); this feature lets the user watch the same vocabulary act on their own prompts, first in the `hello-world` sandbox the onboarding hands off to. The chat code lives in `src/lib/agent-chat/` (`pi-events.ts`, `MessageList.svelte`, `ToolPart.svelte`, `ReasoningPart.svelte`, `ChatView.svelte`, `ChatHeader.svelte`), the stage animation engine in `src/lib/onboarding/stage/`, and the book's stage component in `src/lib/onboarding/book/Stage.svelte`. MCP badges on tool calls belong to `chat-mcp-learning` ([01_ideas.md](../chat-mcp-learning/01_ideas.md)), and the guided hands-on checklist is deferred ([04_improvements.md](04_improvements.md)).

## Current State

- **pi's stream is already split into segments.** `applyPiRecord()` in `src/lib/agent-chat/pi-events.ts` folds pi's records into `reasoning`, `text`, `tool` and `notice` parts (`ChatPart` in `src/lib/agent-chat/types.ts`). No new RPC parsing is needed to tell thinking, replies and tool calls apart.
- **The local model emits thinking.** The recorded stream `src-tauri/src/agent_host/fixtures/01-roundtrip.jsonl` carries `thinking_start`, `thinking_delta` and `thinking_end` events, and `ReasoningPart.svelte` renders them as a collapsible "Thinking…" or "Thought" trace.
- **A tool part already encodes ask-then-do.** A tool part enters state `input` when the model's message finishes a `toolCall` (the `toolcall_end` delta, or `assistantParts()` on `message_end`), moves to `running` on `tool_execution_start`, and ends in `output` or `error` on `tool_execution_end` (`ToolState` in `src/lib/agent-chat/types.ts`). The `input` state is the Brain's request and the `running`→`output` span is the Arms' work.
- **No recorded stream contains a tool call.** None of the fixtures in `src-tauri/src/agent_host/fixtures/` holds a `tool_execution_*` record, so the timing and ordering of a local-model tool call are unobserved.
- **Tool rendering is one box per call.** `ToolPart.svelte` renders one `Trace` per tool part, keyed by `toolName` (`bash`, `read`, `write`, `edit`, generic fallback), with the arguments and output inside one collapsible.
- **The chat column is centred and narrow.** `MessageList.svelte` and the composer in `ChatView.svelte` cap content at `max-w-3xl`, so a wide window already has unused space beside the messages. The chat pane renders in the main webview with no child webview, so its inner layout is frontend-only and independent of the Rust pane rectangle (`docs/architecture/workspace-ui.md`).
- **The stage engine is separate from the book, but script-driven.** `createStage()` in `src/lib/onboarding/stage/index.ts` takes any `<svg>` and returns the cast (`you`, `chat`, `brain`, `pc`, `host`, `box`), arms (`reach`, `typing`, `armsName` in `stage/arms.ts`), tool tiles (`ToolKind` in `stage/tools.ts`) and effects. Scenes are scripted `play()` timelines that a page turn cancels (`cancelAll()` in `stage/core.ts`), and `Stage.svelte` adds captions and a Replay button.
- **The stage assumes a wide board.** The viewBox is 1000 x 440 (`Placement` in `stage/board.ts`), `createBoard()` parks hidden actors at the board centre, and `stage/stage.css` caps the SVG at `max-height: 34vh`.
- **The book's vocabulary fixes the roles.** The chapter calls the model "the Brain" and the agent harness "the arms" (`ARM_LABEL` in `stage/arms.ts`, Act 2 "Someone had a clever idea"), tools are what the arms carry (Act 3 "The arms carry tools"), and Act 2 "The new loop" shows the Brain-Arms-result cycle.
- **The first sandbox is named `hello-world`.** `FIRST_SANDBOX` in `src/lib/onboarding/setup.svelte.ts`; `startHandoff()` in `src/lib/onboarding/handoff.svelte.ts` opens its chat at the end of onboarding.

## Roles: the anatomy classifier

A pure function in a new `src/lib/agent-chat/anatomy.ts` maps each `ChatPart` (and, for tool parts, each `ToolState`) to a role. The classifier reads existing chat state only; `pi-events.ts` stays unchanged.

| Role | Source | Story meaning |
|---|---|---|
| `brain-think` | `reasoning` part | The Brain reasons before answering or acting |
| `brain-reply` | `text` part | The Brain talks to you through the chat |
| `brain-ask` | `tool` part, any state (its request half) | The Brain decides which tool the Arms should use, and with which arguments |
| `arms-act` | `tool` part, states `running`, `output`, `error` (its execution half) | The Arms carry the tool out on the sandbox computer |
| `result` | `tool` part output | What the Arms bring back to the Brain for the next turn |

The classifier also maps a tool name to a stage `ToolKind`: `bash` → `terminal`, `read` → `read`, `edit` and `write` → `edit`, `grep`, `find` and `ls` → `find`, anything else → a generic tile. A separate function derives the run's current beat (idle, thinking, replying, asking, acting, returning) from the last streaming part and the running tool parts, for the live stage to follow.

## Annotated messages

- **Outlines and badges:** in learning mode, `MessageList.svelte` wraps each assistant part in a role frame: a coloured outline and a small corner badge such as "Brain · thinking" or "Arms · terminal". The frame is a new `src/lib/agent-chat/RoleFrame.svelte`, so `ReasoningPart.svelte` and `ToolPart.svelte` keep their markup.
- **Split tool box:** `ToolPart.svelte` renders a tool part as two bands in learning mode. The top band, in the Brain colour, reads "Brain asked: terminal" with the command or path. The bottom band, in the Arms colour, appears on `tool_execution_start` and holds the running indicator and then the output. Outside learning mode the tool box renders as today.
- **Clickable badges:** a badge click selects the part, which drives the companion panel's explainer and highlights the matching actor on the stage. Clicking "Brain · thinking" also expands the reasoning trace, which answers "click on thinking to see the reasoning" directly.
- **Role colours:** two theme tokens, Brain and Arms, are added to `src/app.css` for light and dark mode. `src/lib/onboarding/stage/stage.css` mirrors the light values as `--s-*` tokens, with a path comment at both ends.

## The companion panel

- **Placement:** `ChatView.svelte` becomes a two-column layout in learning mode: the companion panel on the left, the existing header, message list and composer on the right. The panel is a new `src/lib/agent-chat/AnatomyPanel.svelte`.
- **Contents, top to bottom:** the live stage; one sentence naming the current beat ("The Brain is thinking…", "The Arms are running a command in your sandbox"); and the explainer for the selected part, two or three sentences in the book's voice with the book's terms.
- **Narrow windows:** below a container width, chosen with a Tailwind container query on the chat pane rather than the window, the panel collapses to a header button that opens the panel in a `sheet` (`src/lib/components/ui/sheet/`). The app sidebar already occupies the window's left edge, so mid-width windows hit the collapse first.

## The live stage

- **Component:** a new `LiveStage.svelte` mounts `createStage()` on its own `<svg>` with no captions and no Replay button. The book keeps `src/lib/onboarding/book/Stage.svelte` for scripted scenes.
- **Cast:** `brain`, the arms, and `box` (the sandbox computer), in a portrait layout that suits a tall, narrow panel. The `chat` actor is left out, because the real chat sits beside the panel.
- **Live driver:** a new `src/lib/agent-chat/anatomy-driver.ts` turns beat changes from the classifier into stage calls. Thinking pulses the brain with thought marks; a tool call shows the chosen tool tile at the brain; execution `reach()`es the arms into `box` holding that tile with `typing(true)`; a result travels back along the arms to the brain; a reply sends a bubble off-panel toward the chat column.
- **Beat queue:** the driver queues beats and gives each a minimum on-screen duration, so a tool that finishes in milliseconds still reads as reach, work and return. When the queue grows past a few beats, the driver drops intermediate beats and jumps to the newest, so the stage never lags a long run.
- **Live events only:** the driver animates beats from records that arrive while the panel is mounted. A fresh attach rebuilds history through `messagesFromHistory()` in `pi-events.ts`, and the driver settles the stage to the final beat of that history without replaying it.
- **Reduced motion:** under `prefers-reduced-motion: reduce` the driver places actors and swaps highlights without tweens, matching the existing reduced-motion rule in `src/app.css`.

## Learning mode

- **Default:** learning mode is on by default in the `hello-world` sandbox and off elsewhere. A toggle in `ChatHeader.svelte` turns learning mode on or off for the sandbox in view.
- **State:** the toggle lives on the per-sandbox chat store returned by `ensureChat()` in `src/lib/agent-chat/chat.svelte.ts`, so it survives route changes. Persistence across app restarts is Open Question "Learning-mode persistence".

## Onboarding refactor

- **Move the engine:** `src/lib/onboarding/stage/` moves to a neutral `src/lib/story-stage/`, because the chat would otherwise import from the onboarding feature. `src/lib/onboarding/AGENTS.md` and the book components update their imports and the layout table.
- **Board size becomes a parameter:** `createBoard()` in `stage/board.ts` takes the board width and height instead of assuming 1000 x 440, and parks hidden actors at the given centre. `Stage.svelte` keeps passing 1000 x 440.
- **Live use of the timing core:** `stage/core.ts` keeps `sleep`, `tween` and `cancelAll` unchanged; the live driver calls `cancelAll()` when it jumps to the newest beat, the same way a page turn does.
- **Unchanged:** the book kit (`src/lib/onboarding/book/`), the chapter and its scenes, and `setup.svelte.ts`.

## Technology Choices With Tradeoffs

### Panel visual: the existing SVG stage engine, driven live

- **Chosen:** reuse `createStage()` with a live driver. The panel then shows the same brain, arms, sandbox and tool tiles the user just read about, drawn by the same code, so the hands-on step reads as a continuation of the book.
- **Rejected: a static diagram of brain, arms and sandbox icons that light up.** The user asked for an animated, polished panel, and a static diagram breaks the visual link to the book's animations.
- **Rejected: pre-rendered animation loops (Lottie, Rive or video), one per beat.** Loops add a runtime dependency and an asset pipeline, cannot show the actual tool kind or command, and would drift from the book's hand-drawn style.
- **Rejected: a new CSS-only animation component.** A CSS-only component re-implements the arms, actors and tool tiles that `src/lib/onboarding/stage/` already draws, and two drawings of the same cast would diverge.

### Where roles are computed: a derived classifier in the frontend

- **Chosen:** `anatomy.ts` derives roles from the existing `ChatPart` list. `pi-events.ts` stays a faithful adapter of pi's protocol, and the classifier is pure and unit-testable against recorded fixtures.
- **Rejected: a `role` field added to each part inside `applyPiRecord()`.** Roles are a teaching interpretation, not part of pi's protocol, and mixing them into the adapter couples learning copy to the pinned pi version's record handling.
- **Rejected: tagging in the Rust attach stream (`src-tauri/src/agent_host/`).** The Rust side relays journal records without interpreting them, and the frontend already holds every field the roles need.

### Tool call presentation: one box, two bands

- **Chosen:** one tool box split into a Brain band (request) and an Arms band (execution). The split matches both pi's events (`toolcall_end` then `tool_execution_*`) and the book's story that the Brain decides and the Arms act.
- **Rejected: a single "Arms" box per tool call.** A single box teaches that the Arms choose the tool, which contradicts the book and hides the Brain's decision.
- **Rejected: two separate parts in the message list.** Separate parts break the collapsible trace pattern, double the vertical space of every tool call, and separate the request from its output in long runs.

### Driving the stage: a beat queue over derived state

- **Chosen:** the driver watches the derived current beat and plays queued beats with minimum durations, dropping stale beats under load.
- **Rejected: one scripted scene per pi record.** pi emits dozens of delta records per second while streaming, and per-record scenes would either stutter or fall behind.
- **Rejected: mirroring state with no minimum durations.** A fast tool call would flash the arms for one frame, so the reach, work and return would never read.

### Panel placement: left of the messages, inside the chat pane

- **Chosen:** a left column inside `ChatView.svelte`. The panel reads first ("here is the anatomy, now watch it"), and the composer's right gutter stays free for the activity bell fixed in the window corner.
- **Rejected: a right-hand column.** The activity bell already crowds the bottom-right corner, and the right side is where the user's own messages align.
- **Rejected: a window-level column outside the chat pane.** A window-level column would change the Rust layout constants that the frontend sidebar width must equal (`docs/architecture/workspace-ui.md`, Invariants), for a panel only the chat uses.
- **Rejected: a sheet only, at every width.** A sheet covers the messages, so the user cannot watch the stage and the chat together, which is the point of the panel.

### Stage engine home: move to `src/lib/story-stage/`

- **Chosen:** move the engine to a neutral directory both features import.
- **Rejected: import `src/lib/onboarding/stage/` from the chat directly.** The chat would depend on the onboarding feature's internals, and `src/lib/onboarding/AGENTS.md` describes the stage as the book's engine.
- **Rejected: copy the engine into `src/lib/agent-chat/`.** Two copies of the cast drift apart as the book's drawings change.

### Learning-mode default: on in `hello-world` only

- **Chosen:** on by default in `hello-world`, the sandbox the onboarding hands off to, and a toggle everywhere.
- **Rejected: on for the first N chat sessions in any sandbox.** A session count needs a persisted counter and turns the panel off at an arbitrary moment the user did not choose.
- **Rejected: off everywhere until switched on.** A user fresh from the book would never discover the panel.

## Validation

The design rests on the pi records the chat already handles (`src/lib/agent-chat/pi-events.ts`), the recorded RPC streams in `src-tauri/src/agent_host/fixtures/`, and the stage engine's current API (`src/lib/onboarding/stage/`). The planned spike was first skipped, then run on 2026-11-22: [spike/01_tool-call-stream.md](spike/01_tool-call-stream.md) records five local-model tool-call runs with pi 1.5.0. It answered all four questions this section posed:

- **Record order for one call:** the Brain's request streams as `toolcall_start`, then `toolcall_delta`s; `toolcall_end` arrives together with `thinking_end` at the end of the message; `tool_execution_start` follows within 1 ms.
- **Typical tool execution:** 7 to 90 ms.
- **Thinking before tool calls:** yes, before every assistant message, on both the 4B and the 2B model.
- **Concurrent calls:** pi runs tool calls of one message in parallel by default.

The spike also found that `thinking_end` is held until the message ends, that only `toolcall_start` gives the Brain's request any on-screen time, and that a run can fail and auto-retry mid-run. `02_dev.md` takes these into account.

## Open Questions

- **Learning-mode persistence:** whether the per-sandbox toggle survives an app restart. Options: an entry in `settings.json` through `src-tauri/src/settings/mod.rs`, browser `localStorage` in the webview, or no persistence (the default applies on every launch).
- **Identifying the onboarding sandbox:** whether learning mode keys on the name `hello-world` (`FIRST_SANDBOX`), which a user can also create by hand, or on a marker the onboarding records when it creates the sandbox.
- **Concurrent tool calls on the stage:** the stage has one pair of arms. If pi runs several tool calls at once, options are showing the newest call with a count badge, or playing the calls one after another.
- **Explainer copy source:** whether the explainer sentences live in the chat (`src/lib/agent-chat/`) or in the chapter's shared copy (`src/lib/onboarding/chapters/agents-and-sandboxes/copy.ts`) so the book and the chat share one wording.
- **Reply bubble on the stage:** whether a `brain-reply` beat animates toward the real chat column across the panel edge, or stays inside the stage as a small bubble.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Hidden actors fly in from the wrong point of the portrait stage | `createBoard()` in `src/lib/onboarding/stage/board.ts` parks hidden actors at the centre of a 1000 x 440 board | Pass the board size into `createBoard()` and park hidden actors at the centre of the given board |
| The arms flash for one frame and the reach never reads | A fast tool call goes from `running` to `output` within milliseconds, and the stage mirrors state with no minimum duration | Play beats through the beat queue in `src/lib/agent-chat/anatomy-driver.ts` with a minimum on-screen duration per beat |
| Opening a long chat replays every past tool call on the stage | The driver animated the history that `messagesFromHistory()` in `src/lib/agent-chat/pi-events.ts` rebuilds on a fresh attach | Settle the stage to the last beat of the rebuilt history, and animate only records that arrive after the attach |
| The stage keeps animating an old beat after the run moved on | A beat's `sleep` or `tween` was not cancelled when the driver jumped to the newest beat | Call `cancelAll()` from `src/lib/onboarding/stage/core.ts` before playing the newest beat, as a book page turn does |
| Stage drawings lose their styles in the chat panel | The stage SVG is built imperatively, and its rules in `stage/stage.css` are scoped under `.story-stage` | Give the live stage's wrapper the `story-stage` class and import `stage.css` in `LiveStage.svelte` |
| Role outlines are unreadable in dark mode | The stage's `--s-*` tokens in `stage/stage.css` are light-only by design (the board is a whiteboard) | Define the Brain and Arms tokens in `src/app.css` for both themes, and use those tokens for chat outlines and badges |
| A tool box shows only the Brain band and never the Arms band | The part never received `tool_execution_start`, for example because the run was aborted before execution | Treat a tool part left in state `input` after `agent_settled` as "asked but not run" in `ToolPart.svelte`, not as a running call |
