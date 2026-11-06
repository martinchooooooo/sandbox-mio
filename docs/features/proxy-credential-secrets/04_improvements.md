---
type: improvements
title: "Proxy Credential Secrets: Deferred Improvements"
feature: proxy-credential-secrets
---
# Proxy Credential Secrets: Deferred Improvements

Ideas scoped out of `01_ideas.md`/`02_dev.md` as worthwhile but out of scope for the initial
credential-sourced-secret feature.

## Confirmation dialog on credential deletion

`02_dev.md` Resolved Decision 4 has `credentials::delete` (`src-tauri/src/credentials/mod.rs`) stay
unaware of which proxy rules reference the service being deleted; the user only learns a rule went
inert the next time they look at the proxy panel. A future iteration could have
`delete_credential_bundle` (`src-tauri/src/commands/credentials.rs`) accept a dry-run-style count of
affected `secret_id`s across sandboxes before deleting, surfaced as a confirmation dialog ("N proxy
rules across M sandboxes reference this credential"). Deferred because it requires
`credentials/` to read `proxy/`'s rule state to compute the count, a new cross-module read this
feature's Resolved Decision 5 deliberately kept one-directional (`proxy/` reads `credentials/`, never
the reverse).

## Proactive re-materialise on re-capture

`02_dev.md` Resolved Decision 3 leaves a re-captured credential inert until the next rule edit or
explicit restart. A future iteration could have `capture_agent_credentials`
(`src-tauri/src/commands/credentials.rs`) enumerate proxy-enabled sandboxes with a rule sourcing the
just-captured `service_id` and trigger `restart_proxy` for each. Deferred because it is the same
`credentials/` → `proxy/` reverse-read the confirmation-dialog idea above needs, and because the
restart cost (one `podman restart` per affected sandbox, `network-proxy/02_dev.md` Resolved Decision
11) is not free enough to trigger silently on every capture without the user asking for it.

## Multi-value descriptors per service

`02_dev.md` Resolved Decision 2 keeps `CredentialSecretDescriptor` at one extractable value per
`service_id`. If a future agent's bundle held more than one useful value (an access token plus a
separate org-scoped key, say), the descriptor table would need a `descriptor_id` alongside
`service_id` and the picker command/UI would need a second selection level. Deferred until a real
second value exists, matching the reasoning `network-proxy/04_improvements.md`'s original deferred
entry already used ("revisit once a second consumer needs the same extraction").

## `RawFile` extraction with no real consumer yet

`01_ideas.md`'s `ExtractionMethod` enum ships a `RawFile { file }` variant alongside `JsonField` even
though every credential-store bundle that exists today (`claude-code`/`anthropic`) is JSON-shaped. It
was included so the next agent whose credential is a bare token file does not force a signature
change under time pressure, but nothing exercises it and Phase A's unit tests (`02_dev.md`) can only
assert its parsing logic against a synthetic fixture, not a real bundle. Worth revisiting — either
adding a real second `service_id` that uses it, or removing the variant if no agent ever needs it —
once a concrete second agent is catalogued with `credentialService` in `src/lib/agents/catalogue.ts`.

## Encryption at rest for `secrets.json` and extracted values

Neither this feature nor `network-proxy/02_dev.md` encrypts `<app_data_root>/proxy/secrets.json` or
the credential bundles `extract_secret` reads from at rest; both are plaintext, matching
`docs/features/shared-agent-auth/01_ideas.md`'s own accepted-for-v1 plaintext stance and its still-open
"Encryption at rest" question (Windows DPAPI, no user password needed). A future pass could wrap both
stores under the same encryption mechanism at once, since a `Credential`-sourced `secrets.json` entry
means the proxy's materialised `rules.json` now transitively carries the same token the credential
store protects (or fails to protect) — the two stores' at-rest posture should move together rather
than being decided independently later.
