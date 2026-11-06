---
type: dev-plan
title: "Proxy Credential Secrets: Build Plan"
feature: proxy-credential-secrets
---
# Proxy Credential Secrets: Build Plan

This document is the build plan for sourcing network-proxy secret-injection values from the
host-side credential store, scoped in `docs/features/proxy-credential-secrets/01_ideas.md`, and
supersedes it wherever the two disagree. Nothing is implemented yet — every phase below is
`Remaining`. This feature has a hard dependency on `docs/features/network-proxy/02_dev.md`'s own
Phase D (`src-tauri/src/proxy/rules.rs`, `src-tauri/src/commands/proxy.rs`), which is itself
`Remaining` there; Resolved Decision 1 below settles how the two build plans interlock. The
2026-08-18 spike at `docs/features/proxy-credential-secrets/spike/tar_extract_spike/` proved the
extraction mechanism (Phase A) works against a bundle shaped like a real `credentials::capture`
output and needs no repeat before Phase A starts.

## The Concrete Model

A new descriptor table in `src-tauri/src/credentials/secret_extract.rs` names, per credential-store
`service_id`, which file inside that service's `home.tar.gz` holds an injectable secret and how to
read it out. `src-tauri/src/proxy/types.rs` (planned by `network-proxy/02_dev.md` Phase D) gains a
`SecretSource` enum so a `secrets.json` entry is either a value the user typed or a pointer at one of
these descriptors. `src-tauri/src/proxy/rules.rs`'s `materialise` (also Phase D) resolves a
`credential`-sourced entry by calling `secret_extract::extract_secret` at materialise time — every
`set_proxy_rules` call and every rule-triggered restart — rather than caching a copy, so a re-captured
credential is picked up the next time rules are (re)materialised with no separate propagation step.
The frontend's secret-entry surface (`network-proxy/02_dev.md` Phase E) gains a two-way source toggle
backed by a new read-only command listing which services currently have both a descriptor and a
non-empty bundle.

## Resolved Decisions

1. **Sequencing against `network-proxy`** (`01_ideas.md` Open Question 1) — folded in, not
   sequenced after. `network-proxy/02_dev.md` Phase D has not been implemented yet, so
   `SecretSource` ships as the tagged union from Phase D's very first `secrets.json` write; Phase D's
   own file list (`src-tauri/src/proxy/rules.rs`, `src-tauri/src/commands/proxy.rs`) must be built
   using this document's Phase B shape rather than a bare `HashMap<String, String>` that gets migrated
   later. Whichever feature's `feature-implement` run reaches Phase D first implements Phase B/C below
   as part of that same phase; this document's phases stay separate sections only so this feature's
   own scope (the extraction primitive, the descriptor table, the sourcing UI) is separately
   trackable, not because they can land as an independent code change against an already-shipped
   Phase D.
2. **Multiple extractable secrets per service** (`01_ideas.md` Open Question 2) — one descriptor per
   `service_id` in v1; `CredentialSecretDescriptor` carries no separate `descriptor_id`. A second
   value for the same service (hypothetical, no real case exists yet) would add a `descriptor_id`
   field and change `CREDENTIAL_SECRET_DESCRIPTORS`'s lookup from "by `service_id`" to "by
   `(service_id, descriptor_id)`" — a additive signature change deferred until a real second value
   exists, per `01_ideas.md`'s own "revisit once a second consumer needs the same extraction" framing
   carried over from `network-proxy/04_improvements.md`.
3. **Auto-refresh cadence beyond restart-on-change** (`01_ideas.md` Open Question 3) — no proactive
   push in v1. `credentials::capture`'s success path (`src-tauri/src/commands/credentials.rs`,
   `capture_agent_credentials`) gains no new side effect into `proxy/`; a re-captured credential takes
   effect on the next rule edit or explicit restart, matching `network-proxy/02_dev.md` Resolved
   Decision 11's existing "addon reads `rules.json` once at load, restart on rule change" model
   exactly. Documented as a gotcha (below) rather than built around.
4. **Revocation on credential deletion** (`01_ideas.md` Open Question 4) — no synchronous warning in
   v1. `credentials::delete` (`src-tauri/src/credentials/mod.rs`) is unchanged by this feature; a rule
   pointing at a deleted bundle goes inert on the next materialise via the per-rule status mechanism
   (Phase B), which is the same signal a missing/never-captured bundle already produces. A
   confirmation dialog counting affected rules is recorded as a deferred improvement (see
   `04_improvements.md`).
