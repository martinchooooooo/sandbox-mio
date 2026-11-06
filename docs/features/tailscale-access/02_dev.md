---
type: dev-plan
title: "Tailscale Access — Build Plan"
feature: tailscale-access
---
# Tailscale Access — Build Plan

This document is the build plan for the tailscale-access feature ideated in `docs/features/tailscale-access/01_ideas.md`: joining each remotely deployed sandbox to the user's tailnet as its own ephemeral node so other devices reach the zellij web terminal (port 8082) and the paseo agents UI (port 4780) under a MagicDNS name. The plan continues every assumption of the ideas document (mise install via `aqua:tailscale/tailscale`, userspace networking, per-sandbox nodes, OAuth-client credential pasted once) and resolves all eight of its open questions below. Three product-level resolutions (raw ports, automatic join, share dialog with QR) follow the recommended defaults because the user was not available to confirm; each is marked revisitable. Status: no phase is implemented — Phases A through D are all remaining work.

## The Concrete Model

A new backend module `src-tauri/src/tailscale/` (shaped like `src-tauri/src/settings/`: `mod.rs` + `types.rs`, commands registered in `src-tauri/src/lib.rs`) owns the tailnet credential and the Tailscale API client. The user pastes an OAuth client ID and secret (created in the Tailscale admin console with scope `auth_keys` and tag `tag:sandbox-mio`) into a global connect surface; the module persists them as `oauth.json` under the existing credential-store root (`store_dir` in `src-tauri/src/credentials/mod.rs`, service directory `credentials/tailscale/`). When a sandbox deploys, the `deploy_sandbox` command in `src-tauri/src/lib.rs` chains a tailnet provisioning step after `reproduce_remote`: the tailscale module exchanges the OAuth client credentials for an access token (`POST https://api.tailscale.com/api/v2/oauth/token` via the existing `reqwest` dependency), mints a tagged + ephemeral + pre-authorized auth key with a short expiry (`POST /api/v2/tailnet/-/keys`), and the runtime manager installs the binaries with `mise use aqua:tailscale/tailscale` (idempotent, recorded in `mise.toml`/`mise.lock`), starts `tailscaled --tun=userspace-networking --statedir /root/.local/share/tailscale` with `podman exec -d ... mise exec --`, and joins with `tailscale up --authkey <key> --hostname <sandbox-name>`. Other tailnet devices then reach the sandbox at `http://<sandbox-name>.<tailnet>.ts.net:8082` and `:4780` — userspace tailscaled forwards inbound tailnet connections to the container's loopback on the same port, which reaches both zellij (loopback-bound) and paseo. A per-sandbox share dialog mints a fresh zellij one-time token and displays the two URLs, the token, and the paseo password as copyable text plus a QR code. `undeploy_sandbox` and `destroy_sandbox` run `tailscale logout` best-effort before removing the remote container so the ephemeral node deregisters immediately.

## Resolved Decisions

Each entry restates an Open Question from `docs/features/tailscale-access/01_ideas.md` and records the resolution.

