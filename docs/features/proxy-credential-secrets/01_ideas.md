---
type: ideas
title: "Proxy Credential Secrets: Sourcing Injectable Values from the Credential Store"
feature: proxy-credential-secrets
---
# Proxy Credential Secrets: Sourcing Injectable Values from the Credential Store

This document covers letting the network-proxy feature's secret-injection rules source a value
from the existing host-side credential store (`src-tauri/src/credentials/mod.rs`) instead of only
from a value the user types by hand. Nothing of this feature is implemented; the feature it extends,
`network-proxy`, is itself entirely unimplemented (`docs/features/network-proxy/02_dev.md` — every
phase is `Remaining`). `docs/features/network-proxy/02_dev.md`'s Resolved Decision 5 chose
`<app_data_root>/proxy/secrets.json`, written by a `set_proxy_secret` command, as the only source of
injectable secret values for v1, and deferred sourcing a value from `credentials::capture`'s bundles
in `docs/features/network-proxy/04_improvements.md`, reasoning that "pulling one bearer token out of
one is agent-specific work with no general shape — the extraction rule differs per agent CLI and
would need its own per-service descriptor." Relevant existing files: `src-tauri/src/credentials/mod.rs`
and `types.rs` (the bundle store this feature reads from), `src/lib/agents/catalogue.ts` and
`src/lib/agents/types.ts` (the frontend agent descriptor whose `auth` fields already name the exact
files this feature must read), `docs/features/network-proxy/01_ideas.md` and `02_dev.md` (the proxy
this feature's output feeds), and `docs/features/shared-agent-auth/01_ideas.md` (prior spike findings
about the credential files' shape and rotation behaviour).

## Current State

**Exactly one agent CLI is catalogued with credentials today.** `src/lib/agents/catalogue.ts`'s
`AGENTS` array has three entries (`claude-code`, `opencode`, `pi`); only `claude-code` carries an
`auth: ExecInteractiveAuth` block and a `credentialService: "anthropic"`. OpenCode and Pi are
"config-provisioned" against a local LLM (`provision: ProviderProvisionConfig`) and hold no
credential of any kind — there is nothing in their case for this feature to extract. The "differs per
agent CLI" language in the deferred idea is correct in principle but, in the codebase as it stands,
the concrete surface is one agent, not an open-ended set: `docs/features/agent-setup/01_ideas.md`
line 110 and `docs/features/shared-agent-auth/03_e2e.md` line 230 both confirm the exact on-disk
shape of `/root/.claude/.credentials.json`: `{ claudeAiOauth: { accessToken, refreshToken, expiresAt,
scopes, subscriptionType } }`. The bearer value a proxy rule would want to inject is
`claudeAiOauth.accessToken`.

**The bundle a service's credentials live in is a `home.tar.gz`, not loose files.**
`credentials::capture` (`src-tauri/src/credentials/mod.rs`, line 115) runs `tar -C / -czf -` inside
the source container over the credential file's containing directory (`credential_dir`, the dirname
of `credential_path`) plus, if given, `account_source_path`, and writes the resulting gzip stream
verbatim to `<app_data_root>/credentials/<service_id>/home.tar.gz` (`bundle_dir`, line 37).
`tar_relative` (line 264) strips the leading `/` before handing paths to `tar`, so archive entries are
named `root/.claude/.credentials.json`, `root/.claude/<everything else under .claude/>`, and
`root/.claude.json` — never `/root/...`. Nothing in the crate today parses this archive on the host;
`credentials::inject` (line 157) reads it back only to pipe the raw bytes into a *different*
container's `tar -C / -xzpf -` (`tar_inject`, line 288). No Rust code in the crate has ever opened a
`home.tar.gz` to look inside it.

**`json_path_get` already exists and is exactly the extraction primitive needed.**
`crate::agents::json_path_get` (`src-tauri/src/agents/mod.rs`, line 25, `pub(crate)`) resolves a
dot-separated path against a `serde_json::Value` — `credentials::capture` already uses it to pull
`expires_at_json_key` (e.g. `"claudeAiOauth.expiresAt"`) and `account_email_json_key` out of files
read from a *live* container. The same function, unmodified, resolves a value out of a JSON document
read from inside a *stored* bundle instead of a live container; only the byte source changes.

