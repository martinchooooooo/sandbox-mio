---
type: improvements
title: "Proxy Toggle Management: Deferred Improvements"
feature: proxy-toggle-management
---
# Proxy Toggle Management: Deferred Improvements

Ideas noticed while scoping `01_ideas.md`/`02_dev.md` that are worthwhile but out of scope for this
feature's initial build.

## Automatic credential bundle re-injection after create or recreate

`02_dev.md` Resolved Decision 4 leaves credential re-injection exactly as undecided as it already is
for a fresh `create`: `credentials::inject` (`src-tauri/src/credentials/mod.rs`) is a manual, per-service
action today, and nothing tracks which services were ever injected into a given sandbox, so there is no
list to replay automatically after a recreate. Solving this well needs a per-sandbox record of "these
service bundles were injected here," which is a new piece of state this feature's own scope doesn't
otherwise need. Revisit once a second feature (not just recreate) wants the same "remember what was
injected" list — `docs/features/network-proxy/04_improvements.md`'s "Secret values sourced from the
`credentials/` bundle store" entry is the sibling gap this would also help close.

## Zero-downtime recreate via a throwaway-named container and a rename step

`01_ideas.md`'s Technology Choices rejected building the new container under a throwaway name and
renaming it into place, on the grounds that Podman has no atomic rename that also relocates a
bind-mounted workspace's identity, and the SSH identity/alias keying on sandbox name would need its
own relocation logic. If a future user report shows the always-a-moment-of-downtime recreate is a real
UX complaint, revisit with a concrete design for relocating `ensure_sandbox_identity`'s per-name
identity directory (`<app_data_root>/ssh-keys/<name>/`) and the SSH alias block
(`src-tauri/src/ssh/mod.rs`) atomically alongside the container rename.

## `MIO_*` environment override for `resolve_default_proxy`

`02_dev.md` Resolved Decision 1 chose not to add an environment override analogous to
`MIO_MACHINE_PROVIDER` for the new `default_proxy` setting, reasoning that every e2e spec can pass
`proxy` explicitly to `create_sandbox` instead. If `03_e2e.md` for this feature (or a later one) finds
a scenario that genuinely needs to flip the ambient default without mutating `settings.json` — e.g., a
spec asserting the create dialog's own default-seeding behavior end to end — add `MIO_PROXY_DEFAULT`
to `settings::resolve_default_proxy` (`src-tauri/src/settings/mod.rs`), mirroring
`resolve_default_provider` exactly, and document it in the root `AGENTS.md` environment table.

## Microsandbox provider support for both the default setting and recreate

Both pieces of this feature are Podman-only, following `docs/features/network-proxy/02_dev.md`
Resolved Decision 9's existing refusal of every proxy-related operation for a microsandbox-provider
sandbox. `AppSettings.default_proxy` itself is provider-neutral (nothing stops a user from setting it
while their default provider is microsandbox; the create dialog's picker just forces the per-sandbox
switch back off, per existing behavior), but `retoggle_proxy` cannot do anything for a microsandbox
sandbox at all. Revisit once microsandbox has a confirmed network-attachment story of its own
(`docs/features/microsandbox-provider/01_ideas.md`) and network-proxy grows microsandbox support.
`docs/features/microsandbox-network-policy/01_ideas.md` now proposes that support, and depends on
this feature's `SandboxBackend::recreate` to apply microsandbox egress-rule changes, which `msb modify`
cannot change on a running sandbox.

## Progress cancellation for a stuck recreate

`02_dev.md` Phase D wraps `retoggle_proxy` in `activity::scope` (`src-tauri/src/activity/mod.rs`) for
progress visibility (`TaskKind::SandboxRecreate`) but does not add a way to cancel a recreate stuck on
a slow step (an image pull over a bad connection, `mise install` hanging on a slow tool download). The
existing `activity::TaskRegistry` has no cancellation primitive to build this on top of today across
any of its current task kinds, so adding one is a cross-cutting change bigger than this feature.
Revisit if `TaskRegistry` grows cancellation for another feature's task first.