1. **Serve vs raw ports** — RESOLVED (default, revisitable): raw ports. Devices open `http://<node>:8082` and `http://<node>:4780` directly; traffic is WireGuard-encrypted despite the `http` scheme. `tailscale serve` is deferred — it adds a serve-config step per service and cert latency for no v1 gain.
2. **Second-device authentication handoff** — RESOLVED (default, revisitable): a per-sandbox share dialog ("Open on another device"). A new Tauri command mints a fresh zellij token (same `zellij web --create-token` exec used by `prepare_terminal` in `src-tauri/src/runtime/mod.rs`) and returns both service URLs, the token, and the paseo password; the frontend renders copyable fields and a QR code per URL.
3. **Key minting transport** — RESOLVED: mint via the Tailscale HTTP API from Rust. `reqwest` with the `json` feature is already in `src-tauri/Cargo.toml`, so the client-credentials token exchange plus key creation adds no dependency. The OAuth-secret-as-authkey shortcut is rejected permanently: the non-expiring client secret must never enter a container.
4. **Daemon supervision after container restart** — RESOLVED: lazy idempotent ensure. A single `ensure_tailnet` orchestration (command layer, `src-tauri/src/lib.rs`) checks node state via `tailscale status --json`, and only when the daemon is down or logged out re-runs daemon start / key mint / `tailscale up`. Called from `deploy_sandbox` and from `start_sandbox` when the sandbox has a sync session (the existing deployed-detection in `sandbox_deployments`).
5. **Local sandboxes** — RESOLVED: out of scope. Provisioning triggers only on the deploy path; local sandboxes never join the tailnet in v1.
6. **Per-sandbox opt-out** — RESOLVED (default, revisitable): automatic join whenever `credentials/tailscale/oauth.json` exists, with a per-sandbox "Disconnect from tailnet" action (runs `tailscale logout` in the container) instead of a deploy-time toggle.
7. **Ephemeral GC window vs stopped sandboxes** — RESOLVED: silent re-join. Because the hostname is the sandbox name, a re-minted node reclaims the same MagicDNS name, so recycling is invisible to the user; the `ensure_tailnet` call on start covers the re-join with no dedicated UI.
8. **Tag name and ACL guidance** — RESOLVED: default `tag:sandbox-mio`, stored in `oauth.json` and editable in the connect form. The connect UI shows a copy-pasteable ACL `tagOwners` snippet and links to the admin-console OAuth-client creation page.

## Remaining: Phase A — Container Spike

Run the spike proposed in the Validation section of `docs/features/tailscale-access/01_ideas.md` before writing Rust: in a throwaway container from `sandbox-mio/base:mise` on `podman-machine-default`, install with `mise use aqua:tailscale/tailscale@latest`, start the daemon with `podman exec -d ... mise exec -- tailscaled --tun=userspace-networking --statedir /root/.local/share/tailscale --socket /run/tailscale/tailscaled.sock`, join a test tailnet with a hand-minted tagged ephemeral pre-authorized key, and open `http://<node>:8082` from a second device against a running `zellij web`.

Risks the spike must clear, recorded back into the ideas document's Validation section with commands and dates:

- The aqua backend install works under `MISE_LOCKFILE=1` and pins into `mise.lock` (so `reproduce_remote` in `src-tauri/src/runtime/mod.rs` replays it).
- `tailscaled` survives the `podman exec -d` session ending and a subsequent `podman exec` reaches it via `--socket /run/tailscale/tailscaled.sock`.
- Inbound tailnet traffic reaches the loopback-bound zellij port end-to-end from a second device.
- Node state in `--statedir` survives container stop/start and the node resumes without a new key inside the ephemeral GC window.
- `tailscale status --json` distinguishes the three states `ensure_tailnet` branches on: daemon down, daemon up but logged out (`NeedsLogin`), and running.

Blocker input required from the user: a test tailnet, the `tag:sandbox-mio` ACL entry, and a hand-minted auth key from the admin console.

## Remaining: Phase B — Tailscale Module and Credential Storage

New module `src-tauri/src/tailscale/` registered via `mod tailscale` in `src-tauri/src/lib.rs`:

- `types.rs` — `TailscaleError` enum (thiserror, variants for IO, HTTP, API-rejection, parse, not-connected); `TailnetCredential` struct (`client_id`, `client_secret`, `tag`, `tailnet_dns_suffix` cached after first API call, `connected_at_ms`); `NodeStatus` struct for the parsed `tailscale status --json` subset (`dns_name`, `online`, `backend_state`).
- `mod.rs` — `save_credential` / `load_credential` / `delete_credential` persisting `oauth.json` under `store_dir(app)?.join("tailscale")` (reusing `store_dir` from `src-tauri/src/credentials/mod.rs` so all plaintext secrets live under one root); `mint_auth_key(credential) -> String` performing the client-credentials token exchange and the key-creation call (`reusable: false`, `ephemeral: true`, `preauthorized: true`, `tags: [credential.tag]`, `expirySeconds: 300` — the key is consumed within seconds of minting).
- Error wiring: a `Tailscale(#[from] tailscale::types::TailscaleError)` variant on `AppError` in `src-tauri/src/error.rs` with a `"Tailscale"` arm in `kind()`, mirrored in the `AppErrorKind` union in `src/lib/errors.ts` (per the error-handling rules in `AGENTS.md`).
- Tauri commands in `src-tauri/src/lib.rs`: `tailscale_connect(client_id, client_secret, tag)` — validates by performing one token exchange, then saves; `tailscale_connection_status()` — returns the stored credential's metadata (never the secret) or not-connected; `tailscale_disconnect()` — deletes `oauth.json` (container nodes are untouched; the UI states that).