**`tar` and `flate2` are already resolved in `Cargo.lock` at the versions this feature needs.**
`src-tauri/Cargo.lock` pins `tar 0.4.46` and `flate2 1.1.9` (pulled in transitively today, not
declared in `src-tauri/Cargo.toml`'s `[dependencies]`). Promoting both to direct dependencies adds no
new entry to the dependency tree and no new download — see Validation.

**The network-proxy secret-management surface this feature plugs into does not exist yet.**
`docs/features/network-proxy/02_dev.md` Phase D (`Remaining`) plans `src-tauri/src/proxy/rules.rs`
(`ProxyRule { host, path_prefix, header, secret_id }`, `materialise` resolving each `secret_id`
against `<app_data_root>/proxy/secrets.json`) and `src-tauri/src/commands/proxy.rs`
(`set_proxy_secret`, `delete_proxy_secret`, among others — "No command ever returns a secret value").
Phase E plans a proxy panel beside `PortsManager.svelte`'s pattern under `src/routes/projects/` for
rule and secret entry. This feature's design has to fit inside that shape, not invent a parallel one,
since `network-proxy` will have landed first (see Open Questions on sequencing).

## Proposed Model

### A small extraction-method enum, not one hardcoded shape (`src-tauri/src/credentials/secret_extract.rs`, new)

The deferred idea frames "agent-specific work with no general shape" as the blocker. The concrete
shape needed for the one agent that exists today is "read a named JSON file inside the bundle, then
apply a dot-path lookup" — but a future agent (a CLI storing a raw token file instead of JSON, or a
cookie jar) would not fit that. Rather than special-case Claude Code, the descriptor's extraction step
is a small enum with room to grow:

- `JsonField { file: &'static str, json_key: &'static str }` — read `file` (a tar-relative path) out
  of the bundle, parse as JSON, resolve `json_key` with `crate::agents::json_path_get`. Covers Claude
  Code today.
- `RawFile { file: &'static str }` — read `file` verbatim, trimmed, as the secret value. No agent
  needs this yet; it is included so the next agent that stores a bare token file does not need a third
  enum variant invented under time pressure.

This is deliberately not a fully generic plugin system (no shelling out to an agent-supplied script,
no regex extraction) — both would be over-engineering for one real consumer, and the "no general
shape" framing in the deferred idea is answered by naming the two shapes that are actually plausible,
not by building an extensibility mechanism for shapes nobody has asked for.

### The per-service descriptor table (`CREDENTIAL_SECRET_DESCRIPTORS` in `secret_extract.rs`)

A `CredentialSecretDescriptor { service_id: &'static str, label: &'static str, method:
ExtractionMethod }` static table, one entry per extractable secret. Concretely, for every agent
`credentials::capture` currently has a real caller for:

| Agent (catalogue `id`) | `service_id` | File in bundle | Extraction method | JSON key | What it yields |
|---|---|---|---|---|---|
| `claude-code` | `anthropic` | `root/.claude/.credentials.json` | `JsonField` | `claudeAiOauth.accessToken` | Anthropic API bearer token |

`opencode` and `pi` have no `credentialService` (Current State) and so have no row — there is nothing
stored for them to extract. The table lives in Rust, not in `src/lib/agents/catalogue.ts`: the
frontend catalogue's `credentialFile`/`expiresAtJsonKey` describe *live-container* paths used for
auth-status checks and injection, a different concern from *bundle-file* extraction for the proxy,
and coupling the two would mean every catalogue edit risks an unrelated proxy behaviour change. The
descriptor's `file` value is deliberately re-derived by hand from the catalogue's `credentialFile`
(`/root/.claude/.credentials.json` → `root/.claude/.credentials.json`, dropping the leading `/` per
`tar_relative`) rather than computed at runtime, because the two are already forced to agree by
`credentials::capture`'s own `credential_dir`/`tar_relative` logic and a second computed derivation
would just be an indirect way of writing the same literal.

### Streaming extraction with no disk unpack (`extract_secret` in `secret_extract.rs`)

