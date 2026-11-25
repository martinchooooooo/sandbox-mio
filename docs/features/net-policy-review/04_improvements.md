---
type: improvements
title: "Net Policy Review: Deferred Improvements"
feature: net-policy-review
---
# Net Policy Review: Deferred Improvements

Work noticed while planning [01_ideas.md](01_ideas.md) and [02_dev.md](02_dev.md) that is worthwhile but out of scope for keeping the egress policy out of the sandbox's reach.

## Policy preview in the create dialog

`02_dev.md`, Resolved Decision 5 recommends building the accepted policy from the create dialog alone and leaving an imported folder's `.mio/net-policy.json` as a proposal reviewed after create. Showing the file's rules and bindings in `src/routes/projects/CreateSandboxItem.svelte` and accepting them at create would save a step for a user importing a repository with a deliberate allowlist. Deferred because the create dialog has no policy view today and the post-create review covers the security need.

## Watching the workspace policy file

`src/routes/projects/NetPolicyEditor.svelte` reads the policy state on mount and, after `02_dev.md` Phase B, on window focus. A file watcher on `.mio/net-policy.json` (or a Mutagen sync event for a deployed sandbox) would show a new proposal while the editor is open. Deferred because no apply path reads the file any more, so a late banner is a convenience gap, not a security gap.
