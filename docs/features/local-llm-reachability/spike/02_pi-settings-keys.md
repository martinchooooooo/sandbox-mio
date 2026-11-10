---
type: spike
title: "Spike 02: pi 1.5.0 Default Provider and Model Keys"
feature: local-llm-reachability
---
# Spike 02: pi 1.5.0 Default Provider and Model Keys

This spike confirms the `settings.json` keys that `provision_pi` writes for pi's default provider and model (`docs/features/local-llm-reachability/02_dev.md`, Phase D). It ran on 2026-11-10 against the published package `@earendil-works/pi-coding-agent@1.5.0`, read from its npm tarball without executing it. The plan asked for the check inside a sandbox with pi installed. No such sandbox was running: `rrr` was cycling through `starting`, which is the stopped-sandbox boot loop recorded in `04_improvements.md`, and no Podman sandbox was up. The package source is the same code a sandbox runs, so the key names and the file location carry over.

## Findings

| Question | Answer | Evidence |
|---|---|---|
| Key for the default provider | `defaultProvider`, a string | the package's settings reference, row "Startup AI provider"; `SettingsManager.setDefaultProvider` marks `defaultProvider` modified |
| Key for the default model | `defaultModel`, a bare model id without the provider prefix | the package's settings reference, row "Startup model ID"; the model resolver calls `getModel(defaultProvider, defaultModelId)` |
| File location | `settings.json` in the agent directory, the same directory as `models.json` | `SettingsManager` joins the resolved agent dir with `settings.json`; the agent dir honours `PI_CODING_AGENT_DIR` |
| Project-level override | `<cwd>/.pi/settings.json` overrides the global file | `SettingsManager` also reads a project path; the app writes only the global file |
| Does pi rewrite these keys itself | Yes: choosing a model in pi saves both keys to the global `settings.json` | `setDefaultModelAndProvider` writes both and calls `save()` |

## Consequences for Phase D

- `provision_pi` writes `defaultProvider: "local-llm"` and `defaultModel: "<model id>"` into `.mio/pi-agent/settings.json`, merging into any existing keys.
- Because pi saves the user's own choice into the same file, the Resolved Decision 4 rule (overwrite only when `defaultProvider` is absent or `local-llm`) preserves a cloud model the user picked in pi.
- A user who picks a different `local-llm` model inside pi has it replaced by the running model on the next re-provision, which is the intended "default model follows the server" behaviour.