`extract_secret(app: &AppHandle, service_id: &str) -> Result<String, CredentialError>` looks up the
descriptor for `service_id`, opens `credentials::bundle_dir(app, service_id)?.join("home.tar.gz")`
with `std::fs::File::open`, wraps it in `flate2::read::GzDecoder`, wraps that in
`tar::Archive::entries()`, and iterates looking for the entry whose `path()` equals the descriptor's
`file`. It reads only the matched entry's bytes into memory (`Read::read_to_string`); entries that do
not match are never read by application code, and the `tar` crate's own iterator advances past an
unread entry's remaining bytes internally before yielding the next header. Nothing calls
`.unpack()`/`.unpack_in()` anywhere in this path, so no loose file is ever written to disk beyond the
`home.tar.gz` that already exists — confirmed by the spike in Validation. If no entry matches, or the
matched entry does not parse as the expected shape, `extract_secret` returns `CredentialError::NotFound`
(reusing the existing "no bundle" variant's semantics: the same UI state — "not connected" — is
correct whether the bundle is entirely absent or present but missing the expected field, since both
mean "this rule has no usable value right now").

### `secrets.json` values become tagged, not bare strings (`SecretSource` in `src-tauri/src/proxy/types.rs`)

`network-proxy/02_dev.md`'s Resolved Decision 5 has `secrets.json` store a value the user typed,
implicitly `HashMap<String, String>` keyed by `secret_id`. This feature widens the value type to a
tagged union so a `secret_id` can point at either source without changing `.mio/proxy-rules.json`'s
shape (`ProxyRule.secret_id` stays an opaque string naming an entry in `secrets.json`, exactly as
already planned):

- `{ "kind": "manual", "value": "<string the user typed>" }` — today's behaviour, renamed into a
  variant.
- `{ "kind": "credential", "serviceId": "<credential store service id>" }` — resolved at materialise
  time by calling `extract_secret`, never written into `secrets.json` as a copied value.

`set_proxy_secret` (`src-tauri/src/commands/proxy.rs`, planned) gains a `source: SecretSource`
argument in place of a bare `value: String`, and a companion read-only command lists which
`service_id`s currently have a non-empty bundle (`credentials::list`, already implemented, already
filters to bundles with a loadable manifest) intersected with `CREDENTIAL_SECRET_DESCRIPTORS`, for
the picker described next.

### Live resolution at materialise time, not a copied snapshot

`ProxyRule::materialise` (`src-tauri/src/proxy/rules.rs`, planned in `network-proxy/02_dev.md` Phase
D) already re-reads `secrets.json` and writes the proxy-side `rules.json` on every `set_proxy_rules`
call and every rule change (`network-proxy/02_dev.md` Resolved Decision 11: rule changes restart the
proxy container). For a `credential`-sourced secret, `materialise` calls `extract_secret` at that same
moment rather than reading a value cached at link time. Consequence, stated as a design property, not
a limitation: a credential bundle re-captured after re-login (`credentials::capture` run again from
the credentials page) is picked up by the *next* proxy restart with no extra propagation step, and
`docs/features/shared-agent-auth/01_ideas.md`'s spike finding that Claude Code refresh tokens are
single-use (a sandbox's injected copy refreshing invalidates every other copy, including the host
snapshot) means a stale cached copy would eventually inject a dead token silently — live resolution
avoids that failure mode entirely rather than needing a staleness check for it.

### One broken credential-sourced rule does not take down every rule

`materialise` processes each `ProxyRule` independently. A rule whose `secret_id` resolves to a
`credential` source that fails extraction (`extract_secret` returns `NotFound`) is omitted from the
materialised `rules.json` rather than failing the whole `set_proxy_rules` call — a manual-value rule
for a different host on the same sandbox must keep working. `set_proxy_rules` returns a per-rule status
list (`Vec<RuleMaterialiseStatus { rule_index, ok: bool }>` or equivalent) instead of a bare `Result<()>`
so the frontend can show which rule is currently inert, rather than silently dropping it with no
signal.

### UI: a source toggle on the existing secret-entry surface, not a new page