5. **Cross-module call from `proxy/` into `credentials/`** (new; clarifies a question `01_ideas.md`
   did not pose directly) — `proxy::rules::materialise` calls
   `credentials::secret_extract::extract_secret` directly, a plain function call, not a
   mockable-trait dependency. `src-tauri/AGENTS.md`'s "subsystem modules never depend on each other"
   rule is scoped to the trait-injected, external-process subsystems in that document's table
   (`SandboxBackend`, `RuntimeManager`, `SyncEngine`, `MachineManager`, `NetworkManager`); `extract_secret`
   touches only `std::fs` against a path derived from `AppHandle`, exactly like `mio_files.rs`'s
   functions, which every subsystem already calls freely. No new trait, no new mock, no e2e `light`
   tier gap is introduced by this call.

## Remaining: Phase A — the extraction primitive

- `src-tauri/Cargo.toml`: add `tar = "0.4.46"` and `flate2 = "1.1.9"` to `[dependencies]` — both
  already resolved at these versions in `src-tauri/Cargo.lock` transitively (`01_ideas.md`
  Validation), so this adds no new entry to the dependency tree. Add a `[dev-dependencies]` section
  (none exists yet) with `tempfile = "3.27.0"`, also already resolved transitively, for Phase A's unit
  tests.
- `src-tauri/src/credentials/secret_extract.rs` (new): `ExtractionMethod` enum (`JsonField { file:
  &'static str, json_key: &'static str }`, `RawFile { file: &'static str }`),
  `CredentialSecretDescriptor { service_id: &'static str, label: &'static str, method:
  ExtractionMethod }`, and the static table `CREDENTIAL_SECRET_DESCRIPTORS: &[CredentialSecretDescriptor]`
  with its one entry: `service_id: "anthropic"`, `label: "Claude Code"`, `method: JsonField { file:
  "root/.claude/.credentials.json", json_key: "claudeAiOauth.accessToken" }`.
- Same file: `pub fn extract_secret(app: &AppHandle, service_id: &str) ->
  Result<String, CredentialError>` — looks up the descriptor by `service_id` (returns
  `CredentialError::NotFound(service_id.to_string())` if no descriptor exists, reusing the variant
  `credentials::load_manifest` already uses for "no bundle"), opens
  `credentials::bundle_dir(app, service_id)?.join("home.tar.gz")` with `std::fs::File::open` (mapping
  a missing file to the same `NotFound`), wraps it in `flate2::read::GzDecoder` then
  `tar::Archive::entries()`, and scans for the entry whose `path()` equals the descriptor's `file`.
  For `JsonField`, the matched entry's bytes are read with `Read::read_to_string`, parsed with
  `serde_json::from_str`, and resolved with `crate::agents::json_path_get` (widened from `pub(crate)`
  to accept the call from `credentials/`, which is already crate-internal visibility, so no
  visibility change is actually needed — both modules are in the same crate). For `RawFile`, the
  matched entry's bytes are read and `.trim()`-ed. No matching entry, or a `JsonField` value that is
  not a JSON string, returns `CredentialError::NotFound(service_id.to_string())` — deliberately the
  same variant as "bundle absent," per `01_ideas.md`'s reasoning that both cases mean "this rule has
  no usable value right now" to a caller.
- Same file: `pub fn available_secret_sources(app: &AppHandle) -> Result<Vec<(&'static str,
  &'static str)>, CredentialError>` — intersects `CREDENTIAL_SECRET_DESCRIPTORS` against
  `credentials::list(app)?` (already implemented, already filters to bundles with a loadable
  manifest) by `service_id`, returning `(service_id, label)` pairs for services that are both
  catalogued here and actually captured. Backs the Phase C command.
- `src-tauri/src/credentials/mod.rs`: add `pub mod secret_extract;` alongside the existing `pub mod
  types;`.
