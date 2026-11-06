---
type: improvements
title: "Proxy Traffic Viewer: Deferred Improvements"
feature: proxy-traffic-viewer
---
# Proxy Traffic Viewer: Deferred Improvements

Ideas noticed while scoping `01_ideas.md` and `02_dev.md` that are worthwhile but out of scope for
this feature's v1.

## Independent opt-in for flow capture

`02_dev.md`'s Resolved Decision 4 makes flow capture unconditional for every proxy-enabled sandbox,
with no separate toggle beyond the base proxy toggle (`docs/features/network-proxy/02_dev.md`,
Resolved Decision 3). A user who wants traffic proxied (for secret injection) but does not want a
metadata log of every host and path their agent contacted written to disk has no way to get that
combination in v1. A second toggle in `ProxyPanel.svelte` (or a new section of it) would let flow
capture be disabled independently. Deferred to keep v1's surface to the panel itself, with no new
create-dialog or settings surface.

## Lossless resync after the addon's cap-triggered rewrite

`watch_flows` (`src-tauri/src/proxy/flowlog.rs`) tracks a byte offset, which the addon's
truncate-to-`FLOW_LOG_MAX_LINES` rewrite invalidates. The implementation recovers by re-reading the
file from zero and dropping entries whose `id` it already emitted — exact within one proxy process,
since ids are monotonic there, but a container restart resets the addon's counter, so a restart
followed by a rewrite can drop a handful of genuinely new rows. Fixing it properly needs an id the
app can order across processes (a container-start epoch prefix, or a rotation marker line the addon
writes before truncating). Deferred: the failure costs at most a few rows of a log that is already
capped and lossy by design (`02_dev.md`, Resolved Decision 1).

## Flow history beyond the in-memory/on-disk cap

`02_dev.md`'s `FLOW_LOG_MAX_LINES` cap (Resolved Decision 1) discards the oldest flows once a proxy
container has logged more than 500 since its last restart, both on disk (`flows.ndjson`) and in the
frontend's ring buffer (`src/lib/proxy/traffic.svelte.ts`). A user auditing a long-running unattended
agent session has no way to review traffic from earlier in that session once it ages out. A rotated
archive file (`flows-archive.ndjson.gz` or similar, written by the addon before truncating) or an
explicit "export visible flows" action in `TrafficPanel.svelte` would preserve history past the cap.
Deferred because it adds a second file format/rotation policy to a v1 that intentionally keeps the
addon's file handling to one file, one cap, one truncation rule.

## Blocked-vs-allowed visual distinction

`docs/features/proxy-network-policy/04_improvements.md` already records "Blocked-request visibility"
as a deferred idea on its own side, dependent on this feature's flow log existing first. This
feature's own schema (`FlowEntry` in `src-tauri/src/proxy/flowlog.rs`) captures a flow's status code
and whether it errored, which is enough raw material for a future badge distinguishing a
policy-blocked flow (a synthetic 4xx/5xx `status`, or an `error` field) from an ordinarily successful
one — but `TrafficPanel.svelte` (`02_dev.md`, Phase D) does not attempt that distinction in v1, since
`proxy-network-policy` is not implemented and Phase A's own gating spike has not yet confirmed whether
a policy-blocked flow reaches `response()`/`error()` at all (`02_dev.md`, Resolved Decision 2).
Revisit once `proxy-network-policy` ships and Phase A's findings are known.

## Full-screen or expanded flow-log view

`TrafficPanel.svelte` (`02_dev.md`, Phase D) is a compact per-sandbox panel matching
`ProxyPanel.svelte`/`PortsManager.svelte`'s existing inline-section convention, capped at 500 visible
rows and no dedicated screen space of its own. A user reviewing a busy sandbox's traffic in detail
might want a larger, dedicated view — a modal or its own route — with more comfortable filtering and
row density than a sidebar-style panel affords. Deferred because the compact panel is a straightforward
first cut matching this project's existing per-sandbox panel pattern, and a larger view is additive UI
work once the underlying data plumbing (Phases A–C) is proven.

## Opt-in raw request/response capture for deep debugging

`01_ideas.md`'s Technology Choices deliberately excludes header and body content from every flow
entry, for the secret-exposure reasons `docs/features/network-proxy/02_dev.md`'s own Secret Handling
design already established. A power user debugging a specific failing request (not routinely, and with
full awareness of the risk) might want a narrow, explicitly-triggered "capture full detail for the next
N flows to this host" mode, scoped tightly enough that a real secret is never left sitting in the
default always-on log. Deferred because it needs its own careful design (expiry, explicit user
confirmation, a visibly different code path from the always-on metadata log) rather than a variant of
this feature's default schema.
