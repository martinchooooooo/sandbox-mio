---
type: feature
title: "github-cli-auth"
description: "Log in to GitHub once through the GitHub CLI in the auth container, then apply that token to every sandbox's gh and git, at create and to existing sandboxes, with a per-sandbox opt-out."
status: in-progress
depends_on: [shared-agent-auth, credentials-inline-auth, techdebt-sweep]
architecture: [docs/architecture/agents.md, docs/architecture/providers.md, docs/architecture/network.md, docs/architecture/workspace-ui.md]
---

# github-cli-auth

Log in to GitHub once through the GitHub CLI in the auth container, then apply that token to every sandbox's gh and git, at create and to existing sandboxes, with a per-sandbox opt-out.

## Status note

All phases (A to E) are implemented in `02_dev.md`. Awaiting user approval of the behaviour, then the i18n pass (`TODO(i18n)` markers) and `03_e2e.md`. Open manual checks: the classic-PAT scope probe (Phase B) and a `standard` image build (Phase A).
