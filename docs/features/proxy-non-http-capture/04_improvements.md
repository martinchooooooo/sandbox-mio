---
type: improvements
title: "Proxy Non-HTTP Capture: Deferred Improvements"
feature: proxy-non-http-capture
---
# Proxy Non-HTTP Capture: Deferred Improvements

Ideas scoped out of `01_ideas.md` as worthwhile but out of scope for the visibility-only v1.

## SOCKS5/generic-TCP capture mode

`01_ideas.md`'s Proposed Model ("Investigated and not recommended for v1") confirms mitmproxy/
mitmdump supports a SOCKS5 listen mode (`--mode socks5`) that can run concurrently with the existing
regular HTTP proxy mode on one `mitmdump` process, with no new container or base image needed
(`docs.mitmproxy.org`, fetched 2026-08-18). It was not built because no sandboxed tool in this
project's toolchain has a wired path to use a SOCKS proxy setting today: HTTP(S)-capable tools already
go through the existing `http_proxy`/`https_proxy` mechanism, and the flagship non-HTTP example the
deferred idea names — git-over-SSH — needs a `ProxyCommand` shim and a SOCKS5-capable helper binary
(`nc`/`ncat`/`connect-proxy`) not currently installed in `src-tauri/resources/Containerfile`, plus new
per-sandbox SSH/git configuration (`GIT_SSH_COMMAND` or `core.sshCommand`) this project has never
shipped. Revisit once `01_ideas.md`'s bypass-visibility count (built as v1) shows real, recurring
non-HTTP traffic from a workload that would concretely benefit — the "real usage" signal the original
deferred idea's own framing asked for before scoping this further. Building it would need: a new
package in the Containerfile, new per-sandbox SSH/git config plumbing, a second `--mode socks5@<port>`
in `create_proxy` (`src-tauri/src/proxy/mod.rs`) with its own port/reachability consideration, and a
gating spike (mitmproxy's SOCKS mode has not been confirmed against the pinned `MITMPROXY_IMAGE_DEFAULT`
tag).

## Push-event architecture for the bypass count

`01_ideas.md`'s Technology Choices chose a plain polled command (`proxy_bypass_status`) over a
dedicated Tauri event and module-scoped store, reasoning that a poll-loop-plus-event stack (matching
`network://forwards-changed` or `proxy-traffic-viewer/01_ideas.md`'s planned flow-log event) is
disproportionate machinery for a single slow-changing count. If `proxy-traffic-viewer` ships its event
pattern first, folding the bypass count into that event's payload (or a sibling event on the same
mechanism) would remove the need for `ProxyPanel.svelte`'s own `setInterval`-based refresh. Deferred
because it depends on a sibling feature's implementation choices that have not landed yet.

## Per-endpoint detail beyond the capped sample

`01_ideas.md` leaves the `sample` field (a capped list of `"host:port"` strings) as an Open Question
for whether it ships in v1's frontend phase at all. A richer version — first-seen/last-seen timestamps
per bypassed endpoint, or a running rate rather than a lifetime distinct count — was not designed here
because it adds real backend state (the current design is intentionally a single in-memory `HashSet`
plus counters with no persistence) for a level of detail the deferred idea's own "lightweight signal"
framing does not ask for. Revisit if the plain count/sample proves too coarse in practice.