- Unit tests in `secret_extract.rs` (`#[cfg(test)] mod tests`, matching the style in
  `src-tauri/src/workspace/mod.rs`, the crate's only prior unit-test module): build a synthetic
  `home.tar.gz` with `tar::Builder` + `flate2::write::GzEncoder` into a `tempfile::NamedTempFile`
  (the same construction the spike used, minus the on-disk `Cargo.toml` scaffolding), point
  `extract_secret` at it via a `service_id`/bundle-dir override, and assert: the target entry's value
  round-trips correctly, a decoy entry of different content is never mistaken for the target, and a
  bundle missing the target file returns `CredentialError::NotFound`. `extract_secret` itself takes
  `app: &AppHandle` and derives the bundle path via `credentials::bundle_dir`, so the test needs
  `MIO_CREDENTIALS_DIR` (`src-tauri/src/env_ext.rs`) pointed at a temp directory the same way other
  credential-store tests would, per the existing override contract in `credentials::store_dir`'s doc
  comment.

## Remaining: Phase B — `SecretSource` and materialise integration (lands inside `network-proxy` Phase D)

- `src-tauri/src/proxy/types.rs`: add `SecretSource` — `#[serde(tag = "kind", rename_all =
  "camelCase")] enum SecretSource { Manual { value: String }, Credential { service_id: String } }`.
  `secrets.json`'s on-disk shape (`network-proxy/02_dev.md` Resolved Decision 5) becomes
  `HashMap<String, SecretSource>` keyed by `secret_id`, read/written through the same
  `<app_data_root>/proxy/secrets.json` path already planned there.
- `src-tauri/src/proxy/rules.rs`: `ProxyRule::materialise` (or its free-function equivalent) resolves
  each rule's `secret_id` against the `secrets.json` map; a `Manual` entry's `value` is used directly,
  a `Credential` entry calls `credentials::secret_extract::extract_secret(app, &service_id)`. A rule
  whose resolution fails (missing `secret_id`, or `extract_secret` returning `NotFound`) is omitted
  from the written `rules.json`, not treated as a hard error for the whole materialise call — per
  `01_ideas.md`'s "one broken credential-sourced rule does not take down every rule."
  `materialise`'s return type becomes `Vec<RuleMaterialiseStatus { rule_index: usize, ok: bool }>` (or
  an equivalent per-rule outcome list) instead of a bare `Result<(), ProxyError>`, so
  `set_proxy_rules` (`src-tauri/src/commands/proxy.rs`) can report which rules are currently inert.
- `src-tauri/src/commands/proxy.rs`: `set_proxy_secret` takes `source: SecretSource` in place of a
  bare `value: String`; `set_proxy_rules`'s return type widens to carry the per-rule status list
  above. No command in this file returns an extracted secret value — `network-proxy/02_dev.md` Phase
  D's existing invariant ("No command ever returns a secret value") is unchanged and extends to
  `Credential`-sourced entries.

## Remaining: Phase C — the sourcing command and picker data

- `src-tauri/src/commands/credentials.rs`: new command `list_credential_secret_sources(app:
  tauri::AppHandle) -> Result<Vec<CredentialSecretSourceOption>, AppError>` wrapping
  `secret_extract::available_secret_sources`, where `CredentialSecretSourceOption { service_id:
  String, label: String }` is a small serialisable struct added to `secret_extract.rs` (or
  `credentials/types.rs`, matching where `CredentialManifest` already lives) purely for this IPC
  boundary — the internal `&'static str` pairs are not `Serialize` as tuples in a
  frontend-friendly shape.
- `src-tauri/src/commands/mod.rs`: register `credentials::list_credential_secret_sources` in
  `tauri::generate_handler![]`.

## Remaining: Phase D — frontend sourcing UI