`network-proxy/02_dev.md` Phase E plans a proxy panel with "secret entry" as part of the rule-editing
surface, but the credential-source picker belongs on *secret* definition, not on the *rule* that
references a `secret_id` by name — the same `secret_id` a rule points at might later be repointed from
manual to credential-backed without touching the rule. Concretely: the secret-entry form gains a
two-option source toggle (`switch`-adjacent or a simple radio, matching `src-tauri/AGENTS.md`'s
component inventory) — "Type a value" (today's `Input`) vs. "From a saved login," which reveals a
`select` populated from the intersection command above (only services with both a descriptor and a
non-empty bundle appear, so a user can never pick a service that would resolve to nothing). No secret
value is ever echoed back to the frontend for a credential-backed source, matching the existing
invariant that no command returns a secret value (`network-proxy/02_dev.md` Phase D); the row shows
"linked to `<service label>`" instead of a masked value.

## Technology Choices with Tradeoffs

### Extraction transport: Rust `tar`/`flate2` streaming scan (chosen) vs. shelling out to a host `tar` binary vs. extracting the whole bundle to a temp directory

Chosen: read the archive in-process with the `tar` and `flate2` crates, matching a single entry by
path and reading only that entry's bytes.

- **Shell out to a host-installed `tar`** (e.g. `tar -xzf home.tar.gz -O root/.claude/.credentials.json`)
  — rejected: the project's own convention (`src-tauri/AGENTS.md`, `process/` module) is to route
  every external-process call through `ProcessRunner` for mockability, and this call has no sandbox or
  container involved at all — it is pure host-side file parsing, a poor fit for a subsystem trait built
  around spawning `podman`/`mutagen`/`msb`. It would also depend on a host `tar` binary being on
  `PATH`, which Windows has shipped as `bsdtar` only since relatively recent builds, an assumption
  nothing else in this Windows-first crate currently makes.
- **Extract the whole bundle to a temp directory, then read the one file off disk** — rejected: it is
  strictly more disk I/O and one more cleanup obligation (a leaked temp directory on a crash) for no
  benefit over reading the one entry in-process, and it is the literal "full extraction to disk" this
  feature's brief calls out to avoid if avoidable.
- The chosen option's cost is genuinely small: `tar` and `flate2` are already resolved in
  `src-tauri/Cargo.lock` transitively (Current State), so `src-tauri/Cargo.toml` gains two direct
  `[dependencies]` lines and no new crate enters the build.

### Descriptor storage: static Rust table (chosen) vs. a JSON/TOML descriptor file under `resources/` vs. extending the frontend catalogue

Chosen: a static `&[CredentialSecretDescriptor]` in `src-tauri/src/credentials/secret_extract.rs`,
compiled into the binary.

- **A data file (JSON/TOML) shipped in `src-tauri/resources/`** — rejected: it buys user- or
  build-time editability that nothing in this feature needs (a new agent's extraction rule is a code
  change requiring a new `ExtractionMethod` review anyway, since the enum itself may need a new
  variant), while adding a parse-at-startup failure mode and a second place (beyond
  `src/lib/agents/catalogue.ts`) to keep in sync with the actual agent set.
- **Extend `AgentDescriptor`/`ExecInteractiveAuth` in `src/lib/agents/types.ts`** with a
  `secretJsonKey` field and thread it over IPC into `extract_secret` — rejected: it would make the
  frontend catalogue the source of truth for a Rust-only extraction path that never touches the
  frontend (materialise runs entirely host-side, see Proposed Model), and would require validating a
  caller-supplied `json_key`/`file` pair against arbitrary IPC input rather than a table the compiler
  already checked. The intersection command described in Proposed Model still gives the frontend
  everything it needs (which `service_id`s are pickable) without exposing the extraction internals.

### Value freshness: live extraction at materialise time (chosen) vs. copy the value into `secrets.json` when the source is linked

Chosen: `extract_secret` runs every time `materialise` runs; `secrets.json` never stores an extracted
value, only the `SecretSource::Credential { service_id }` pointer.

- **Copy the extracted value into `secrets.json` at link time, refreshing only on explicit re-link** —
  rejected on the staleness ground already laid out in Proposed Model: `shared-agent-auth/01_ideas.md`'s
  spike 2 confirmed Claude Code's refresh token is single-use, so a cached copy can go silently dead
  the moment any other copy of the same credential refreshes elsewhere, and there is no signal that
  would tell this feature to re-copy. Live extraction has no such window — the worst case is one proxy
  restart lagging one credential-page re-login by however long the user leaves rules unmodified, which
  is bounded and observable (the rule shows inert per the per-rule status above), not silent.

### Scope of the extraction-method enum: two shapes (chosen) vs. one hardcoded Claude-Code-only path vs. an open plugin mechanism

Chosen: `JsonField`/`RawFile`, sized to "one real consumer plus the next-most-plausible shape," per
Proposed Model.

- **Hardcode only Claude Code's exact JSON-field extraction with no enum at all** — rejected: it would
  directly reproduce the deferred idea's own complaint ("no general shape") the moment a second
  service (an API-key-in-a-file agent, or a config file with an org id) needs a bearer value, forcing a
  rewrite of `extract_secret`'s signature rather than an added match arm.
- **A fully general extraction mechanism (a small embedded script, arbitrary JSONPath/regex over any
  file)** — rejected as over-engineering for a feature whose entire justification (Current State) is
  one concrete agent: it would add a new execution surface (untrusted-ish extraction logic running
  over credential material) and a testing burden with no second consumer yet to prove it against.

## Validation

Spiked 2026-08-18, artefacts kept at `docs/features/proxy-credential-secrets/spike/tar_extract_spike/`
(a throwaway `cargo` crate; its `target/` build output was deleted after the run, only `Cargo.toml`,
`Cargo.lock`, and `src/main.rs` are kept). The spike built a synthetic `home.tar.gz` in memory with
three entries shaped like a real `credentials::tar_capture` output — `root/.claude/.credentials.json`
(the target, containing `claudeAiOauth.accessToken`), a 64 KB decoy entry at
`root/.claude/settings.json` that the reader must skip without reading its content, and
`root/.claude.json` — wrote it to disk, then reopened it via `flate2::read::GzDecoder` +
`tar::Archive::entries()`, located the one matching entry by path, read only its bytes, parsed it as
JSON, and resolved `claudeAiOauth.accessToken` with a copy of `json_path_get`'s logic. `cargo run`
printed `entries seen: 3` and `SPIKE OK: selective streaming read of a multi-entry tar.gz bundle works
with no disk extraction`, confirming: (1) `tar 0.4.46`/`flate2 1.1.9` correctly parse an archive shaped
like the real capture path's output, (2) matching by exact tar-relative path string (`root/.claude/...`,
no leading slash) works as `tar_relative` (`src-tauri/src/credentials/mod.rs`) produces it, (3) the
crate never required a call to `.unpack()`. The spike did not run against a real captured bundle from
a real `claude auth login` (no live login was performed) — the JSON shape it assumes is instead
corroborated by the two independent e2e-fixture citations in Current State
(`docs/features/agent-setup/03_e2e.md`, `docs/features/shared-agent-auth/03_e2e.md`), both of which
construct the identical `{"claudeAiOauth":{"accessToken":...}}` shape as ground truth for existing,
already-shipped auth-status parsing.

