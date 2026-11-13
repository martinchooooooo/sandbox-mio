---
type: ideas
title: "Podman Egress Boundary: Internal Network and iron-proxy Sidecar"
feature: podman-egress-boundary
---
# Podman Egress Boundary: Internal Network and iron-proxy Sidecar

This document covers giving Podman sandboxes a real, fail-closed network boundary and a complete source of traffic data by adopting existing tools instead of growing the project's hand-written mitmproxy addons. The proposal has two halves. The boundary is Podman's own `--internal` network: the sandbox joins a network with no route out, and the proxy sidecar is dual-homed onto that network and an external one, so the sidecar is the only exit. The proxy is iron-proxy (github.com/paradigmxyz/iron-proxy; images and releases published as `ironsh/iron-proxy`), replacing the mitmproxy sidecar for allowlist policy, secret swapping and the per-request audit record that feeds the existing traffic panel. The user prefers an existing tool over a home-grown one and favours this design; the tradeoffs below are recorded as found. Nothing is implemented and no spike has run: everything about iron-proxy comes from its README and source, read on 2026-11-13 (Validation). Today Podman egress policy is "Not implemented" (`docs/architecture/network.md`, Capability Matrix), and the Podman sidecar is reachable only through `http_proxy` environment variables, so any tool that ignores them leaves over the bridge's NAT uninspected. Relevant code: `src-tauri/src/proxy/` (sidecar lifecycle, rules, flow log), `src-tauri/src/net_policy.rs` (the shared `.mio/net-policy.json`), `src-tauri/src/provider/` (the Podman network and sidecar seam), `src-tauri/resources/mitm_addon.py` and `src-tauri/resources/mitm_flowlog_addon.py` (the two addons iron-proxy would replace), `src/lib/proxy/` and `src/routes/projects/` (`ProxyPanel.svelte`, `TrafficPanel.svelte`, `NetPolicyEditor.svelte`).

## Current State

**The Podman sidecar is an observer, not a gate.** `create_podman_proxy` in `src-tauri/src/proxy/mod.rs` creates a per-sandbox `mio-proxy-<sandbox>` bridge (`allocate_network` walks `10.90.<index>.0/24`), runs `docker.io/mitmproxy/mitmproxy:11.1.3` (`MITMPROXY_IMAGE_DEFAULT`, overridable by `MIO_PROXY_IMAGE`) pinned at `.2`, and returns a `ProxyAttachment::PodmanBridge` (`src-tauri/src/proxy/types.rs`) whose environment comes from `proxy_env`. The sandbox joins the bridge instead of `pasta` (`docs/features/network-proxy/02_dev.md`, Resolved Decision 1). Both containers share the bridge's outbound NAT, so traffic from a tool that ignores `http_proxy`/`https_proxy` leaves directly; `docs/features/network-proxy/02_dev.md`, Resolved Decision 7, accepted that gap.

**Two spike facts constrain every network design.** `aardvark-dns` cannot start on `podman-machine-default` ("Failed to connect to user scope bus via local transport"), so `PodmanProvider::create_network` (`src-tauri/src/provider/mod.rs`) always passes `--disable-dns` and containers address each other by static IP. Podman rejects `pasta` combined with a named network ("cannot set multiple networks without bridge network mode, selected mode pasta"). Both are recorded in `docs/features/network-proxy/02_dev.md`, Spike Results.

**The sidecar seam takes one network.** `SidecarSpec.network` (`src-tauri/src/provider/types.rs`) is a single `Option<InstanceNetwork>`, and `MachineProvider::create_network(name, subnet)` has no internal-network flag. Dual-homing the sidecar needs both seams widened.