The `client_secret` never crosses the IPC boundary after the initial connect call, matching the manifest-only rule documented on `CredentialManifest` in `src-tauri/src/credentials/types.rs`.

## Remaining: Phase C — Runtime Provisioning and Deploy Hook

Runtime trait additions in `src-tauri/src/runtime/types.rs`, implemented for the Podman CLI runtime in `src-tauri/src/runtime/mod.rs` and stubbed in `src-tauri/src/runtime/mock.rs`:

- `tailnet_status(name, connection) -> Option<NodeStatus>` — `mise exec -- tailscale --socket ... status --json` via `podman_on`; `None` when the daemon is not running.
- `ensure_tailnet_up(name, connection, auth_key, hostname)` — idempotent: installs with `mise use aqua:tailscale/tailscale` from the workspace dir (no-op when already in `mise.toml`), starts `tailscaled` with `podman exec -d` when `tailnet_status` is `None` (userspace networking, statedir `/root/.local/share/tailscale` — deliberately outside the Mutagen-synced `/root/work`), and runs `tailscale up --authkey <key> --hostname <name>` when `backend_state` is not `Running`.
- `tailnet_logout(name, connection)` — best-effort `tailscale logout`; errors are logged, never propagated.

Command-layer orchestration in `src-tauri/src/lib.rs`:

- `ensure_tailnet(name, connection)` helper: loads the credential (silently returns when absent — Tailscale never blocks a deploy), checks `tailnet_status`, mints a key via `tailscale::mint_auth_key` only when a join is actually needed, then calls `ensure_tailnet_up`.
- `deploy_sandbox` calls `ensure_tailnet` after `engine.create` (sync session established; Tailscale failure must not fail the deploy — surfaced as a toast-able warning event, not an `AppError`).
- `start_sandbox` calls `ensure_tailnet` when the sandbox has a sync session (deployed), covering daemon death on container restart and ephemeral GC re-join (Resolved Decisions 4 and 7).
- `undeploy_sandbox` and `destroy_sandbox` call `tailnet_logout` before `remove_remote`, next to the existing `sync::forward_terminate` cleanup.
- New command `tailnet_node_status(name)` returning `Option<NodeStatus>` for the per-sandbox UI, and `tailnet_share_info(name)` returning the DNS name, both service URLs, a freshly minted zellij token, and the paseo password (reusing the token-mint exec from `prepare_terminal` and the password source used by the `prepare_agent_web` caller).

## Remaining: Phase D — Frontend Surfaces

- **Global connect surface**: a Tailscale section on the same global page that hosts the shared-agent-auth credentials UI (or a sibling route under `src/routes/`, linked from `src/lib/components/GlobalNav.svelte`). Form fields for client ID, client secret, and tag (default `tag:sandbox-mio`); the copy-pasteable ACL snippet and the admin-console link (Resolved Decision 8); connected-state display from `tailscale_connection_status`; disconnect button with the "existing nodes stay joined" caveat. Built from `src/lib/components/ui/` primitives (field, input, button) per `AGENTS.md`.
- **Per-sandbox node status**: deployed sandboxes (identified via the existing `sandbox_deployments` data) show tailnet state from `tailnet_node_status` — the MagicDNS name when running, nothing when the feature is unconfigured.
- **Share dialog**: an "Open on another device" action on deployed sandboxes calling `tailnet_share_info`, rendering the terminal and agents URLs with copy buttons and QR codes, plus the one-time zellij token and paseo password. QR rendering needs a new frontend dependency (`uqr` — tiny, no DOM assumptions); confirm with the user before adding, per the component-addition rule in `AGENTS.md`.
- **Error surface**: `"Tailscale"` added to `AppErrorKind` in `src/lib/errors.ts`; failures surface via the existing sonner toast pattern.
- **i18n**: all user-facing strings ship hardcoded first and move to Paraglide keys (`messages/en.json` + mirrors) only after the user approves the working feature, per the feature-implement convention.

