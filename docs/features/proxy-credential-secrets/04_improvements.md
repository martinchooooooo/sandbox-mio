---
type: improvements
title: "Proxy Credential Secrets: Deferred Improvements"
feature: proxy-credential-secrets
---
# Proxy Credential Secrets: Deferred Improvements

Ideas scoped out of `docs/features/proxy-credential-secrets/01_ideas.md`, which covers only the GitHub token behind the iron-proxy sidecar.

## Agent OAuth tokens behind the sidecar

The earlier version of this feature extracted the Claude OAuth access token from its credential bundle (`/root/.claude/.credentials.json` inside `home.tar.gz`, `src-tauri/src/credentials/mod.rs`) into a proxy rule. Deferred: an OAuth agent refreshes its own token, and the refresh response carries a new token, which a request-only swap cannot keep out of the sandbox. Two routes to evaluate: a response-side swap (next section), or iron-proxy's `oauth` transform, which mints access tokens from a refresh token held by the sidecar and injects them as a Bearer header (untested).

## Response-side swap

iron-proxy 0.56.0 leaves responses untouched in its `secrets` transform. An external transform over the gRPC `TransformService` (`proto/transform/v1/transform.proto` in iron-proxy) could rewrite real values back into placeholders. Deferred: the protocol replaces a response only by returning `REJECT` with a modified response, the whole body is buffered, and the GitHub case needs no response swap.

## Credential-store values for workspace bindings

A picker in `NetPolicyEditor.svelte` (`src/routes/projects/`) could let a hand-made workspace binding take its value from a credential-store entry instead of `set_proxy_secret`. Deferred: a workspace binding is sandbox-writable, so a credential it references could be rebound to any host (`01_ideas.md`, "An app-owned GitHub binding"); a safe design needs the host list stored app-side.

## microsandbox start environment merge

`docs/features/techdebt-sweep/04_improvements.md` asked for the microsandbox `StartEnvSource` and `ToolchainSecrets` merge to land with this feature. The merge stays relevant once microsandbox sandboxes take the placeholder route; it is not needed for the iron-proxy-only scope.

## Encryption at rest

The credential store and the sidecar's materialised token file are plaintext under the app-data root. Windows DPAPI could protect both without a user password. Deferred: the same open question applies to every credential the app stores (`docs/features/shared-agent-auth/01_ideas.md`, "Encryption at rest").
