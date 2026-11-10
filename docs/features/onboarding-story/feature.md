---
type: feature
title: "onboarding-story"
description: "Illustrated onboarding book inside the welcome flow, extensible by chapter, with first-run setup running in the background and reported through the activity center."
status: in-progress
depends_on: [welcome-onboarding, activity-center]
architecture: [docs/architecture/workspace-ui.md]
---

# onboarding-story

Illustrated onboarding book inside the welcome flow, extensible by chapter, with first-run setup running in the background and reported through the activity center.

## Status note

Phases A to E are implemented. Phase F (the translation pass) waits for the English copy to be approved; until then every onboarding string except `onboarding_story_title` is a hard-coded literal under a `TODO(i18n)` marker.