## Lifecycle and Cross-Cutting Concerns

- **Daemon lifetime**: `tailscaled` runs as a `podman exec -d` child, dies with the container, and is revived lazily by `ensure_tailnet` on deploy and on start-of-deployed-sandbox. No supervisor, no entrypoint change, no Containerfile change.
- **Node lifetime**: ephemeral nodes self-remove after the GC window offline; explicit `tailnet_logout` on undeploy/destroy makes removal immediate. Re-joins reclaim the same MagicDNS name because the hostname is the sandbox name.
- **Secret hygiene**: the OAuth client secret exists in exactly one place (`credentials/tailscale/oauth.json` on the host); containers only ever see short-lived (300 s) single-use auth keys; the secret never crosses Tauri IPC after connect. The tailscale state dir must stay outside `/root/work` or Mutagen syncs the node's WireGuard key to the laptop.
- **Failure isolation**: every tailscale step in deploy/start paths is non-fatal — a tailnet outage or revoked OAuth client degrades to laptop-only access (the Mutagen forward path from `docs/features/remote-terminal/02_dev.md` is untouched and remains the app's own access path).
- **Mock runtime**: `src-tauri/src/runtime/mock.rs` implements the new trait methods as recorded no-ops so existing unit/E2E flows compile; the E2E strategy for tailnet behaviour is deferred to `docs/features/tailscale-access/03_e2e.md`.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `mise use tailscale` fails with "tool not found" during provisioning | The mise registry has no `tailscale` short name (verified 2026-07-03 in `01_ideas.md`) | Use the full backend string `aqua:tailscale/tailscale` in `ensure_tailnet_up` (`src-tauri/src/runtime/mod.rs`) |
| `tailscaled` exits immediately with a TUN error | Sandboxes run pasta networking with no `/dev/net/tun` and no `NET_ADMIN`; capabilities cannot be added to existing containers | Always pass `--tun=userspace-networking` in the daemon start args in `ensure_tailnet_up` |
| `tailscale: command not found` from `podman exec` | Non-interactive exec shells never read `.bashrc`, so mise-managed binaries are off PATH | Route every tailscale invocation through `mise exec --` from the workspace dir (`--workdir /root/work`), as `prepare_agent_web` does for paseo |
| `tailscale` CLI cannot reach the daemon ("failed to connect to local tailscaled") | The CLI defaults to a socket path the non-systemd container never creates | Pass the same `--socket /run/tailscale/tailscaled.sock` to both the `tailscaled` start and every `tailscale` CLI call |
| Key-mint API returns "requested tags are invalid or not permitted" | The tag in the key request is not among the tags the OAuth client was created with, or the tag is missing from the ACL `tagOwners` | Mint with exactly `credential.tag` (`mint_auth_key` in `src-tauri/src/tailscale/mod.rs`); connect UI shows the required ACL snippet |
| Deploy fails outright when the tailnet is unreachable | Tailscale provisioning wired into `deploy_sandbox` as a fatal `?` step | Keep `ensure_tailnet` non-fatal in `src-tauri/src/lib.rs`: log + warning event, never an `AppError` from the deploy path |
| Node joins but the sandbox is unreachable after the container restarts | `tailscaled` died with the container and nothing restarts it eagerly | `start_sandbox` must call `ensure_tailnet` for deployed sandboxes (Resolved Decision 4) |
| Laptop copy of the workspace suddenly contains `tailscaled.state` | Tailscale statedir placed inside `/root/work`, which Mutagen syncs | Statedir is `/root/.local/share/tailscale` in `ensure_tailnet_up`; never move the statedir into the workspace mount |
| Second device gets the URL but zellij rejects the session token | zellij web tokens are one-time; the token shown was already consumed by the laptop webview | `tailnet_share_info` mints a fresh token per dialog open via `zellij web --create-token`, independent of `prepare_terminal`'s token |
| Frontend build fails after adding the `Tailscale` error variant | `kind()` arm in `src-tauri/src/error.rs` or the `AppErrorKind` union in `src/lib/errors.ts` not updated in lockstep | Add both in the same change; the compiler enforces the Rust arm, review enforces the TS union |
