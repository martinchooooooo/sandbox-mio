---
type: improvements
title: "Agent Anatomy in the Native Chat: Improvements"
feature: chat-agent-anatomy
---
# Agent Anatomy in the Native Chat: Improvements

Work deferred out of the first build of `chat-agent-anatomy` ([01_ideas.md](01_ideas.md)).

## Guided hands-on checklist

A short list of tasks in the companion panel (`src/lib/agent-chat/AnatomyPanel.svelte`), such as "Ask something that needs no tools", "Ask the agent to create a file" and "Open the Brain's thinking", each ticked off by watching the roles the anatomy classifier (`src/lib/agent-chat/anatomy.ts`) reports. Deferred because the checklist needs its own copy, progress state and completion rules, and the panel teaches without the checklist.

## Links from the explainer into the book

An explainer entry links to the matching page of the onboarding book, for example "Meet the Brain (AKA Model)" for a `brain-think` part. Deferred because the book renders inside the welcome flow and the `/intro` route only (`src/lib/onboarding/OnboardingBook.svelte`), with no way to open it at a given page from elsewhere.

## Replaying a past part on the stage

Clicking a past tool call replays that call's beats on the live stage, so a user can study a step after the run finished. Deferred because the first build animates live records only, and replay needs the beat queue to accept historical beats without disturbing a run in progress.

## Use the `sheet` component for the narrow-width panel

Below 896 px the companion panel opens in `Dialog.Content` restyled as a left drawer (`src/lib/agent-chat/ChatView.svelte`), because `src/lib/components/ui/sheet/` does not exist in the tree although the root `AGENTS.md` lists it. Deferred because adding it needs the retro-ui-svelte registry fetched over the network; swap to `sheet` once it is added.

## Show a "+N" count for many requested tools while asking

The live stage's `asking` beat draws at most two requested tool tiles beside the Brain (`src/lib/agent-chat/anatomy-driver.ts`); only the `acting` beat shows the "+N" count. Deferred because requests beyond two are rare and the count reads best next to the hands.
