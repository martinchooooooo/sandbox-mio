---
type: dev-plan
title: "Network Proxy: Build Plan"
feature: network-proxy
---
# Network Proxy: Build Plan

This document is the build plan for the per-sandbox mitmproxy sidecar scoped in
`docs/features/network-proxy/01_ideas.md`, and supersedes it wherever the two disagree. Nothing is
implemented yet — every phase below is `Remaining`. The gating spike `01_ideas.md` demanded was run
on 2026-08-17 against `podman-machine-default` (podman 5.8.2, WSL) and **rejected that document's
reachability design**: Podman refuses to combine `pasta` with a second network, and `aardvark-dns`
cannot start inside this project's podman machine at all. The corrected model — one private
DNS-disabled bridge per proxy-enabled sandbox, replacing `pasta` rather than joining it, with the
proxy pinned to a static IP — was then validated end to end, including TLS interception and addon
header rewriting. Spike Results below records what ran.

## Spike Results

Every finding was produced by throwaway `mio-spike-*` containers and networks on
`podman-machine-default`, all removed afterwards. The mitmproxy image pulled during the spike was
left on the machine.

- **`pasta` plus a named network is rejected outright.** `podman create --network pasta --network
  <bridge>` fails with `cannot set multiple networks without bridge network mode, selected mode
  pasta`. The "second `--network` attachment alongside the existing `pasta` one" in `01_ideas.md`'s
  Proposed Model cannot be built. Do not reintroduce it.
- **`aardvark-dns` cannot start on the podman machine.** Any container on a DNS-enabled network
  fails to start with `aardvark-dns failed to start: Failed to connect to user scope bus via local
  transport`. The machine's WSL distro runs no `systemd --user` session for the `user` account
  (`/run/user/1000/bus` is absent, `systemctl --user is-system-running` fails the same way), and
  Podman launches `aardvark-dns` through the user bus. Container-name resolution between siblings is
  therefore unavailable, and every custom network this feature creates must pass `--disable-dns`.
- **A DNS-disabled bridge with static IPs works.** Two containers on `podman network create
  --disable-dns --subnet <cidr> <name>`, one pinned with `--network <name>:ip=<addr>`, reached each
  other's listeners with no port published anywhere.
