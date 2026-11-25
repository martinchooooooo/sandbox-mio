---
type: feature
title: "net-policy-review"
description: "Stop a sandbox from rewriting its own egress policy and secret bindings: every sidecar apply enforces only the app-side accepted policy, and a changed workspace policy file is a proposal the user reviews as a diff before it applies."
status: implemented
depends_on: [podman-egress-boundary, proxy-credential-secrets, proxy-remote-deploy]
architecture: [docs/architecture/network.md]
---

# net-policy-review

Stop a sandbox from rewriting its own egress policy and secret bindings: every sidecar apply enforces only the app-side accepted policy, and a changed workspace policy file is a proposal the user reviews as a diff before it applies.

## Status note

Created on 2026-11-24 from task T2 of `docs/improvement-backlog.md` and finding 1 of `docs/features/podman-egress-boundary/05_dev_review.md`. `01_ideas.md` maps every apply path and the options; `02_dev.md` holds a phased plan for the recommended hybrid. The user settled all ten Resolved Decisions by taking each recommendation, and Phases A to E are implemented: every apply renders the accepted policy (`applied.json` with a revision), the workspace file is a proposal reviewed in a dialog, secret values are per sandbox with a startup migration, and create no longer accepts an existing policy file. Light spec `net-policy-review-ui` and the migrated egress-boundary specs cover it. Still deferred (see `04_improvements.md`): a policy preview in the create dialog, reserved variable names, and a file watcher.