**mitmproxy owns three jobs today.** It generates the per-sandbox CA into a bind-mounted confdir (`wait_for_ca`, then `inject_ca` pipes the PEM into the sandbox; per-runtime CA variables come from `proxy_env`, `docs/features/network-proxy/02_dev.md`, Resolved Decision 8). `mitm_addon.py` injects a header the sandbox never sent, per rule in `.mio/proxy-rules.json` (`src-tauri/src/proxy/rules.rs`), with values materialised from `<app_data_root>/proxy/secrets.json`. `mitm_flowlog_addon.py` appends metadata-only rows (method, host, path, status, timing, sizes, `secretInjected`) to `flows.ndjson`, which `watch_flows` in `src-tauri/src/proxy/flowlog.rs` tails into `proxy://flows-changed` for `src/lib/proxy/traffic.svelte.ts` and `TrafficPanel.svelte`. Rule or addon changes restart the sidecar (`restart_proxy_container`, `docs/features/network-proxy/02_dev.md`, Resolved Decision 11).

**The policy file already exists, Podman ignores it.** `.mio/net-policy.json` (`NetPolicy` in `src-tauri/src/net_policy.rs`) is the shared provider-neutral format: egress mode (`open` or `allowlist`), ordered allow/deny rules targeting a host, CIDR or group with optional protocol and ports, secret bindings (`SecretBinding`: secret id, guest variable, hosts, headers/query/body), a violation action and a reserved `interactive` flag. `ProviderCapabilities.egress_policy` is `false` for Podman (`src-tauri/src/provider/mod.rs`), and `net_policy::validate` refuses what a provider cannot enforce.

**microsandbox already has the target shape.** microsandbox enforces egress in its host-side stack, substitutes a `$MSB_<ENV>` placeholder for the real value only toward bound hosts, and chains an authenticated SOCKS5 mitmproxy sidecar VM for the viewer (`docs/features/microsandbox-network-policy/01_ideas.md`). NVIDIA OpenShell was deferred there: WSL-only and experimental on Windows, a `systemd --user` Podman socket the machine lacks, Landlock ABI 3 and seccomp user-notification in the WSL kernel, and no egress for `podman exec` processes, which breaks the exec transport (`docs/features/microsandbox-network-policy/04_improvements.md`, Adopting NVIDIA OpenShell).

**Local inference does not use the sandbox network.** Local-model traffic reaches the host over the sandbox's own loopback forward (`docs/architecture/network.md`, Invariants; `no_proxy` in `proxy_env` covers loopback), and the terminal, agent web UI and port forwards ride the `podman exec` transport. Neither path should need a route out, which spike 1 must confirm.

## Relationship to Planned Proxy Features

- **`proxy-network-policy`** (`planned`; `docs/features/proxy-network-policy/`): hand-rolled `mitm_policy_addon.py` for a private-range block and host allow/deny, plus an interactive approval channel that holds the flow while the user answers. This feature replaces the whole enforcement half with iron-proxy's `allowlist` transform and upstream deny list, and replaces the held-flow approval with a reject-then-reload shape (Behaviour Changes). Its `feature.md` already says its `02_dev.md` needs a refresh. Expressed per `docs/features/README.md`: no change to `proxy-network-policy`'s documents at idea stage; when this feature's `02_dev.md` is accepted, `proxy-network-policy/feature.md` moves to `status: superseded` with `superseded_by: podman-egress-boundary` and a `**Supersede**` entry goes into `docs/features/log.md`. Whether any of its approval-channel design survives is Open Question "Supersede scope".
- **`proxy-non-http-capture`** (`planned`): counts connections that bypass the proxy. Inside the boundary such connections fail instead of leaving, so the feature's purpose disappears for bounded sandboxes; blocked non-HTTP attempts become invisible rather than uninspected (Behaviour Changes).
- **`proxy-credential-secrets`** (`planned`): sources secret values from the credential store. Orthogonal: it changes where a value comes from, while this feature changes how the value reaches the request (a file the sidecar reads, not a header rule).
- **`proxy-traffic-viewer`** (`implemented`): keeps its frontend; its data source changes from `mitm_flowlog_addon.py` to the iron-proxy audit stream.
- **`proxy-toggle-management`** and **`proxy-remote-deploy`** (`planned`): unchanged constraints. Proxy and boundary state stay baked into `podman create`, so toggling still means recreating, and deploying a bounded sandbox stays refused.