- **The bridge is unreachable from outside itself.** The podman machine's own root netns gets `No
  route to host` to the bridge subnet, and a container on `pasta` cannot reach it either — the
  rootless bridge lives in the rootless network namespace. That is a stronger isolation property
  than the `0.0.0.0`-publish option `01_ideas.md` rejected, not merely an equal one.
- **Outbound internet works from a bridge-only container** (HTTPS 200 to a public host), so dropping
  `pasta` costs the sandbox nothing on egress.
- **`host.containers.internal` still resolves and connects from a bridge-only container** (HTTP 200
  against a `0.0.0.0` listener inside the machine), so `wire_sandbox_to_inference`
  (`src-tauri/src/sync/mod.rs`, `docs/features/local-llm/02_dev.md` Phase G) survives the switch off
  `pasta`.
- **`docker.io/mitmproxy/mitmproxy` runs unmodified.** `mitmdump --listen-host 0.0.0.0
  --listen-port 8080` on the bridge proxied plain HTTP from the sandbox container immediately.
- **TLS interception works after CA injection.** HTTPS through the proxy failed with `unable to get
  local issuer certificate` before the CA was installed and returned 200 after
  `mitmproxy-ca-cert.pem` was piped into the sandbox with `podman exec -i` and `update-ca-certificates`
  was run — the same transport `credentials::tar_inject` (`src-tauri/src/credentials/mod.rs`) uses.
- **A Windows host directory bind-mounts as mitmproxy's confdir.** With a host path mounted at
  `/home/mitmproxy/.mitmproxy`, the container (which runs as uid 1000, not root) generated the whole
  CA set onto the host, readable by the app directly — no `podman exec` round trip needed to fetch
  the CA.
- **A mounted addon script loads and rewrites requests.** `mitmdump -s /addons/addon.py` with the
  script bind-mounted from a Windows path injected a header into an intercepted HTTPS request,
  confirmed by an echo endpoint. mitmdump logs one line per flow to stdout, including the client's
  bridge IP.
- **Each proxy instance's CA is distinct.** A sandbox trusting one proxy's CA rejected a second
  proxy's certificate, which is the per-sandbox CA isolation `01_ideas.md` chose, observed rather
  than assumed.

## The Concrete Model

A proxy-enabled sandbox owns three extra resources, all named from the sandbox name and all created
before the sandbox container itself: a Podman network `mio-proxy-<sandbox>`, a container
`<sandbox>-proxy` running `docker.io/mitmproxy/mitmproxy`, and a host state directory
`<app_data_root>/proxy/<sandbox>/`.

The network is created with `--disable-dns` and an explicit `--subnet` the app picks, because
`aardvark-dns` is unavailable and because the proxy's address must be known before either container
exists. Candidate subnets are `10.90.<index>.0/24` for `index` in `0..=255`; `podman network create`
rejects an overlap, so the allocator walks to the next index on that failure. The gateway takes
`.1`, the proxy container is pinned to `.2` with `--network mio-proxy-<sandbox>:ip=10.90.<index>.2`,
and the sandbox takes any other address from the same subnet.

The sandbox container joins `mio-proxy-<sandbox>` **instead of** `pasta`, not alongside it. That
costs nothing: an exec-tunnelled sandbox reaches nothing over its own network stack — the terminal,
agent-web and expose-port paths all run over the `podman exec`-tunnelled `mio-sshd` that
`ensure_sandbox_alias` (`src-tauri/src/backends/mod.rs`) writes as a `ProxyCommand`, and
`PublishModel::ExecTunnelled` publishes no ports at all. Proxy enablement is therefore gated on
`Sandbox::supports_exec_transport` (`src-tauri/src/backends/types.rs`); a `PublishModel::PublishRelay`
sandbox, whose zellij reachability does depend on pasta's `--host-lo-to-ns-lo`, is refused.

Proxy configuration reaches the sandbox as container-wide environment, set by `--env` at create
time so every `podman exec` session sees it: `http_proxy`/`https_proxy`/`HTTP_PROXY`/`HTTPS_PROXY`
pointed at `http://10.90.<index>.2:8080`, and `no_proxy`/`NO_PROXY` covering `localhost`,
`127.0.0.1`, `::1` and `host.containers.internal` — the last one load-bearing, since routing the
local-LLM base URL through the proxy would break inference for no benefit.

