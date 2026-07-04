# Tailscale Access to Deployed Sandboxes

This document covers the ideation for reaching remotely deployed sandboxes from devices other than the laptop running the app, using [Tailscale](https://tailscale.com). Each deployed sandbox joins the user's tailnet as its own node, so the sandbox's in-container web services (the zellij web terminal on port 8082 and the paseo agents UI on port 4780) become reachable from a phone or second laptop under a stable MagicDNS name, without the Mutagen forwarding chain that ties access to the app's host machine today. The end goal is a "Connect Tailscale" surface in the Tauri app where the user provides tailnet credentials once, after which the app provisions the Tailscale tooling and node identity into any deployed sandbox automatically. Nothing of the feature is implemented; this document records the design, the technology decisions, and the desk-research that validated the container-side approach. The seams it builds on live in `src-tauri/src/runtime/mod.rs` (tool install and daemon start), `src-tauri/src/backends/mod.rs` (deploy flow), and `src-tauri/src/credentials/` (host-side credential store).

## Current State

Remote access today is laptop-only. A deployed sandbox is a container on a remote Podman host, created by `deploy` in `src-tauri/src/backends/mod.rs` with `--network pasta:--host-lo-to-ns-lo` and `--publish 127.0.0.1::<port>` for the zellij web port (8082, `ZELLIJ_WEB_PORT`) and the paseo port (`PASEO_WEB_PORT`). The app reaches those ports through a Mutagen forwarding session from the Windows-host loopback over the deploy-time SSH alias (`docs/features/remote-terminal/01_ideas.md`). Every hop terminates on the laptop: no other device can reach the sandbox, and access dies when the laptop or the Mutagen daemon is off.

Seams the feature extends:

- **Per-sandbox tool install**: `RuntimeManager::install(name, package)` in `src-tauri/src/runtime/types.rs` runs `mise use` inside the container, recording the tool in the workspace `mise.toml`/`mise.lock`. `reproduce_remote` in `src-tauri/src/runtime/mod.rs` replays that manifest on the remote container after deploy — so a mise-installed tool is reproduced on the remote for free.
- **Daemon start pattern**: `prepare_agent_web` in `src-tauri/src/runtime/mod.rs` starts a long-lived daemon inside the container with `podman exec -d ... mise exec -- <binary>`, and `prepare_terminal` shows the idempotent check-then-start shape (`zellij web --status` before `--daemonize`). A `tailscaled` daemon follows the same pattern.
- **Host-side credential store**: `src-tauri/src/credentials/mod.rs` persists per-service secret bundles under `app_data_dir()/credentials/<service_id>/` with a `manifest.json`. The Tailscale tailnet credential is a new service entry in this store, not a new storage mechanism.
- **Global connect UI**: the shared-agent-auth work (`docs/features/shared-agent-auth/01_ideas.md`) establishes the pattern of a global, sandbox-independent login surface whose result is injected into sandboxes on demand. "Connect Tailscale" is a sibling of that surface.
- **Deploy hook point**: the `deploy` Tauri command in `src-tauri/src/lib.rs` already chains post-create provisioning (`reproduce_remote`); Tailscale bring-up is one more step in that chain.

## Proposed Model

### Tailscale binaries installed per sandbox via mise (`RuntimeManager::install`)

The `tailscale` CLI and the `tailscaled` daemon are installed into a sandbox with the existing `install` seam using the full backend string `aqua:tailscale/tailscale` — the mise registry has **no** `tailscale` short name (verified 2026-07-03; see Validation), so the aqua backend string is mandatory. The aqua registry entry (`aquaproj/aqua-registry`, `pkgs/tailscale/tailscale/registry.yaml`) downloads Tailscale's official static Linux tarball from `pkgs.tailscale.com` and exposes both the `tailscale` and `tailscaled` binaries. Because the install goes through `mise use`, the pinned version lands in `mise.toml`/`mise.lock` and `reproduce_remote` reinstalls it on the remote container after deploy with no extra transport code.

Non-interactive `podman exec` sessions do not read `.bashrc`, so mise-managed binaries are not on their PATH (the same constraint that forced the zellij and paseo symlinks in `src-tauri/resources/Containerfile`). Daemon start and every CLI call therefore go through `mise exec -- tailscaled ...` / `mise exec -- tailscale ...` from the workspace directory, mirroring how `prepare_agent_web` launches paseo.

### tailscaled in userspace networking mode (new runtime function, `src-tauri/src/runtime/mod.rs`)

Sandboxes run under pasta networking with no `/dev/net/tun` device and no `NET_ADMIN` capability, and Podman cannot add devices or capabilities to an existing container. `tailscaled` is therefore started with `--tun=userspace-networking`, which requires no device and no capability. In userspace mode, inbound connections from the tailnet are forwarded to the same port on the container's `127.0.0.1` — which reaches both paseo (binds `0.0.0.0`) and zellij (binds container-loopback only). Outbound connections from the sandbox into the tailnet would require routing through tailscaled's SOCKS5/HTTP proxy; the feature does not need outbound (devices dial in, the sandbox never dials out to tailnet peers), so no proxy environment is configured.

A new runtime function (peer of `prepare_terminal`) starts the daemon idempotently: check `tailscale status` via `mise exec`, and when the daemon is absent run `podman exec -d ... mise exec -- tailscaled --tun=userspace-networking --statedir /root/.local/share/tailscale --socket /run/tailscale/tailscaled.sock`. The state directory sits deliberately **outside** `/root/work`: the workspace is Mutagen-synced to the laptop, and syncing the node's private key off the container would both leak the key and collide on redeploy. Container overlay storage survives stop/start and dies with `podman rm`, which matches the node's intended lifetime.

### Node identity: one tailnet node per sandbox, joined with a minted auth key

After the daemon is up, the sandbox joins the tailnet with `tailscale up --authkey <key> --hostname <sandbox-name>`. Each sandbox is its own node: it gets a per-sandbox MagicDNS name (`<sandbox-name>.<tailnet>.ts.net`), appears individually in the Tailscale admin console, can be targeted individually by ACLs via its tag, and disappears individually when destroyed. The auth key is minted per sandbox by the app (see the auth model below) as **tagged, ephemeral, and pre-authorized**: tagged because OAuth-minted keys must carry a tag and because the tag is the ACL handle; ephemeral so a destroyed or long-stopped sandbox is garbage-collected from the tailnet automatically; pre-authorized so joining needs no admin-console click.

### Tailnet credential: OAuth client stored in the credential store (`src-tauri/src/credentials/`)

Tailscale offers no third-party interactive OAuth (authorization-code) flow — OAuth clients are client-credentials pairs an admin creates once in the Tailscale admin console. "Login to Tailscale" in the app is therefore a **paste flow**, not a browser flow: the user creates an OAuth client with the `auth_keys` scope and a tag (e.g. `tag:sandbox-mio`) in the admin console, pastes the client ID and secret into the app, and the app stores them as a `tailscale` service bundle in the credential store (`src-tauri/src/credentials/mod.rs`). From then on the app mints per-sandbox auth keys against the Tailscale API (`POST /api/v2/tailnet/-/keys` using the client-credentials token) whenever a sandbox needs to join — OAuth client secrets do not expire, so this is a genuine one-time setup, unlike raw auth keys (maximum 90-day life).

The UI surface is a global page (sibling of the shared-agent-auth credentials page) showing connection status, the tailnet name, and per-sandbox node status, with a link to the Tailscale admin-console page where the OAuth client is created.

### Provisioning trigger: deploy-time hook in the `deploy` command (`src-tauri/src/lib.rs`)

When the credential store holds a `tailscale` bundle, the `deploy` command chains Tailscale provisioning after `reproduce_remote`: install (already reproduced from the manifest if the sandbox had it locally, otherwise installed fresh), start `tailscaled`, mint a key, `tailscale up`. The scope is deliberately remote-only for v1: local sandboxes are reachable at the laptop already, and joining every local sandbox to the tailnet adds node churn with no access gain. A per-sandbox opt-out (or opt-in) toggle is an open question. `undeploy` and `destroy` should run `tailscale logout` (best-effort) so the ephemeral node is removed promptly rather than waiting for the ephemeral GC timeout.

### Service exposure over the tailnet

With the node up, other tailnet devices reach the sandbox directly: `http://<sandbox>.<tailnet>.ts.net:8082` hits zellij and `:4780` hits paseo, because userspace tailscaled forwards inbound tailnet traffic to the container's loopback on the same port. Optionally, `tailscale serve` can front the services with real TLS on port 443 (`tailscale serve --bg --https=443 http://127.0.0.1:8082`), giving browsers a certificate-clean URL; serve is tailnet-only by design (Funnel, which exposes to the public internet, is explicitly out of scope). Whether v1 ships raw ports or `tailscale serve` mappings is an open question, as is how the zellij one-time token and the paseo password reach the second device.

## Technology Choices with Tradeoffs

### Install mechanism: mise `aqua:tailscale/tailscale` (chosen) vs Containerfile bake vs apt vs tsnet

Chosen: per-sandbox install through the existing `RuntimeManager::install` seam with the backend string `aqua:tailscale/tailscale`.

- **Bake static binaries into `src-tauri/resources/Containerfile` (like zellij/paseo)** — rejected for v1: it adds ~25–50 MB to every sandbox image including local-only sandboxes that never use Tailscale, requires a `BASE_IMAGE_VERSION` bump and a rebuild on every remote host (`ensure_image_on` gotcha in `docs/features/remote-deploys/01_ideas.md`), and couples the Tailscale version to image releases instead of the per-sandbox lockfile. Revisit if per-sandbox install latency proves annoying — the bake gives zero install cost at deploy.
- **`apt install tailscale` from Tailscale's Debian repository** — rejected: it is an image-level mechanism (needs the repo key and source list in the Containerfile, so it inherits all the bake downsides), installs a systemd service the container cannot use, and its version is not pinned by `mise.lock`, breaking the local/remote reproduction invariant that `reproduce_remote` relies on.
- **Embed `tsnet` (Tailscale's Go library) in a small proxy binary** — rejected: it means writing and shipping a new Go binary per platform, and `tsnet` exposes one process's listeners rather than a general node, so each new in-sandbox service would need proxy code changes. The static `tailscaled` gives a whole-node join with zero code owned by this project.

### Networking mode: userspace networking (chosen) vs kernel TUN vs host-level tailscaled vs sidecar container

Chosen: `tailscaled --tun=userspace-networking` inside each sandbox container.

- **Kernel TUN mode (`/dev/net/tun` + `NET_ADMIN`)** — rejected: the remote `create` in `src-tauri/src/backends/mod.rs` grants neither, capabilities cannot be added to existing containers (every already-deployed sandbox would need recreation), and granting `NET_ADMIN` weakens the sandbox isolation the product is named for. Userspace mode's only real cost — outbound tailnet traffic needs a SOCKS5 proxy — is irrelevant because the feature is inbound-only.
- **One tailscaled on the remote host with subnet routing or port forwards** — rejected: it requires installing software on the host outside Podman (nothing in the app touches the remote host beyond `podman --connection` and SSH mkdir today), yields one node for all sandboxes (no per-sandbox MagicDNS name, ACL target, or lifecycle), and forces the app to manage a host-port allocation table to disambiguate sandboxes.
- **Tailscale sidecar container sharing the sandbox's network namespace** — rejected: pasta networking is per-container user-mode networking, not a shareable namespace in the rootless setup used here; a sidecar also doubles the container count every backend list/lifecycle operation must filter.

### Tailnet credential: admin-created OAuth client, pasted once (chosen) vs raw auth key paste vs interactive per-sandbox login

Chosen: user pastes an OAuth client ID/secret (scope `auth_keys`, tagged) once; the app mints per-sandbox tagged, ephemeral, pre-authorized auth keys via the Tailscale API.

- **User pastes a reusable auth key directly** — rejected as the primary flow: auth keys expire after at most 90 days, so the "login once" promise breaks quarterly; a single reusable key shared across all sandboxes also has a wider blast radius than per-sandbox minted keys and cannot vary options (ephemeral, tags) per sandbox. Acceptable as a manual fallback for users unwilling to create an OAuth client.
- **Interactive `tailscale up` login URL per sandbox** — rejected: it re-couples authentication to each sandbox (a browser round-trip per deploy), which is exactly the per-sandbox-login problem shared-agent-auth exists to remove. It is the only flow that stores no long-lived secret on the host, so it remains a conceivable privacy-mode fallback.
- **Use the OAuth client secret directly as the auth key** (`tailscale up --authkey tskey-client-...?ephemeral=true&preauthorized=true`) — a documented Tailscale shortcut that skips the minting API call. Rejected as the default because it hands the long-lived, non-expiring client secret to every sandbox container (any process in the sandbox can read it from the daemon's state or process args), whereas a minted key is short-lived and single-purpose. Kept as a simplification option in `02_dev.md` if the minting call proves brittle — see Open Questions.

Constraint to design around: **every key minted by an OAuth client must carry a tag**. The tag (e.g. `tag:sandbox-mio`) must exist in the tailnet ACL before the OAuth client can be created, so the connect UI must document a two-step admin-console setup (add tag to ACL, create OAuth client with that tag).

### Node granularity: per-sandbox node (chosen) vs per-remote-host node

Chosen: each deployed sandbox is its own tailnet node. The per-host alternative (one node fronting many sandboxes) was rejected under the networking-mode tradeoff above; the additional per-sandbox arguments are naming (MagicDNS gives `<sandbox>.<tailnet>.ts.net` with zero port bookkeeping) and lifecycle (an ephemeral node vanishes with its sandbox; a shared host node would leak stale port mappings). The cost — more devices in the tailnet — is bounded by Tailscale's free-plan 100-device limit, which is ample for the target usage.

### Node lifetime: ephemeral + `--statedir` in container overlay (chosen) vs durable node vs state in workspace

Chosen: ephemeral pre-authorized keys, node state under `/root/.local/share/tailscale` in the container overlay.

- **Durable (non-ephemeral) nodes** — rejected for v1: destroyed sandboxes would accumulate as offline devices in the admin console, and deleting them requires the `devices` API scope on the OAuth client (broader than `auth_keys`). Ephemeral nodes self-clean after going offline. The cost: a sandbox stopped longer than the ephemeral GC window loses its node identity and must re-join (re-mint) on next start — acceptable because the app mints keys automatically.
- **State dir inside the workspace (`/root/work`)** — rejected outright: the workspace is Mutagen-synced to the laptop, so the node's WireGuard private key would be copied off the container and would collide when a redeployed container replays stale state.

## Validation

Desk research performed 2026-07-03 (no in-container spike yet):

- **mise registry has no `tailscale` short name** — confirmed by listing `jdx/mise` `registry/` contents via the GitHub API (`tailpipe.toml` exists, `tailscale.toml` does not). Consequence: the install string must be the full `aqua:tailscale/tailscale` backend form.
- **aqua registry ships both binaries from official static builds** — confirmed by fetching `https://raw.githubusercontent.com/aquaproj/aqua-registry/main/pkgs/tailscale/tailscale/registry.yaml`: type `http`, URL template `https://pkgs.tailscale.com/stable/tailscale_{{trimV .Version}}_{{.Arch}}.tgz`, files `tailscale` and `tailscaled`, `supported_envs` includes linux.
- **Userspace networking handles inbound without TUN/NET_ADMIN** — Tailscale docs state userspace mode drops the `CAP_NET_ADMIN`/`CAP_NET_RAW` requirement and forwards incoming tailnet connections to the same port on localhost (the server sees `127.0.0.1` as source). This is the load-bearing fact for reaching zellij's loopback-only bind.
- **OAuth-minted keys must be tagged; ephemeral/pre-authorized are key options** — Tailscale OAuth-clients and auth-keys docs.

Proposed spike (not yet run — record results here when done): in a throwaway container from `sandbox-mio/base:mise` on `podman-machine-default`, run `mise use aqua:tailscale/tailscale@latest`, start `tailscaled --tun=userspace-networking` via `podman exec -d ... mise exec`, join a test tailnet with a manually minted tagged ephemeral key, and from a second device open `http://<node>:8082` against a running `zellij web`. Risks the spike must clear: (1) the aqua-backend install works under `MISE_LOCKFILE=1` and lands in `mise.lock`; (2) `tailscaled` runs as PID-non-1 under `podman exec -d` and survives the exec session ending; (3) inbound to the loopback-bound zellij port actually works end-to-end; (4) node state survives container stop/start and the node resumes without a new key inside the ephemeral GC window.

## Open Questions

- **Serve vs raw ports**: Does v1 expose zellij/paseo at raw tailnet ports (`:8082`, `:4780`) or behind `tailscale serve --https=443` (TLS, one hostname, but only one service per port and an extra serve-config step)? Affects the URL the UI shows for "open from another device".
- **Second-device authentication handoff**: The zellij web token is one-time and minted by `prepare_terminal` on the laptop; the paseo password is generated app-side. How does a phone or second laptop obtain them — QR code in the app, a long-lived token variant, or Tailscale identity headers via serve?
- **Key minting transport**: Mint per-sandbox keys via `POST /api/v2/tailnet/-/keys` from Rust (new HTTP dependency in `src-tauri`), or use the OAuth-secret-as-authkey shortcut and accept the secret reaching the container? The rejection rationale above prefers minting; `02_dev.md` must confirm the client-credentials token exchange works from `reqwest` (or the Tauri HTTP plugin) without extra scopes.
- **Daemon supervision after container restart**: `tailscaled` started with `podman exec -d` dies with the container. Is bring-up re-run lazily (an idempotent ensure-up called from status polling, like `prepare_terminal`) or eagerly on every sandbox start? Interacts with the ephemeral GC window.
- **Local sandboxes**: v1 is remote-only. Should local sandboxes optionally join the tailnet too (useful when the laptop sleeps but the podman machine keeps running — currently they die together, so likely no)?
- **Per-sandbox opt-out**: Is tailnet join automatic for every deploy once the credential exists, or a per-sandbox toggle in the deploy UI? Automatic is fewer clicks; a toggle avoids surprising users who deploy sensitive work.
- **Ephemeral GC window vs stopped sandboxes**: A sandbox stopped for hours loses its node (ephemeral GC) and its MagicDNS name until re-join. Is silent re-mint on start acceptable, or does the UI need to surface "tailnet identity was recycled"?
- **Tag name and ACL guidance**: What tag does the app instruct the user to create (`tag:sandbox-mio`?), and does the connect UI ship a copy-pasteable ACL snippet?

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `mise use tailscale` fails with "tool not found" | The mise registry has no `tailscale` short name (verified 2026-07-03) | Use the full backend string `aqua:tailscale/tailscale` |
| `tailscaled` exits with a TUN device error at startup | Sandboxes run pasta networking with no `/dev/net/tun` and no `NET_ADMIN`; kernel mode cannot work and capabilities cannot be added post-create | Always pass `--tun=userspace-networking` |
| `tailscale`/`tailscaled` not found in `podman exec` | Non-interactive exec shells never read `.bashrc`, so mise-managed binaries are off PATH (same as the paseo gotcha in `docs/features/agent-web-ui/02_dev.md`) | Invoke through `mise exec -- tailscale ...` from the workspace dir, or symlink like zellij/paseo |
| Key minting API returns "exactly one capability scope must be populated" or a tag error | Auth keys minted by an OAuth client must specify a tag the client was created with | Create the OAuth client with the sandbox tag and pass that tag in the key-creation request body |
| Sandbox unreachable from tailnet after being stopped for a while | Ephemeral nodes are garbage-collected after staying offline past the GC window; the saved state in `--statedir` is then invalid | Re-run the join flow (mint a fresh key, `tailscale up`) on sandbox start when `tailscale status` reports logged-out |
| Node's WireGuard private key appears on the laptop, or redeploy fights over node identity | Tailscale state dir placed inside `/root/work`, which Mutagen syncs bidirectionally | Keep `--statedir` under `/root/.local/share/tailscale`, outside the workspace mount |
| A service in the sandbox cannot reach other tailnet devices | Userspace networking only forwards inbound; outbound tailnet traffic needs tailscaled's SOCKS5/HTTP proxy | Out of scope for this feature (inbound-only); if ever needed, set `ALL_PROXY=socks5://localhost:1055` and start tailscaled with the proxy flags |
| Service sees every tailnet visitor as `127.0.0.1` | Userspace mode forwards inbound connections via a localhost dial, discarding the peer source IP | Accept it; if per-user identity is needed later, `tailscale serve` can inject Tailscale identity headers |
| Tailnet connect UI works but deploy-time join fails only on the remote | `reproduce_remote` installs the mise-pinned tailscale binaries, but the join step also needs the daemon started and a key minted — steps that live outside the mise manifest | Chain daemon start + key mint + `tailscale up` after `reproduce_remote` in the `deploy` command in `src-tauri/src/lib.rs` |