## Proposed Model

### Boundary: an internal sandbox network with a dual-homed sidecar

`allocate_network` (`src-tauri/src/proxy/mod.rs`) creates `mio-proxy-<sandbox>` as an internal network (`podman network create --internal --disable-dns`), so the sandbox has no gateway route and no NAT. The sidecar joins that network at the pinned `.2` address and also joins an external network, which gives the sidecar alone a path out (choice under Technology Choices). The seams: `MachineProvider::create_network` gains an internal flag, `SidecarSpec` gains a second network attachment (or the Podman path runs `podman network connect` after create), and `PodmanProvider::create_sidecar` renders both. The sandbox still joins only `mio-proxy-<sandbox>` instead of `pasta`. Raw-IP connections, a custom resolver, git over SSH and any other non-proxied socket then fail at the kernel with no route, instead of leaving uninspected. No container gains `NET_ADMIN`.

### Proxy: iron-proxy replaces mitmproxy in the Podman sidecar

`create_podman_proxy` runs a pinned `ironsh/iron-proxy` image with `-config` pointing at a YAML file the app generates into `<app_data_root>/proxy/<sandbox>/` and mounts read-only. The listeners used: the tunnel listener (`proxy.tunnel_listen`, absolute-form HTTP, CONNECT and SOCKS5) on the port `proxy_env` already advertises; the HTTP and HTTPS listeners (`:80`, `:443`) that DNS-routed traffic lands on; the DNS server (`:53`); and the management listener bound to the sidecar's own loopback. The transform pipeline is `allowlist`, then `secrets`; `annotate`, `body_capture`, `header_allowlist`, `judge` and MCP policy stay off (`04_improvements.md`). `proxy.upstream_deny_cidrs` replaces the default list (metadata endpoints and loopback only; RFC 1918 is intentionally excluded upstream) with one that also covers private and link-local ranges, which is the private-range block `proxy-network-policy` planned in `server_connect`.

### Routing: tunnel listener for proxy-aware tools, built-in DNS for the rest

Proxy-aware tools keep using `http_proxy`/`https_proxy` from `proxy_env`, now pointed at the tunnel listener. The sandbox is created with `--dns <sidecar ip>`, and iron-proxy's DNS answers every name with the proxy IP, so a tool that ignores proxy variables still resolves to the sidecar and is SNI-routed through the same pipeline on ports 80 and 443. The DNS server never forwards a query upstream unless a `dns.passthrough` pattern matches, which closes the DNS exfiltration channel `docs/features/microsandbox-network-policy/01_ideas.md` warns about for `allow@dns`. The built-in DNS may also cover the missing `aardvark-dns` on the podman machine (spike 2). A DNS-routed connection to a port other than 80 or 443 lands on a closed port of the sidecar and fails.

### Policy: `.mio/net-policy.json` rendered into iron-proxy config, applied by reload

A new renderer beside `src-tauri/src/provider/msb_policy.rs` (for example `src-tauri/src/proxy/iron_config.rs`) turns `NetPolicy` (`src-tauri/src/net_policy.rs`) into the YAML: `allowlist` mode becomes the `allowlist` transform's `domains` and `cidrs`; `open` mode becomes the same transform with `warn: true`, so every request passes but would-be rejections are annotated. iron-proxy's allowlist has no ports, protocols, groups or deny entries, so `net_policy::validate` refuses rules Podman cannot honour (Open Question "Rule vocabulary gaps"), matching the invariant that a control a provider cannot honour is refused. Setting `ProviderCapabilities.egress_policy` true for Podman lets `NetPolicyEditor.svelte` show the editor. A policy change rewrites the YAML and calls `POST /v1/reload` through `podman exec` against the sidecar's loopback management listener, which per the README swaps in a rebuilt pipeline atomically without a process restart and keeps the old pipeline if the new config is invalid. Unlike microsandbox, no sandbox recreate is needed.

