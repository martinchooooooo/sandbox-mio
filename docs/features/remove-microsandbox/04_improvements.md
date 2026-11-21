---
type: improvements
title: "Remove microsandbox Improvements"
feature: remove-microsandbox
---
# Remove microsandbox Improvements

Work deferred out of `docs/features/remove-microsandbox/01_ideas.md`. Removing microsandbox unblocks several iron-proxy capabilities that earlier plans declined because microsandbox could not enforce them; none is part of the removal. Nothing here was verified against iron-proxy 0.59.0 (0.56.0 is pinned in `src-tauri/src/proxy/iron.rs`).

## Approximating address groups and global ports

iron-proxy cannot express per-destination ports, hostname denies, UDP or raw TCP rules, or a custom deny message. Address groups are approximable through `upstream_deny_cidrs`, and a global port restriction through nftables rules on the sidecar (not the sandbox) or by dropping the tunnel listener. Deferred because each is new enforcement code needing live verification, and Podman sandboxes never had these rules.

## Rule `methods` and `paths`

iron-proxy rules can match HTTP methods and paths. Rendering them from `NetRule` (`src-tauri/src/net_policy.rs`) in `src-tauri/src/proxy/iron_config.rs` would add per-endpoint policy that the shared schema could not carry while microsandbox was an enforcer. Deferred as a schema and editor extension of its own.

## Injected secret bindings

`docs/features/podman-egress-boundary/04_improvements.md` ("Injected bindings") was rejected only because microsandbox could swap placeholders but not inject. With one enforcer left, an inject-mode `SecretBinding` needs no guest placeholder, so bindings added after create work at once and SDK key-format checks stop mattering. Deferred pending a decision on whether legacy placeholder bindings are kept.

## `require` and `require_tls` on secrets

iron-proxy's `secrets` entries accept `require` and `require_tls`. Rendering them stops an agent from sending its own credential to a bound host and refuses plaintext use of a secret. Deferred because the shared policy format needs a new field and the editor needs a control; the earlier objection (microsandbox's `violationAction` semantics differ) no longer applies.

## Header allowlist, MCP policy and judge

iron-proxy's header allowlist, MCP tool policy and LLM judge transforms (`docs/features/podman-egress-boundary/04_improvements.md`, "LLM judge, MCP tool policy and header allowlist") are no longer constrained by a second engine. They also interact with `docs/features/mcp-servers/`. Deferred as separate features.

## Podman recreate for proxy toggling

Podman's `recreate_instance` already works; lifting the `CliBackend::recreate` refusal and building the bounded sandbox belongs to `docs/features/proxy-toggle-management/`. Recorded here so the removal does not appear to drop the capability.

## Return `build dev` to `tauri dev`

With microsandbox gone, the Job Object breakaway that forced `build dev` to launch the app directly is no longer needed (`02_dev.md`, Resolved Decision 6). `run_dev_directly` in `build.rs` still carries the locked-file move-aside and retry, stdin redirected to the null device and the opt-in `devtools` feature, and `src-tauri/src/process/mod.rs` records `tokio::process` hanging on `podman machine list` under the `cargo tauri dev` Job Object. Deferred until a hands-on Podman-only dev session under `tauri dev` shows those are covered; the gain is Rust auto-rebuild.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| An improvement here is built against a stale iron-proxy | The pinned image in `src-tauri/src/proxy/iron.rs` is 0.56.0 while newer releases exist | Verify each transform against the pinned version, or bump the pin through `MIO_IRON_PROXY_IMAGE` first |