## Open Questions

- **Sequencing against `network-proxy`.** This feature's entire integration surface — `SecretSource`,
  `ProxyRule`/`materialise`, `set_proxy_secret`, the proxy panel — is planned but `Remaining` in
  `network-proxy/02_dev.md`. Should this feature's own `02_dev.md` phases assume `network-proxy`'s
  Phase D (rules/secrets) has already landed and slot in afterward, or should the two build plans be
  merged so `SecretSource` is designed as a tagged union from `network-proxy`'s first `secrets.json`
  write rather than a later migration of a shipped `HashMap<String, String>`? Left open because it
  depends on `network-proxy`'s own implementation order, which this document does not control.
- **Multiple extractable secrets per service.** Today's one descriptor row yields exactly one secret
  per `service_id` (`claudeAiOauth.accessToken`). If a future service's bundle held two useful values
  (e.g. an access token and a separate org-scoped key), does `CredentialSecretDescriptor` grow a list
  of named extractions per service (`service_id` + a second `descriptor_id`), or does the store's
  service-per-bundle keying (`shared-agent-auth/01_ideas.md`'s "keyed by service, not agent") mean a
  second value should be a second `service_id` bundle instead? Deferred until a second real value shows
  up, per the deferred idea's own "revisit once a second consumer needs the same extraction" framing.
- **Auto-refresh cadence beyond restart-on-change.** Live resolution (Proposed Model) means a
  materialised secret is only as fresh as the last `set_proxy_rules`/restart. If a credential is
  re-captured (re-login) while a proxy-enabled sandbox is already running with no rule edit in
  between, does anything proactively re-materialise and restart the proxy, or does the user have to
  touch a rule (even a no-op edit) to force it? Affects whether `credentials::capture`'s success path
  (`src-tauri/src/commands/credentials.rs`, `capture_agent_credentials`) needs a new side effect that
  reaches into `proxy/` — which would be the first place `credentials/` and `proxy/` couple to each
  other, a boundary `network-proxy/01_ideas.md`'s Technology Choices never needed to cross.
- **Revocation on credential deletion.** `credentials::delete` (`src-tauri/src/credentials/mod.rs`,
  line 85) removes a bundle with no awareness that a proxy `secret_id` might point at it. After
  deletion, the next `materialise` correctly omits the now-unresolvable rule (per-rule status,
  Proposed Model), but should `delete_credential_bundle` itself warn the user synchronously ("N proxy
  rules across M sandboxes reference this credential") before deleting, or is the eventual inert-rule
  signal sufficient? Deferred as a cross-feature UX question rather than a data-model one.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `extract_secret` returns `NotFound` even though the credentials page shows the service as connected | The bundle's `manifest.json` loaded fine (so `credentials::list` shows it connected) but the specific file the descriptor names is missing from `home.tar.gz`, or the JSON key it points at is absent — e.g. a bundle captured before `.credentials.json` was written | Confirm the descriptor's `file` and `json_key` in `CREDENTIAL_SECRET_DESCRIPTORS` (`src-tauri/src/credentials/secret_extract.rs`) still match the live catalogue path in `src/lib/agents/catalogue.ts`; re-capture the credential from the credentials page |
| A proxy rule silently stops injecting a header after a re-login | Live resolution (Proposed Model) means the rule is correct on the next materialise, but nothing triggers that materialise automatically — see Open Questions, "Auto-refresh cadence" | Edit or re-save the rule (or any rule on the same sandbox) to force `set_proxy_rules`/restart, until the open question is resolved with a proactive trigger |
| Two sandboxes' proxies inject the same credential-backed secret, and one starts failing with an auth error from the upstream API | Claude Code's refresh token is single-use (`docs/features/shared-agent-auth/01_ideas.md`, spike 2) — the host bundle `extract_secret` reads may have already been invalidated by a refresh that happened inside one of the sandboxes | Not specific to this feature; re-login on the credentials page to recapture a live token, same recovery path `shared-agent-auth/01_ideas.md` documents |
| Adding a new agent's descriptor row compiles but `extract_secret` always returns `NotFound` for it | The `file` path is missing the `root/` prefix or still has a leading `/` — `tar_relative` (`src-tauri/src/credentials/mod.rs`) strips the leading slash before the entry is written into the archive, so the descriptor's `file` must match the stripped form exactly | Write descriptor `file` values as `root/<rest of the container-absolute path>`, never with a leading `/`; the spike's synthetic entries (`docs/features/proxy-credential-secrets/spike/tar_extract_spike/src/main.rs`) show the expected form |
| `cargo check` fails after adding `tar`/`flate2` to `src-tauri/Cargo.toml` with a version-selection error | The versions pinned in `[dependencies]` don't match what `Cargo.lock` already resolved transitively, forcing a second, incompatible resolution | Pin the exact versions already in `src-tauri/Cargo.lock` (`tar 0.4.46`, `flate2 1.1.9` as of this writing) rather than an unconstrained `"*"` or a newer version, then run `cargo update -p tar -p flate2` only if a deliberate bump is wanted |