### Secrets: placeholder-token swap replaces rule-based header injection

Each `SecretBinding` in `.mio/net-policy.json` renders to one iron-proxy `secrets` entry: a `file` source under the sidecar state directory (one file per secret, written atomically, mounted read-only), a `proxy_value` placeholder, `match_headers` / `match_query` / `match_body` from the binding, and `rules` limited to the binding's hosts. The sandbox gets the placeholder in the binding's variable (`env_var`) at create. Real values never enter the sandbox, a rotated value is picked up on reload because `file` sources are re-read then, and `require: true` can reject a request to a bound host that carries no placeholder. The secret model then matches microsandbox's: one binding format, two enforcers. The cost is migration: `.mio/proxy-rules.json` rules inject a header the agent never sent, while a placeholder must be sent by the agent, typically through an API-key variable (Open Question "Migration of existing proxy rules").

### CA: generated by the app, not by the proxy

iron-proxy terminates TLS with a CA it is given (`tls.ca_cert`, `tls.ca_key`) and does not create one at start, unlike mitmdump, which wrote its CA into the confdir that `wait_for_ca` polls. The app therefore produces the per-sandbox CA before the sidecar starts, into `<app_data_root>/proxy/<sandbox>/certs/`, and `inject_ca` and `proxy_env`'s CA variables keep working against the PEM. The state directory still holds the CA key and is still deleted by `remove_proxy`. Choice of generator under Technology Choices.

### Traffic data: the audit stream translated into the existing flow record

Every request iron-proxy handles emits one structured JSON audit record with host, method, path, action, status code, duration, client address, SNI, the transform trace, and `rejected_by` on a rejection; the `secrets` transform's trace names which secrets were swapped. The backend translates each record into the existing `FlowEntry` (`src-tauri/src/proxy/flowlog.rs`), so `traffic.svelte.ts` and `TrafficPanel.svelte` keep their contract and the `FLOW_LOG_MAX_LINES` cap stays shared. `secretInjected` comes from the swap annotation instead of re-matching rules, which also fixes the always-false value microsandbox rows show today. A rejected row gains a reason. Request and response sizes are absent from the audit record as read (Validation), so `reqBytes` and `respBytes` would be empty. How the stream reaches the host is Open Question "Audit transport".

### Behaviour changes users see

- Non-HTTP(S) traffic from a bounded sandbox goes from "works, uninspected" to "blocked": git over SSH, raw TCP, database clients, custom ports. Git over HTTPS keeps working. iron-proxy's opt-in SSH credential listener might later restore git over SSH (`04_improvements.md`).
- The viewer and the boundary are coupled: a bounded sandbox always runs a sidecar, because the sidecar is its only exit. A stopped sidecar already fails closed today (`docs/architecture/network.md`, Invariants); under the boundary the failure covers every protocol.
- The first request to an unlisted host gets a 403. iron-proxy has no hook that holds a flow while a human decides, so approval can only be built from audit rejections plus a reload, after which the agent retries (Open Question "Interactive approval").

## Technology Choices with Tradeoffs

### Boundary mechanism: Podman `--internal` network (chosen) vs. nftables in the sandbox vs. TPROXY vs. DNS-only routing vs. NVIDIA OpenShell

Chosen: an internal network, because the kernel enforces it outside the sandbox, no container needs a new capability, and the network is per-sandbox state named from a run-scoped sandbox name like `mio-proxy-<sandbox>` today.