The proxy container mounts three things from `<app_data_root>/proxy/<sandbox>/`: `certs/` at
`/home/mitmproxy/.mitmproxy` (mitmdump's own confdir, where it generates the per-sandbox CA),
`addon.py` read-only at `/addons/addon.py`, and `rules.json` read-only at `/addons/rules.json`. The
sandbox container mounts none of them and has no path to reach them, which is what keeps injected
secrets out of the sandbox's own filesystem.

After the sandbox container is created, the app reads `certs/mitmproxy-ca-cert.pem` off the host and
pipes it into the sandbox at `/usr/local/share/ca-certificates/mio-proxy.crt`, then runs
`update-ca-certificates`. Runtimes that ignore the OS trust store are covered by additional `--env`
pairs pointing at the same in-container path.

## Resolved Decisions

1. **Pasta-plus-bridge coexistence** (`01_ideas.md` Open Question 1) — resolved negatively by the
   spike and the design changed: Podman rejects the combination, so a proxy-enabled sandbox drops
   `pasta` and joins only its private bridge, which the spike proved keeps egress,
   `host.containers.internal`, and the exec transport working.
2. **Container-name reachability** (implied throughout `01_ideas.md`'s Proposed Model) — impossible
   on this podman machine because `aardvark-dns` cannot start; replaced by a `--disable-dns` network
   with a static proxy IP written into the sandbox's proxy environment variables at create time.
3. **Opt-in default** (Open Question 2) — opt-in per sandbox, chosen in the create dialog and passed
   as a `proxy: bool` argument to `create_sandbox` (`src-tauri/src/commands/sandbox.rs`); default
   off. No app-global default setting in v1 (deferred in `04_improvements.md`). Enablement is never
   persisted: it is derived from the existence of the `<sandbox>-proxy` container, the same way
   deployment state is derived from a Mutagen session rather than stored.
4. **Where secret-injection rules live** (Open Question 3) — split in two. Rule *definitions* live
   in the workspace at `.mio/proxy-rules.json` (mirroring `.mio/ports.json`, read and written
   through `src-tauri/src/mio_files.rs`) and carry host, path prefix, header name and a secret *id*
   only — never a secret value, because the workspace is bind-mounted into the sandbox. The
   *materialised* `rules.json` with real values is written only to
   `<app_data_root>/proxy/<sandbox>/rules.json` and mounted only into the proxy container.
5. **Where secret values come from** (new; narrows Open Question 3) — v1 stores values in
   `<app_data_root>/proxy/secrets.json`, written by a `set_proxy_secret` command and never read back
   over IPC. Sourcing a value from a `credentials/` bundle is deferred (`04_improvements.md`): those
   bundles are whole-home tar archives, and extracting one bearer token from one is agent-specific
   work with no general shape.
6. **Remote deploy scope** (Open Question 4) — out of scope for v1, and refused rather than silently
   degraded: `deploy` of a proxy-enabled sandbox fails with a new
   `BackendError::ProxyDeployUnsupported`. Deploying without a proxy would move a sandbox the user
   enabled inspection for onto an uninspected network, which is a worse outcome than an error.
7. **Non-HTTP(S) traffic** (Open Question 5) — accepted and documented, not surfaced per-connection.
   Anything ignoring `http_proxy`/`https_proxy` leaves over the bridge's own NAT uninspected. The
   proxy panel states the limitation once; per-connection reporting is deferred
   (`04_improvements.md`).
8. **Per-runtime CA trust** (Open Question 6) — configured, not left to the OS store alone.
   `NODE_EXTRA_CA_CERTS`, `REQUESTS_CA_BUNDLE`, `PIP_CERT`, `SSL_CERT_FILE`, `CURL_CA_BUNDLE` and
   `GIT_SSL_CAINFO` are all set by `--env` to `/usr/local/share/ca-certificates/mio-proxy.crt`
   alongside `update-ca-certificates`, because `--env` covers every process tree in the container
   regardless of which runtime mise later installs.
9. **Microsandbox provider support** (Open Question 7) — unsupported and refused. `MsbProvider`
   (`src-tauri/src/provider/msb.rs`) returns `ProviderError::CommandFailed` from the new network
   methods, and the create dialog disables the proxy toggle when the selected provider is
   microsandbox.
10. **Image pinning** — the proxy image is a pinned constant `MITMPROXY_IMAGE` in
    `src-tauri/src/proxy/mod.rs` (a version tag, never `latest`), overridable for tests by
    `MIO_PROXY_IMAGE`. Pulled by an explicit `ensure_proxy_image` step reported through
    `activity::TaskRegistry`, not silently by `--pull=missing`, because the first pull is ~200 MB
    and would otherwise look like a hung create.
11. **Addon reload** — the addon reads `rules.json` once at load, and changing rules restarts the
    proxy container (`podman restart <sandbox>-proxy`). mitmproxy has no built-in watch on a mounted
    data file, and a restart costs a second.
12. **Where the CA is read from** — the host state directory, not the container. The spike showed the
    bind-mounted confdir puts the whole CA set on the host, so the app reads
    `certs/mitmproxy-ca-cert.pem` with ordinary file IO and only needs a container round trip for the
    injection half.

## Implemented: Phase A — provider seam (env, network attachment, network lifecycle)

- `src-tauri/src/provider/types.rs`: add `env: Vec<(String, String)>` and `network:
  Option<InstanceNetwork>` to `InstanceSpec`, and declare `InstanceNetwork { name: String, ip:
  Option<String> }`. Add `create_network(&self, name: &str, subnet: &str)` and `remove_network(&self,
  name: &str)` to the `MachineProvider` trait. Document on `InstanceNetwork` that a `Some` value
  *replaces* the provider's default network mode, citing the spike finding that Podman rejects
  `pasta` plus a named network.
- **Correction found during implementation, not anticipated by this document**: `InstanceSpec`/
  `create_instance` are shaped for a sandbox (exactly one `workspace_source` bind mount at a fixed
  `/root/work`, always launches `/bin/sh`) and cannot express the mitmproxy sidecar Phase D needs
  (three distinct bind mounts at caller-chosen destinations, a pinned static IP, a `mitmdump`
  launch command). Added `SidecarMount { host_path, dest, read_only }`, `SidecarSpec { name, image,
  network, mounts, command, labels }`, and `MachineProvider::create_sidecar(&self, spec:
  &SidecarSpec) -> Result<String, ProviderError>` — a second, purpose-built create path alongside
  `create_instance`, never used for a sandbox. `PodmanProvider` implements it with its own argv
  builder; `MsbProvider::create_sidecar` returns `ProviderError::CommandFailed` unconditionally, the
  same as its `create_network`/`remove_network`.
- `src-tauri/src/provider/mod.rs`: in `PodmanProvider::create_instance`, emit one `--env k=v` per
  `spec.env` entry, and emit `--network <name>[:ip=<addr>]` from `spec.network` in place of the
  `publish_model` branch's `--network pasta` when it is `Some`. Implement `create_network` as
  `podman network create --disable-dns --subnet <subnet> <name>` and `remove_network` as `podman
  network rm --force <name>`, both through `PodmanProvider::run`.
- `src-tauri/src/provider/msb.rs`: reject a `Some` `network` and a non-empty `env` from
  `create_instance` the way a non-`ExecTunnelled` `publish_model` is already rejected; return
  `ProviderError::CommandFailed` from both new network methods.
- `src-tauri/src/mockctl/mod.rs`: add a `"network"` depth-2 arm to `derive_podman_key`, and add
  `podman.network-create`, `podman.network-rm` and `podman.network-inspect` to `KNOWN_CALLS`. No
  tracked state is needed — the default zero-exit output satisfies the callers.
- Existing `InstanceSpec` construction sites (`CliBackend::create` and `CliBackend::deploy` in
  `src-tauri/src/backends/mod.rs`, the provider tests in `src-tauri/src/provider/msb.rs`) gain the
  two new fields set empty/`None`.

## Remaining: Phase B — the `proxy` subsystem and its lifecycle

`src-tauri/src/proxy/` is a module of free functions taking `&Arc<dyn MachineProvider>` and
`&AppHandle`, modelled on `src-tauri/src/credentials/mod.rs` rather than on the trait+mock
subsystems: it owns no long-lived state, and every external call it makes already crosses the
`ProcessRunner` seam through the provider, so the `light` tier is covered without a new trait.

- `src-tauri/src/proxy/types.rs`: `ProxyError` (converting into `BackendError`/`AppError`),
  `ProxyAttachment { network: String, env: Vec<(String, String)> }` — what `SandboxBackend::create`
  needs to build the sandbox's spec — and `ProxyStatus { running: bool, proxy_ip: String,
  network: String }` for the frontend.
- `src-tauri/src/proxy/mod.rs`: `MITMPROXY_IMAGE`, `PROXY_PORT` (8080), `proxy_container_name`,
  `proxy_network_name`, `state_dir` (`crate::paths::app_data_root(app).join("proxy").join(name)` —
  the `MIO_APP_DATA_DIR` override is mandatory here, see Cross-Cutting Concerns),
  `ensure_proxy_image`, `allocate_network` (the `10.90.<index>.0/24` walk), `create_proxy` (network,
  container, start, wait for CA), `wait_for_ca`, `read_ca_pem`, `inject_ca`, `remove_proxy`,
  `sweep`, and `status`.
- `src-tauri/src/backends/types.rs`: widen `SandboxBackend::create` with a `proxy:
  Option<&ProxyAttachment>` parameter; add `BackendError::ProxyDeployUnsupported { name: String }`
  and its arms in `kind()` and the hand-written `From<BackendError> for AppError`
  (`src-tauri/src/error.rs`), mirrored into `AppErrorKind` (`src/lib/errors.ts`).
- `src-tauri/src/backends/mod.rs`: `CliBackend::create` passes the attachment's `network` and `env`
  into the `InstanceSpec` it builds, and refuses a proxy attachment when `exec_capable` is false.
  `CliBackend::deploy` returns `ProxyDeployUnsupported` when `<name>-proxy` exists.
- `src-tauri/src/credentials/mod.rs`: `ensure_auth_container` passes `None` — an auth container is a
  throwaway and never proxied.
- `src-tauri/src/services/sandbox.rs`: `create` takes a `proxy: bool` and, when set, runs
  `ensure_proxy_image` → `create_proxy` → `store.create(.., Some(&attachment))` → `inject_ca`
  before `runtime.init`, so mise's own downloads already trust the proxy. A failure after the proxy
  container exists rolls it back through `remove_proxy`, matching the existing rollback discipline
  around the microsandbox sync failure. `destroy` calls `remove_proxy` after `store.destroy`;
  `undeploy` does not touch the proxy, which is local-only.
- `src-tauri/src/bootstrap.rs`: call `proxy::sweep(&live)` beside the existing
  `NetworkManager::sweep` in the `mock_backend_enabled()`-gated startup block, removing
  `<name>-proxy` containers and `mio-proxy-<name>` networks whose sandbox no longer exists.
- `src-tauri/src/commands/sandbox.rs`: `create_sandbox` gains a `proxy: Option<bool>` argument
  threaded into `services::sandbox::create`.

## Remaining: Phase C — CA trust and proxy environment

- `src-tauri/src/proxy/mod.rs`: `proxy_env` builds the full `--env` list — the six proxy variables,
  the two `no_proxy` spellings, and the six per-runtime CA variables from Resolved Decision 8 —
  from the allocated proxy IP.
- `src-tauri/src/proxy/mod.rs`: `inject_ca` pipes the host-read PEM into the sandbox via
  `MachineProvider::exec_piped` running `sh -c "cat > /usr/local/share/ca-certificates/mio-proxy.crt
  && update-ca-certificates"`, the transport `credentials::tar_inject` established. It starts the
  container first, as `credentials::write_file_via_stdin` already does.
- `CONTAINER_CA_PATH` in `src-tauri/src/proxy/mod.rs` is referenced by both `inject_ca` and
  `proxy_env`; the two must name the same path and each carries a comment pointing at the other.

## Remaining: Phase D — addon script and secret injection

- `src-tauri/resources/mitm_addon.py`: the addon, embedded with `include_str!` in
  `src-tauri/src/proxy/mod.rs` and written to `<state>/addon.py` at every `create_proxy` so an app
  upgrade refreshes it. It loads `/addons/rules.json` once at load and, per request, matches host
  and optional path prefix, then sets the configured header. Header names are emitted lowercase —
  HTTP/2 rejects uppercase names, which the spike observed mitmdump warning about.
- `src-tauri/src/proxy/rules.rs`: `ProxyRule { host, path_prefix, header, secret_id }`, reading the
  workspace's `.mio/proxy-rules.json` through `crate::mio_files`, and `materialise` resolving each
  `secret_id` against `<app_data_root>/proxy/secrets.json` into the proxy-side `rules.json`.
- `src-tauri/src/commands/proxy.rs`: `proxy_status`, `list_proxy_rules`, `set_proxy_rules`,
  `set_proxy_secret`, `delete_proxy_secret`, `restart_proxy`. `set_proxy_rules` re-materialises and
  restarts the container (Resolved Decision 11). No command ever returns a secret value.
- `src-tauri/src/commands/mod.rs`: register every `proxy::*` command in `tauri::generate_handler![]`.

## Remaining: Phase E — frontend

- `src/routes/projects/CreateSandboxItem.svelte`: a proxy toggle (the existing `switch` component),
  disabled when the selected provider is microsandbox, passed as `proxy` to `create_sandbox`.
- `src/lib/workspace/` gains a proxy panel component beside `PortsManager.svelte`'s pattern under
  `src/routes/projects/`: proxy status from `proxy_status`, the rule list, secret entry, and a
  standing note that non-HTTP(S) traffic is not inspected (Resolved Decision 7).
- `src/lib/errors.ts`: add `"ProxyDeployUnsupported"` to `AppErrorKind`.
- `messages/en.json`, `messages/de.json`, `messages/ru.json`: keys for the toggle, the panel, the
  uninspected-traffic note, and the deploy refusal. Per the `feature-implement` skill, the i18n pass
  runs after the user has approved the working feature, not inside Phase E.

## Cross-Cutting Concerns

**Parallel-worktree state.** Three of this feature's resources are host-global and must be treated
as the `feature-dev` skill requires:

- `<app_data_root>/proxy/` is derived from `crate::paths::app_data_root`
  (`src-tauri/src/paths.rs`), so `MIO_APP_DATA_DIR` relocates it per worktree. It must never be
  assembled from the bundle identifier or `%APPDATA%`.
- The `mio-proxy-<sandbox>` network and the `<sandbox>-proxy` container are host-global on the
  podman machine, keyed off a sandbox name that is already run-scoped in e2e. The `10.90.<index>`
  subnet index is genuinely host-global and racy: the allocator's retry-on-overlap walk is what
  makes two concurrent creates safe, and it is a correctness requirement, not an optimisation.
  Specs exercising a real proxy therefore belong in `standard`, which is host-locked to one run at a
  time (`e2e/AGENTS.md`).
- `MIO_PROXY_IMAGE` is a new environment variable. Add it to the root `AGENTS.md` environment list
  when Phase B lands; it is read by the Rust side, so it is not harness-set.

No new host port is introduced — the proxy's 8080 is inside the bridge and is published nowhere, so
`allocatePortPair()` (`e2e/lib/run-identity.ts`) is not involved. No new bundled binary is added, so
`build.rs` needs no asset group. The startup sweep deletes only resources whose sandbox is absent
from `SandboxBackend::list`, so it cannot destroy another worktree's state.

**State derivation.** Nothing about proxy enablement is persisted. `proxy_status` derives it from the
`<sandbox>-proxy` container's presence, and the network and IP are recoverable from `podman network
inspect mio-proxy-<sandbox>`. The consequence is deliberate: a sandbox cannot be switched between
proxied and unproxied after creation, because its proxy environment variables and network attachment
are baked into `podman create`. Toggling means recreating the sandbox.

**Teardown ordering.** `remove_proxy` removes the container before the network — Podman refuses to
remove a network still holding an attached container — and deletes
`<app_data_root>/proxy/<sandbox>/` last. Deleting the state directory is not optional cleanliness:
it holds the CA private key, and leaving it behind leaves a key that was trusted by a sandbox whose
lifetime has ended.

**Secret exposure boundary.** The materialised `rules.json` and `secrets.json` live only under the
app data root and are mounted only into the proxy container. Any change that bind-mounts the proxy
state directory into the sandbox, or writes a resolved secret into `.mio/proxy-rules.json`, defeats
the entire feature — the workspace is bind-mounted into the sandbox at `/root/work`.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `podman create` fails with `cannot set multiple networks without bridge network mode, selected mode pasta` | A proxy-enabled `InstanceSpec` kept `--network pasta` and added the bridge, which Podman rejects outright (Spike Results) | Emit `--network mio-proxy-<sandbox>` *instead of* `--network pasta` in `PodmanProvider::create_instance` (`src-tauri/src/provider/mod.rs`); the exec transport needs no pasta |
| A container fails to start with `aardvark-dns failed to start: Failed to connect to user scope bus via local transport` | The network was created with DNS enabled, but the podman machine's WSL distro runs no `systemd --user` session for the `user` account, so Podman cannot launch `aardvark-dns` | Create every network with `--disable-dns` in `MachineProvider::create_network` and address the proxy by its static IP, never by container name |
| `podman network create` fails with a subnet-overlap error during a concurrent create | The `10.90.<index>.0/24` index is host-global and two sandboxes were created at the same time | `allocate_network` (`src-tauri/src/proxy/mod.rs`) walks to the next index on an overlap failure; do not replace the walk with a single fixed subnet |
| A sandbox's HTTPS requests fail with `unable to get local issuer certificate` after inspection is enabled | The per-sandbox CA was never injected, or was injected before `mitmdump` had written `certs/mitmproxy-ca-cert.pem` | Have `create_proxy` await `wait_for_ca` before `SandboxBackend::create`, and confirm `inject_ca` ran `update-ca-certificates` inside the sandbox |
| A Node.js or Python tool still fails TLS verification while curl and git succeed | Node.js and `certifi`-based Python ignore the OS trust store that `update-ca-certificates` updates | The `--env` list from `proxy_env` must carry `NODE_EXTRA_CA_CERTS`, `REQUESTS_CA_BUNDLE`, `PIP_CERT` and `SSL_CERT_FILE` pointing at `CONTAINER_CA_PATH` (Resolved Decision 8) |
| Local LLM inference breaks in a proxy-enabled sandbox | `OPENAI_BASE_URL` points at `host.containers.internal`, and without a `no_proxy` entry the request is routed through mitmproxy instead | Keep `host.containers.internal` in both `no_proxy` and `NO_PROXY` in `proxy_env` (`src-tauri/src/proxy/mod.rs`) |
| A header the addon injects is missing from the upstream request over HTTP/2 | mitmproxy rejects uppercase header names on HTTP/2 and lowercases them with a warning | Emit header names lowercase in `src-tauri/resources/mitm_addon.py` |
| Editing `.mio/proxy-rules.json` has no effect on requests | The addon reads `/addons/rules.json` once at load, and the mounted file is the materialised copy under `<app_data_root>/proxy/<sandbox>/`, not the workspace file | Re-materialise and restart the proxy container through `set_proxy_rules` (`src-tauri/src/commands/proxy.rs`) rather than editing the workspace file by hand |
| A real secret shows up inside the sandbox despite proxy injection | The value was written into `.mio/proxy-rules.json`, which lives in the workspace bind-mounted at `/root/work` | Keep secret *ids* in `.mio/proxy-rules.json` and values in `<app_data_root>/proxy/secrets.json`, which is mounted into the proxy container only (Resolved Decision 4) |
| `podman network rm` fails with the network still in use during teardown | `remove_proxy` removed the network before the `<sandbox>-proxy` container attached to it | Remove the container first, then the network, then the state directory — the order `remove_proxy` (`src-tauri/src/proxy/mod.rs`) documents |
| `mock_configure` rejects a `podman.network-create` key in a light spec | `KNOWN_CALLS` in `src-tauri/src/mockctl/mod.rs` lists only registered keys, and `derive_podman_key` needs a depth-2 arm for `network` or the key derives as `podman.network` | Add the `"network"` arm to `derive_podman_key` and the three `podman.network-*` keys to `KNOWN_CALLS` (Phase A) |
| Deploying a proxy-enabled sandbox fails with `ProxyDeployUnsupported` | Remote proxy creation is deliberately out of scope for v1 (Resolved Decision 6), and deploying without a proxy would silently drop inspection | Destroy and recreate the sandbox without the proxy toggle if it must be deployed; remote support is tracked in `docs/features/network-proxy/04_improvements.md` |
