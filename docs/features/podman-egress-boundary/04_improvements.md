---
type: improvements
title: "Podman Egress Boundary: Deferred Improvements"
feature: podman-egress-boundary
---
# Podman Egress Boundary: Deferred Improvements

Ideas scoped out of `docs/features/podman-egress-boundary/01_ideas.md` as worthwhile but outside the first build of the internal-network boundary and the iron-proxy sidecar. Nothing here was verified; each item rests on iron-proxy's README or a third-party README read on 2026-11-13.

## microsandbox chained to iron-proxy instead of mitmproxy

Point microsandbox's `--proxy socks5://127.0.0.1:<hostport>` at an iron-proxy sidecar VM instead of the mitmproxy sidecar (`docs/features/microsandbox-network-policy/01_ideas.md`, Chained mitmproxy for Visibility), so both providers share one proxy image and one audit format. Deferred because iron-proxy's SOCKS5 tunnel supports no authentication, while a host-loopback listener needs authentication to keep other local processes out. An HTTP CONNECT chain (`--proxy http://…` with `--http-connect-username`) might avoid the gap if iron-proxy's tunnel listener accepts proxy authentication, which is unverified. microsandbox also already enforces egress and secrets, so iron-proxy would only observe there.

## Non-HTTP visibility with an eBPF sensor

Data-only eBPF sensors could show the non-HTTP connections the boundary now blocks: Inspektor Gadget (2,970 stars, pushed 2026-11-13) and Qtap (1,464 stars, pushed 2026-11-03). Deferred because both need a privileged container and a BTF-enabled kernel in the podman machine, unverified on the WSL kernel, and a privileged container is host-global state on the machine.

## git over SSH through iron-proxy's SSH credential listener

iron-proxy documents an opt-in SSH listener that maps a sandbox's disposable identity onto a separately held upstream key (non-interactive exec, SFTP, wstunnel). It might restore git over SSH inside the boundary without the real key entering the sandbox. Deferred because the README marks it as for local development and nothing about its git compatibility was checked.

## LLM judge, MCP tool policy and header allowlist

iron-proxy's `judge` transform asks an LLM to reject requests matching rules (it can only reject, never widen the allowlist), and could point at the local inference server through its OpenAI-compatible `base_url`. Its MCP policy filters `tools/list` and rejects `tools/call` by a default-deny tool allowlist. `header_allowlist` strips unlisted request headers. Deferred because the first build is about the boundary and the existing viewer; each is an additional transform in the generated config once that ships.

## Reject-then-reload approval

An approval flow built on the audit stream: a `rejected_by` row in the traffic panel offers "allow this host", which adds an allow rule to `.mio/net-policy.json`, calls `apply_sandbox_policy`, and lets the agent retry. iron-proxy has no hook that holds a flow while a human decides, so the first request always fails. Deferred by user decision on 2026-11-13 (`02_dev.md`, Resolved Decision 2); the hold-the-flow design of the superseded `proxy-network-policy` (Phases D–F) is not the starting point. The reserved `interactive` field of `NetPolicy` (`src-tauri/src/net_policy.rs`) is where an opt-in would live.

## Secret placeholders for bindings added after create

Placeholders reach a Podman sandbox only through `--env` at `podman create` (`02_dev.md`, Resolved Decision 7). A guest file such as `/etc/mio/secret-env.sh`, written through `podman exec` at every apply and sourced from `/root/.bashrc` in `src-tauri/resources/Containerfile`, would make a new binding visible to new shells without a recreate. Deferred because agents launched by non-interactive `podman exec` would not source the file, so the behaviour would differ between panes and agents, and the image change needs a `BASE_IMAGE_VERSION` bump.

## Placeholder prefixes

Some client SDKs validate API-key format (for example a fixed `sk-` prefix) and reject the generic `mio-placeholder-<var>` placeholder before sending a request (`02_dev.md`, Resolved Decision 16). An optional per-binding `placeholderPrefix` in `SecretBinding` (`src-tauri/src/net_policy.rs`) would let a binding produce a format-valid placeholder. Deferred until a concrete agent hits the problem.

## Injected bindings

iron-proxy 0.56.0's `secrets` transform has an `inject` mode, verified live in Phase A (`spike/README.md`, check 6b): the sidecar sets a header or query parameter on requests to the bound hosts and paths, with an optional Go-template `formatter`, without the agent sending anything. An `inject` form of `SecretBinding` (`src-tauri/src/net_policy.rs`) for the sidecar dialect would need no guest placeholder, so a binding added after create would work at once (`02_dev.md`, Resolved Decision 7) and SDK key-format checks would not matter (Resolved Decision 16). Legacy `.mio/proxy-rules.json` rules would convert onto it 1:1. Not chosen by user decision on 2026-11-14 (`02_dev.md`, Resolved Decision 8): microsandbox's `--secret` can only swap a placeholder, so an injected binding would be Podman-only and `SecretBinding` would stop being one format with two enforcers.

## Require the placeholder on bound hosts

iron-proxy's `secrets` entries accept `require: true`, rejecting a request to a bound host that carries no placeholder. Rendering it would stop an agent from sending its own credential to a host the user bound a managed secret for. Deferred because the semantics differ from microsandbox's `violationAction`, and the shared policy format would need a new field.

## Request body capture

iron-proxy's `body_capture` transform records request bodies (for example prompts sent to a model API) into the audit record. Deferred, and off by default if ever added: captured bodies are plain text in the audit stream, and the traffic viewer's contract is metadata only (`docs/features/proxy-traffic-viewer/01_ideas.md`).