- **nftables egress rules inside the sandbox** (iron-proxy's own "enforced" mode) — rejected: loading rules needs `CAP_NET_ADMIN` in the client container, which `docs/features/network-proxy/01_ideas.md` (Technology Choices) and `docs/features/tailscale-access/01_ideas.md` already rejected on isolation grounds, and capabilities cannot be added to existing containers.
- **TPROXY** — rejected: needs `ip rule` / `ip route` setup, nftables marking and an `IP_TRANSPARENT` bind on a host that routes the sandbox's packets, which means privileged changes inside the podman machine's network namespace: host-global state that cannot be isolated per worktree.
- **DNS-only routing** (iron-proxy's "simple" mode) — rejected as the boundary: iron-proxy's README states that a workload can bypass it with hard-coded IPs or its own resolver. Kept as the routing layer inside the boundary.
- **NVIDIA OpenShell** — rejected for the reasons recorded in `docs/features/microsandbox-network-policy/04_improvements.md`, chiefly that `podman exec` processes get no egress, which breaks the exec transport.
- **coder/httpjail** — rejected: needs root and nftables on the host plus the local Docker socket.

### Proxy engine: iron-proxy (chosen) vs. keeping mitmproxy vs. Pipelock vs. other egress proxies

Chosen: iron-proxy, because default-deny allowlist with an observe mode, upstream IP deny on the resolved address, placeholder secret swap with re-readable file sources, a per-request structured audit with the rejecting transform named, live reload, and a built-in DNS server are all documented features rather than addon code the project maintains. The cost: a young, fast-moving 0.x project (created 2026-04-01, v0.56.0 released 2026-11-04, about 860 stars, Apache-2.0), owned by the company behind the commercial iron.sh, so the image must be pinned to an exact version and every upgrade re-verified. Figures for the alternatives come from a GitHub check on 2026-11-13 (stars, last push and licence from the GitHub API, features from READMEs only, nothing run).

- **Keeping mitmproxy, using its built-in `block_list` option for policy** — rejected as the primary path but the cheapest fallback: no new image, and the `--internal` boundary works with any proxy. It keeps both custom addons (`mitm_addon.py`, `mitm_flowlog_addon.py`), adds policy as filter expressions with no observe mode, has no placeholder swap, and leaves CA, reload and private-range blocking as project code (`proxy-network-policy`'s Phase A1 found `server_connect` needs self-resolution).
- **Pipelock** (pipelab-org/pipelock, 990 stars, pushed 2026-11-13, Apache-2.0 core plus ELv2 enterprise) — the fallback candidate: strict allowlist and audit modes, TLS interception, JSONL events, exfiltration and prompt-injection scanning. Rejected for now: no secret injection found, and the multi-agent dashboard is paid.
- **Stripe Smokescreen** (1,404 stars, pushed 2026-11-12, MIT, since 2016) — rejected: a mature hostname-ACL CONNECT proxy, but no secret injection seen and, as far as its README shows, no TLS interception, so no method/path/status for the viewer.
- **Infisical Agent Vault** (2,504 stars, pushed 2026-11-07, MIT except `ee/`) — rejected: a credential-brokering MITM proxy with its own vault and server, default-deny not confirmed, heavy per sandbox.
- **Brex CrabTrap** (800 stars, pushed 2026-11-10, MIT) — rejected: static rules plus an LLM judge and a web UI, but it needs PostgreSQL and has no human approval. iron-proxy's `judge` transform credits CrabTrap's design.
- **Process sandboxes** (anthropics/sandbox-runtime, nono, fence) — rejected: they wrap a host process rather than a container, so they do not fit a Podman sandbox.
- **mitmproxy reference designs** (sandcat, mattolson/agent-sandbox) — rejected: both need `NET_ADMIN` in the agent container.
- **strongdm/leash, OpenSnitch** — rejected: stale.

### Sidecar external leg: open choice between the default `podman` network, a per-sandbox egress bridge and one shared app-created network

No pick yet (Open Question "External network for the sidecar"). The default `podman` network is created by Podman itself, so the app allocates nothing, but whether a rootless machine ships it with DNS off is unverified. A per-sandbox `mio-egress-<sandbox>` bridge stays per-sandbox but doubles subnet allocation in the racy `10.90.<index>` walk (`docs/features/network-proxy/02_dev.md`, Cross-Cutting Concerns). One shared app-created network is host-global state shared by every worktree, with a create race and no safe owner to remove it, so it is the least preferred.

### Secret delivery: placeholder swap from `file` sources (chosen) vs. `env` sources vs. keeping header injection by rule

Chosen: `file` sources, because a value file under the app data root is mounted only into the sidecar, re-read on reload, and absent from `podman inspect`.

- **`env` sources** — rejected: values would sit in the sidecar's container environment, visible to `podman inspect`, and fixed at process start.
- **Header injection by rule** (today's `mitm_addon.py`) — not available: the README documents no transform that adds a header the workload did not send, except `mcp_gateway` credentials, which apply only to MCP routes. Undocumented transforms in the source tree (`oauth`, `hmacsign`, `awsauth`, `gcpauth`) were not examined. Corrected by Phase A: the `secrets` transform has an `inject` mode that does add such a header; the user kept placeholder swap anyway (`02_dev.md`, Resolved Decision 8).

### Traffic source: audit records translated host-side (chosen) vs. OpenTelemetry export vs. chaining mitmproxy behind iron-proxy

Chosen: translate audit records into `FlowEntry` in the backend, keeping the frontend contract.

- **OpenTelemetry export to an app-hosted collector** — rejected: the app would host an OTLP endpoint reachable from the sidecar's external leg, which adds a listener and a host-to-container path for data the audit stream already carries.
- **mitmproxy chained behind iron-proxy** — rejected: two TLS interceptions and two images per sandbox to keep a view the audit record mostly reproduces.

### CA generation: app-side generation (chosen, generator open) vs. reusing a mitmproxy CA

Chosen: the app generates the CA before creating the sidecar, either with a Rust certificate crate or by running `iron-proxy generate-ca` (a subcommand present in `cmd/iron-proxy/generate_ca.go`, default validity 8,760 hours, RSA 4096 or Ed25519) in a one-shot container on the pinned image. The one-shot container needs no new Rust dependency but adds a container run per create; a one-year default validity would also need overriding for long-lived sandboxes. Reusing mitmproxy's generated CA was rejected because it keeps mitmproxy in the create path only to mint a certificate.

### Applying changes: `POST /v1/reload` through `podman exec` (chosen) vs. restarting the sidecar

Chosen: reload, because a restart drops the agent's open streams (SSE, WebSocket) and briefly fails every request closed; whether reload itself leaves open streams untouched is unverified. The management listener binds the sidecar's loopback, so the sandbox cannot reach it, and the app reaches it with `podman exec` and the image's own HTTP client (the release image is Alpine). Restart (`restart_proxy_container`, today's mechanism) remains the fallback when reload reports an error.

## Validation

**Desk research only, 2026-11-13.** Nothing was run, no image was pulled and no network was created on the podman machine. Read: iron-proxy's README, `Dockerfile.release`, `iron-proxy.example.yaml`, `cmd/iron-proxy/generate_ca.go`, `cmd/iron-proxy/main.go`, `internal/dnsguard/dnsguard.go` and `internal/transform/audit.go`, all at the default branch on that date, which may differ from tag `v0.56.0`. Repository metadata (created 2026-04-01, pushed 2026-11-11, 861 stars, Apache-2.0, latest release `v0.56.0` on 2026-11-04) came from the GitHub API.

Findings that refine the proposal:

- **Audit records go to stderr.** The README describes "JSON stderr logs" and optional OpenTelemetry export; no file output option was found. The release image is `alpine:latest` plus `ca-certificates` and `openssl` with `iron-proxy` as entrypoint, so a shell wrapper that redirects stderr into a mounted file looks possible but is unverified.
- **Audit fields are nested and lack sizes.** `internal/transform/audit.go` emits the fields under an `audit` group (host, method, path, `remote_addr`, `sni`, `mode`, action, `status_code`, `duration_ms`, `tunnel.target`, `rejected_by`, error, transform traces), while the README example shows them flat. No request or response byte count was found.
- **The default upstream deny list excludes RFC 1918.** `DefaultDenyCIDRs` covers three metadata addresses plus loopback, and a source comment says private ranges are left out on purpose; the app must supply its own list.
- **The SOCKS5 tunnel supports no authentication.** The README says "no-auth only", which conflicts with the mandatory SOCKS5 authentication on microsandbox's host-loopback chain (`docs/features/microsandbox-network-policy/01_ideas.md`, Mitmproxy placement).
- **The README's secrets examples disagree with each other.** One example nests `proxy_value` under `replace:`; another puts it at the entry's top level. `match_query` is documented as default false, yet a later line says "Query parameters are always scanned". The current config shape must come from a real run.
- **The CA can come from the image.** A `generate-ca` subcommand exists (see CA generation above).

Gating spikes, **not yet run**:

| # | Spike | Clears |
|---|---|---|
| 1 | Rootless `podman network create --internal --disable-dns` plus a dual-homed static-IP sidecar on `podman-machine-default`: the sandbox has no direct route out, the sidecar does, and the exec transport (terminal, Mutagen, port forwards) and the local-LLM loopback forward still work | The boundary itself; External network for the sidecar |
| 2 | `--dns <sidecar ip>` on a `--disable-dns` network, with iron-proxy's DNS answering; a non-proxy-aware client reaches an allowed host and a raw-IP client fails | DNS routing; the `aardvark-dns` gap |
| 3 | Whether the audit record carries what `TrafficPanel.svelte` shows (method, host, path, status, timing, sizes), its real JSON shape, and whether it can be written to a mounted file or only to container logs | Audit transport; `FlowEntry` mapping |
| 4 | mise, npm, pip and git (HTTPS) through iron-proxy with the existing per-runtime CA variables from `proxy_env` | Toolchain compatibility |
| 5 | Config and secret file reload (`POST /v1/reload`) from a bind-mounted Windows host directory | Reload-based policy and secret rotation |

## Open Questions

- **Supersede scope.** Does this feature supersede `proxy-network-policy` entirely, or does the approval channel designed in its Phases D–F (reverse tunnel, pending-request registry) survive as the transport for reject-then-reload approval?
- **Interactive approval.** Is reject-then-reload approval (audit `rejected_by` row, user approves, policy rewritten, reload, agent retries) in scope for the first build, or deferred until the static boundary ships?
- **External network for the sidecar.** Default `podman` network, a per-sandbox egress bridge, or a shared app-created network (Technology Choices)?
- **Audit transport.** Shell-wrapped stderr redirected into a mounted file that `watch_flows` keeps tailing, or `podman logs --follow` streamed through the process layer and translated in Rust?
- **Opt-in shape and default mode.** Is the boundary always on whenever the Podman proxy is on, or a separate toggle? Does a new Podman sandbox default to `open` (rendered as `warn: true`), mirroring microsandbox's permissive default (`docs/features/microsandbox-network-policy/01_ideas.md`, Resolved Decision 3)?
- **Rule vocabulary gaps.** iron-proxy's allowlist has domains and CIDRs only. Do `.mio/net-policy.json` deny rules, groups, protocols and ports get refused for Podman by `net_policy::validate`, approximated (CIDR denies into `upstream_deny_cidrs`), or dropped with a warning? Does iron-proxy's `*.example.com` also matching `example.com` diverge acceptably from microsandbox's suffix rules?
- **Placeholder delivery after create.** Container environment is fixed at `podman create`. Does adding a secret binding to a running sandbox require a recreate, or does the placeholder reach the guest another way (the placeholder is not secret, so a workspace file is possible)?
- **Migration of existing proxy rules.** For sandboxes created with `.mio/proxy-rules.json` header rules: convert rules into bindings automatically, keep mitmproxy for those sandboxes until recreated, or require a recreate with a migration notice?
- **One proxy image or two.** Does the microsandbox sidecar stay on mitmproxy (two images, two flow sources) for the first build, or move to iron-proxy in the same feature? SOCKS5 without authentication argues for staying (`04_improvements.md`).
- **Version pinning and updates.** Which tag is pinned (Docker Hub tag format unverified), and what re-verification gates an upgrade of a 0.x dependency that releases every few days?

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A tool in a bounded sandbox fails with "Network is unreachable" or "No route to host" | The sandbox sits on an internal network with no gateway, and the tool opened a raw socket (git over SSH, a database client, a hard-coded IP) instead of going through the sidecar | Expected under the boundary; use HTTPS remotes, or allow the host and route the tool through `http_proxy`/`https_proxy` |
| Every request to an allowed host returns 403 right after enabling the boundary | iron-proxy's `allowlist` is default-deny and the rendered YAML lacks the host, or `open` mode was rendered without `warn: true` | Check the generated config under `<app_data_root>/proxy/<sandbox>/` against `.mio/net-policy.json` and the renderer beside `src-tauri/src/provider/msb_policy.rs` |
| An allowed hostname resolving to a private address still reaches an internal service | iron-proxy's default `upstream_deny_cidrs` covers only metadata endpoints and loopback; RFC 1918 is excluded upstream on purpose | Render an explicit `proxy.upstream_deny_cidrs` covering private and link-local ranges in every generated config |
| HTTPS from the sandbox fails "unable to get local issuer certificate" after the switch | iron-proxy does not generate a CA; the sidecar started without the app-generated CA or `inject_ca` piped an old PEM | Generate the CA into `<app_data_root>/proxy/<sandbox>/certs/` before creating the sidecar and inject that PEM with `inject_ca` (`src-tauri/src/proxy/mod.rs`) |
| A tool ignoring proxy variables times out instead of reaching an allowed host on a non-standard port | DNS routing sends every name to the sidecar, which listens only on 80 and 443 for routed traffic | Route the tool through the tunnel listener with `http_proxy`/`https_proxy`, or accept the port as unsupported |
| The real secret is sent upstream as the literal placeholder | The request went to a host outside the binding's `rules`, or the placeholder sat in a position (`query`, `body`) the binding did not enable | Add the host to the `SecretBinding` in `.mio/net-policy.json` or enable the matching location, then reload |
| A rotated secret value is not used | `file` sources are re-read only on boot, reload or TTL expiry, and an `env` source never refreshes | Write the value file atomically under the sidecar state directory and call `POST /v1/reload`; never render `env` sources |
| The sandbox can rebuild the proxy's pipeline | The management listener was bound to `0.0.0.0` instead of the sidecar's loopback | Bind `management.listen` to `127.0.0.1` and reach it only through `podman exec` |
| `TrafficPanel.svelte` shows empty size columns for Podman rows | The iron-proxy audit record carries no request or response byte counts | Leave `reqBytes`/`respBytes` empty in the `FlowEntry` translation (`src-tauri/src/proxy/flowlog.rs`), or drop the columns for Podman |
| The audit parser misses every row | The audit fields are nested under an `audit` group in the source, while the README example shows them flat | Parse against a real captured line from spike 3, not the README example |
| A microsandbox agent chained to an iron-proxy SOCKS5 listener can be used by any local process | iron-proxy's SOCKS5 supports no authentication, while microsandbox's chain publishes on host loopback | Keep microsandbox on the authenticated mitmproxy sidecar until iron-proxy offers SOCKS5 authentication |
| A container on `mio-proxy-<sandbox>` fails with "aardvark-dns failed to start" | The internal network was created with DNS enabled, and the podman machine runs no `systemd --user` session | Create the internal network with both `--internal` and `--disable-dns` in `MachineProvider::create_network` |
