---
type: improvements
title: "Proxy Network Policy: Deferred Improvements"
feature: proxy-network-policy
---
# Proxy Network Policy: Deferred Improvements

Ideas noticed while scoping `01_ideas.md` and `02_dev.md` that are worthwhile but out of scope for
this feature's v1. `docs/features/network-proxy/04_improvements.md` is a separate document and is not
edited here.

## Unconditional metadata/private-range block

`02_dev.md`'s Resolved Decision 2 keeps the baseline RFC 1918/loopback/link-local block tied to
policy mode `!= "off"`, so a proxy-enabled sandbox that never turns policy on keeps
`network-proxy`'s unrestricted v1 behavior. A narrower, higher-value variant — block this fixed,
non-authored range unconditionally for every proxy-enabled sandbox, independent of whether the user
ever opens the policy UI at all — was left as an open question in `01_ideas.md` rather than decided.
Revisit if real usage shows the opt-in gate is routinely left off and a metadata-endpoint-reachable
sandbox turns out to be a recurring source of concern.

## Configurable interactive-approval timeout and a fail-open mode

`02_dev.md`'s Resolved Decision 3 fixes the interactive-approval timeout at 120 seconds, fail-closed
(`deny`) always. A per-sandbox configurable timeout, or a fail-open mode for a sandbox a user
explicitly marks as "trusted enough to default-allow while I'm away," would help the unattended-agent
case this feature's Open Questions flagged as a genuine product tension. Deferred because it adds a
second axis of configuration (timeout value, and which failure direction) to a v1 that already ships
three policy modes and a fixed baseline block — more surface than the first cut needs.

## Wildcard, subdomain, and CIDR rule matching

`02_dev.md`'s Resolved Decision 5 matches `.mio/proxy-policy.json` rules on an exact host string only,
the same convention `mitm_addon.py`'s existing secret-injection rules use. A rule like `*.example.com`
or a CIDR range for an IP-literal target would remove the need to author one rule per subdomain a
service actually uses. Deferred to keep the addon's matching logic identical in shape to the
already-shipped `SecretInjector`'s, and because it is additive to the exact-match schema, not a
breaking change to it.

## Persisted "always block" action from the approval dialog

`02_dev.md`'s Resolved Decision 7 only ever persists an `"allow"` decision from the interactive
approval flow as a permanent rule; a `"deny"` click answers the one blocked flow without writing
anything to `.mio/proxy-policy.json`, so the same host prompts again later. A second dialog action —
"always block this host" — would let a user close the loop on a host they know they never want
reachable, the same way "allow" already closes it for one they know they do want reachable. Deferred
because it is a small additive UI change once Phase F's dialog exists, not a design blocker for v1.

## Policy mode offered at sandbox-creation time

`02_dev.md`'s Resolved Decision 6 has every sandbox start at policy mode `"off"`, set afterward
through `ProxyPanel.svelte` with no sandbox recreation required. Offering a policy-mode choice
alongside the existing proxy toggle in the create-sandbox dialog would save a round trip for a user
who already knows they want static or interactive policy from the start. Deferred to keep the create
dialog's surface unchanged in v1 — this feature intentionally reuses the existing dialog with policy
configuration living entirely in the panel instead.

## Blocked-request visibility

Neither the static nor the interactive policy mode surfaces a count or log of requests the baseline
block or a `"deny"` rule actually stopped — a user has no in-app way to tell whether a policy is doing
anything short of noticing a tool failure. `docs/features/network-proxy/04_improvements.md`'s "Traffic
viewer UI" idea (a per-sandbox flow-log panel) would be a natural place to add a blocked/allowed
indicator per logged flow, once that viewer exists. Deferred here for the same reason it was deferred
there: it is a straightforward consumer of data the proxy already has, not a design driver for this
feature.
