---
type: feature
title: "proxy-credential-secrets"
description: "Keep the GitHub token on the host: sandboxes hold a fixed placeholder that the iron-proxy sidecar swaps for the real token on GitHub requests."
status: implemented
architecture: [docs/architecture/network.md, docs/architecture/agents.md]
depends_on: [podman-egress-boundary, github-cli-auth]
---

# proxy-credential-secrets

Keep the GitHub token on the host: sandboxes hold a fixed placeholder that the iron-proxy sidecar swaps for the real token on GitHub requests.

## Status note

Rewritten from scratch on 2026-11-18. The earlier plan (extracting Claude OAuth tokens from credential bundles into mitmproxy injection rules) was dropped: its seams were deleted by `podman-egress-boundary`, and its goal moved to `04_improvements.md`. The placeholder-swap spike passed (`docs/features/github-cli-auth/spike/proxy-swap.md`). All four phases of `02_dev.md` are implemented: the GitHub binding in the sidecar (swapping on `github.com`, `api.github.com`, `uploads.github.com`, `gist.github.com` and `raw.githubusercontent.com`), placeholder login for gh, the placeholder for mise, and the Manage card mode with Paraglide strings.

The e2e plan is written in `03_e2e.md` (a standard spec for the real sidecar swap, a light spec for the plaintext card); both specs exist: the light spec passes and the standard spec skips without `MIO_E2E_GITHUB_TOKEN`. On 2026-11-18 the user manually verified all standard-spec scenarios and they passed; the automated standard run is still pending the token.
