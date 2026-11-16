---
type: feature
title: "proxy-credential-secrets"
description: "Source proxy secret-injection values from the credential store instead of hand-entered values."
status: planned
architecture: [docs/architecture/network.md, docs/architecture/agents.md]
depends_on: [podman-egress-boundary, microsandbox-network-policy, shared-agent-auth]
---

# proxy-credential-secrets

Source proxy secret-injection values from the credential store instead of hand-entered values.

## Status note

The goal stands, but `02_dev.md` must be rewritten with `feature-dev` before implementation. It folds into network-proxy's mitmproxy injection rules (`rules.rs`, `materialise`, `set_proxy_rules`, a `SecretSource` inside each rule), which `podman-egress-boundary` deleted (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 20). The new seam is the secret value store: bindings in `.mio/net-policy.json` reference a secret by id, values live in `<app_data_root>/proxy/secrets.json` (written by `set_proxy_secret`), the iron-proxy sidecar reads them through `materialise_secret_files` (`src-tauri/src/proxy/iron.rs`) at every apply, and microsandbox reads them at every start. A credential-sourced secret should resolve at those two points. The 2026-08-18 tar-extraction spike still holds. `docs/features/techdebt-sweep/04_improvements.md` asks for the microsandbox `StartEnvSource` and `ToolchainSecrets` merge to land with this feature.
