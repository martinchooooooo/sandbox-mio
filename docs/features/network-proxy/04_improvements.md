---
type: improvements
title: "Network Proxy: Deferred Improvements"
feature: network-proxy
---
# Network Proxy: Deferred Improvements

Ideas scoped out of `01_ideas.md` as worthwhile but out of scope for the initial inspection-plus-secret-injection feature.

## Domain/host allow-deny policy enforcement

`01_ideas.md`'s Validation section notes microsandbox's own network model is deny-by-default: only public internet and DNS pass; RFC 1918 ranges, loopback, link-local, and the cloud metadata endpoint (`169.254.169.254`) are blocked, with DNS-rebinding protection (rewriting private-IP answers to `NXDOMAIN`) and DNS pinning (a domain rule only matches an IP the sandbox's own DNS query actually returned). This feature's mitmproxy addon is a natural place to add a second capability — reject requests to hosts not on an allowlist, or block the metadata endpoint outright — once the base inspection/injection path is proven. Deferred because it is a distinct, substantial piece of security engineering (rule authoring UI, default policy design, failure-mode decisions for a blocked request) layered on top of infrastructure `01_ideas.md` has not yet built.

## Traffic viewer UI

A per-sandbox panel showing the proxy's captured flow log (method, host, path, status, timing), sourced from mitmdump's flow export or a `mitmweb`-style live view. Deferred until the container lifecycle and reachability design in `01_ideas.md` is settled — the UI is a straightforward consumer of whatever the proxy already logs, not a design driver.

## Secret values sourced from the `credentials/` bundle store

`02_dev.md`'s Resolved Decision 5 has v1 store injectable secret values in
`<app_data_root>/proxy/secrets.json`, entered by the user, rather than reading them out of the
existing host-side credential store (`src-tauri/src/credentials/mod.rs`). Deferred because those
bundles are whole-`$HOME` tar archives captured by `credentials::capture`, and pulling one bearer
token out of one is agent-specific work with no general shape — the extraction rule differs per
agent CLI and would need its own per-service descriptor. Revisit once a second consumer needs the
same extraction.

## App-global default for the proxy toggle

`02_dev.md`'s Resolved Decision 3 makes traffic inspection an explicit per-sandbox opt-in in the
create dialog, with no persisted default. An `AppSettings` field (`src-tauri/src/settings/types.rs`)
defaulting every new sandbox to proxied would suit a user who wants inspection everywhere. Deferred
to keep v1's surface to one toggle; the field is additive whenever it is wanted.

## Remote-deployed sandboxes keep their proxy

`02_dev.md`'s Resolved Decision 6 refuses `deploy` for a proxy-enabled sandbox with
`BackendError::ProxyDeployUnsupported` rather than deploying it unproxied. Supporting it means
creating the network and the `<sandbox>-proxy` container on the target Podman connection during
`CliBackend::deploy` (`src-tauri/src/backends/mod.rs`), re-injecting a target-side CA, and deciding
what happens to the local proxy meanwhile. Deferred because rootless bridge-network behaviour on a
non-WSL remote Podman host is a separate unconfirmed assumption from the local WSL case the spike
covered.

## Switching an existing sandbox between proxied and unproxied

Proxy environment variables and the network attachment are baked in by `podman create`, so
`02_dev.md` accepts that toggling inspection means recreating the sandbox. A future iteration could
recreate the container in place around the preserved workspace, which is a bind mount and survives
`podman rm`. Deferred: container recreation touches the SSH identity, alias, and pane lifecycle, and
is a larger change than the toggle it would serve.

## Microsandbox provider support

`01_ideas.md` scopes to Podman only. Microsandbox has no confirmed `host.containers.internal`-equivalent (`docs/features/microsandbox-provider/01_ideas.md` does not address container-to-container reachability at all), and its own networking model is still described as beta/Windows-preview-quality in that document's Validation section. Revisit once that provider's networking story is further along.

Picked up by `docs/features/microsandbox-network-policy/01_ideas.md`: microsandbox 0.7.12 enforces egress and host-held secrets in its own host-side network stack, and its `--proxy socks5://…` option chains a host-reachable mitmproxy for the traffic viewer instead of the Podman sidecar.

## Non-HTTP(S) traffic capture

`01_ideas.md` accepts that anything ignoring `http_proxy`/`https_proxy` (raw TCP clients, git-over-SSH) bypasses the proxy silently. A future iteration could surface "N connections were not inspected" in the UI, or explore a mitmproxy SOCKS5/generic-TCP mode for tools that support a SOCKS proxy setting even when they don't support an HTTP one. Deferred: it does not change the core inspection/injection design and is better scoped once real usage shows how often it matters.

## Per-request approval / interactive policy

Rather than a static allow-deny list, prompt the user the first time a sandbox contacts a new host, similar to how some desktop firewalls work. Deferred as a much larger UX surface than this feature's inspection-only v1 needs, and dependent on the allow-deny policy groundwork above existing first.