- `src/lib/proxy/types.ts` (new, mirroring `src/lib/agents/types.ts`'s role for the agent catalogue):
  TypeScript `SecretSource` union (`{ kind: "manual"; value: string } | { kind: "credential";
  serviceId: string }`) and `CredentialSecretSourceOption { serviceId: string; label: string }`,
  matching the Rust `#[serde(rename_all = "camelCase")]` field casing.
- The proxy panel's secret-entry surface (`network-proxy/02_dev.md` Phase E, planned under
  `src/lib/workspace/` beside `PortsManager.svelte`'s pattern) gains a source toggle: "Type a value"
  (today's `Input`, wired to `SecretSource.Manual`) vs. "From a saved login" (a `select` populated by
  calling `list_credential_secret_sources`, wired to `SecretSource.Credential`). No component ever
  receives or displays an extracted value — the row shows `"linked to <label>"` for a `Credential`
  source, never a masked or partial token.
- `messages/en.json`, `messages/de.json`, `messages/ru.json`: keys for the source toggle's two
  options and the "linked to `<label>`" status text. Per the `feature-implement` skill, the i18n pass
  runs after the user has approved the working feature, not inside Phase D.

## Cross-Cutting Concerns

**Parallel-worktree state.** `secret_extract::extract_secret` reads only from
`credentials::bundle_dir` (`src-tauri/src/credentials/mod.rs`), which is already
`MIO_CREDENTIALS_DIR`-then-`MIO_APP_DATA_DIR`-relocatable (`store_dir`'s doc comment) — this feature
introduces no new path of its own, only a new reader of an existing one. No new host port, no new
host-global external resource (podman machine/connection, SSH alias, Mutagen session), and no new
bundled binary are introduced by any phase above, so none of the parallel-worktree constraints in the
`feature-dev` skill beyond "route paths through the existing override" apply here.

**Error-variant mirroring.** `CredentialError::NotFound` already has an `AppError`/`AppErrorKind`
mapping via the existing `credentials/` command surface; no new `AppError` variant is needed for
Phase A or C. If Phase B's per-rule status list needs a distinct `ProxyError` variant for "rule
omitted, not a hard failure," it is added under `network-proxy/02_dev.md`'s own error-mirroring
obligations (`kind()` in `src-tauri/src/error.rs`, `AppErrorKind` in `src/lib/errors.ts`), not this
document's.

**Testing tier.** Phase A's unit tests are the primary regression guard and need no e2e `light`
mock, since `extract_secret` touches no `ProcessRunner`-mediated external process — it is pure
filesystem/parsing logic, the same category as `mio_files.rs`. Phase B–D's behaviour (materialise
omitting an inert rule, the picker command, the UI toggle) is covered by `network-proxy`'s own
`03_e2e.md` once that feature reaches its test-plan step; this document does not add a standalone
`03_e2e.md` because the feature has no independently-runnable surface — every command it adds is
reachable only through `network-proxy`'s proxy panel and rule/secret commands.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `extract_secret` compiles but always returns `NotFound` for a service with a real captured bundle | The descriptor's `file` in `CREDENTIAL_SECRET_DESCRIPTORS` (`src-tauri/src/credentials/secret_extract.rs`) has a leading `/` or is missing the `root/` prefix `tar_relative` (`src-tauri/src/credentials/mod.rs`) writes into the archive | Match the exact tar-relative form (`root/.claude/.credentials.json`, no leading slash); the spike's synthetic entries (`docs/features/proxy-credential-secrets/spike/tar_extract_spike/src/main.rs`) show the expected shape |
| `set_proxy_secret` fails to deserialize a request from an older frontend build | `SecretSource`'s `#[serde(tag = "kind")]` shape replaced a bare `value: String` (`network-proxy/02_dev.md` Resolved Decision 5); a stale frontend still sends the old shape | Not a runtime concern pre-release (nothing has shipped `secrets.json` in the old shape yet, per Resolved Decision 1); if `network-proxy` ships before this feature lands, the migration path becomes a real concern and needs its own decision at that time |
| A proxy rule stays inert after a credential is re-captured on the credentials page | No proactive push exists in v1 (Resolved Decision 3) — materialise only runs on `set_proxy_rules` or an explicit restart | Edit and re-save any rule on the affected sandbox (or trigger `restart_proxy`, `network-proxy/02_dev.md` Phase D) to force re-materialisation |
| `cargo test` fails on the new `secret_extract` tests with a `MIO_CREDENTIALS_DIR` collision when run alongside other tests | `credentials::store_dir` reads `MIO_CREDENTIALS_DIR` from the process environment, which is shared across threads in the same `cargo test` binary | Ensure each test in `secret_extract.rs`'s `#[cfg(test)] mod tests` uses its own `tempfile::TempDir` and sets the env var only for the scope of that test (or passes the bundle path directly if `extract_secret` is refactored to accept one, whichever the implementer finds cleaner at Phase A time) |
| `cargo check` reports an unused-import or visibility error on `json_path_get` after Phase A | `crate::agents::json_path_get` (`src-tauri/src/agents/mod.rs`) is `pub(crate)`, which already covers a call from `credentials/secret_extract.rs` in the same crate — no visibility widening is actually needed, so a diff that adds `pub` instead of `pub(crate)` is doing unnecessary work | Keep `json_path_get` `pub(crate)`; import it as `crate::agents::json_path_get` from `secret_extract.rs` |
| A second agent's descriptor is added but `available_secret_sources` never surfaces it in the picker | `credentials::list` (`src-tauri/src/credentials/mod.rs`) only returns services with a bundle *and* a loadable `manifest.json` — a service can be catalogued in `CREDENTIAL_SECRET_DESCRIPTORS` with no user having ever captured it yet | Expected: capture the credential from the credentials page first; `available_secret_sources`'s intersection is intentional (Phase A), not a bug |
