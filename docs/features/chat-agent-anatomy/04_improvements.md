---
type: improvements
title: "Agent Anatomy in the Native Chat: Improvements"
feature: chat-agent-anatomy
---
# Agent Anatomy in the Native Chat: Improvements

Work deferred out of the first build of `chat-agent-anatomy` ([01_ideas.md](01_ideas.md)).

## Guided hands-on checklist

A short list of tasks, such as "Ask something that needs no tools", "Ask the agent to create a file" and "Open the Brain's thinking", each ticked off by watching the roles the anatomy classifier (`src/lib/agent-chat/anatomy.ts`) reports. Deferred because the checklist needs its own copy, progress state, completion rules and a place to live now that the companion panel is gone, and the margin avatars teach without it.

## Links from the explainer into the book

A hover card entry (`src/lib/agent-chat/RoleExplainer.svelte`) links to the matching page of the onboarding book, for example "Meet the Brain (AKA Model)" for a `brain-think` part. Deferred because the book renders inside the welcome flow and the `/intro` route only (`src/lib/onboarding/OnboardingBook.svelte`), with no way to open it at a given page from elsewhere.

## Replaying a past part's animation

Clicking a past avatar plays its part's animation again, so a user can study a step after the run finished. Deferred because history avatars are static (`src/lib/agent-chat/AgentAvatar.svelte` animates only the live part) and a replay needs a scripted sequence per part, not only the end state.

## Pin one explainer at a time

Each info icon (`src/lib/agent-chat/RoleExplainer.svelte`) pins its own hover card, so a user can pin several. Deferred because an outside click or Escape closes a pinned card and no overlap was seen to matter; a shared pinned role in the chat store would make it one at a time.
