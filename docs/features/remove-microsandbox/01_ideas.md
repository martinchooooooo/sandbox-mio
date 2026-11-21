---
type: ideas
title: "Remove microsandbox Ideas"
feature: remove-microsandbox
---
# Remove microsandbox Ideas

This document is the ideation for deleting the microsandbox (`msb`) provider and making Podman the only provider, while keeping the abstractions that make adding a provider later a small task. The user decided the removal; this document scopes it, lists what goes and what stays, and records the tradeoffs. Nothing here is built. The code today carries two `ProviderId` variants, a second `MachineProvider` implementation (`src-tauri/src/provider/msb.rs`), a second machine manager (`src-tauri/src/machine/msb.rs`), a mitmproxy SOCKS5 sidecar path in `src-tauri/src/proxy/mod.rs`, a bundled `msb` runtime fetched by `build.rs`, and msb branches across `backends`, `services`, `process`, `mockctl`, the frontend, the locale files and the e2e harness. The plans being retired are `docs/features/microsandbox-provider/` and `docs/features/microsandbox-network-policy/`; neither is deleted. No spike was run: the removal needs no new technology, and the iron-proxy approximations in Technology Choices are deferred to `04_improvements.md`.

## Current State

- **msb was never Podman-free.** `MsbProvider::ensure_base_image` builds the base image with Podman and exports it, failing with `ProviderError::PodmanUnavailable` when no Podman machine is reachable (`src-tauri/src/provider/msb.rs`, `src-tauri/src/provider/types.rs`). Every msb user already needs Podman, so the second engine added maintenance and no independence.
- **The seams already exist.** `MachineProvider`, `InstanceSpec`, `SidecarSpec`, `ExecOpts`, `InstanceStats`, `EnginePool`, `ProviderCapabilities`, `ProviderId`, `MachineRef` and `ProviderRegistry` live in `src-tauri/src/provider/types.rs` and `src-tauri/src/provider/mod.rs`; `ProcessRunner` and its scripted mocks live in `src-tauri/src/process/types.rs` and `src-tauri/src/mockctl/`. Podman's sidecar boundary (`docs/features/podman-egress-boundary/02_dev.md`) already runs on them.
- **Podman's recreate already works at the provider level.** `PodmanProvider::recreate_instance` removes and re-creates the container, and the bind-mounted workspace survives. Only `CliBackend::recreate` in `src-tauri/src/backends/mod.rs` and the msb-only `recreate_sandbox_network` service in `src-tauri/src/services/sandbox.rs` refuse Podman.
- **`SyncStrategy::Volume` is already dead code** (`src-tauri/src/backends/types.rs`, marked `allow(dead_code)`); the msb workspace-volume sync was routed around it.
- **The mitmproxy flow-log addon is msb-only.** `src-tauri/resources/mitm_flowlog_addon.py` is written into a sidecar only by `create_msb_proxy`. Podman sidecars run iron-proxy, whose audit lines `src-tauri/src/proxy/audit.rs` translates into the shared flow record read by `src-tauri/src/proxy/flowlog.rs`. A Podman sidecar created before the boundary may still run mitmproxy (`ProxyEngine::Mitmproxy` in `src-tauri/src/proxy/types.rs`); the app only reads such a sidecar's existing state.
- **Podman already refuses what msb enforced.** `net_policy::validate` applies the sidecar dialect whenever `sidecar_egress` is true, refusing ports, groups, host denies, allow CIDRs and UDP (`src-tauri/src/net_policy.rs`; `docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 6). The only consumer of `InstanceSpec.egress` is msb; `PodmanProvider::create_instance` refuses `Some`.

## Inventory of What Goes

| Area | Removed |
|---|---|
| Provider | `src-tauri/src/provider/msb.rs`, `src-tauri/src/provider/msb_policy.rs`, the SOCKS5 readiness probe `src-tauri/src/provider/probe.rs`, `ProviderId::Microsandbox`, `ProviderError::PodmanUnavailable`, `StartEnvSource` with `NoStartEnv` and `src-tauri/src/services/start_env.rs`, `SidecarReadiness`, `ChainedProxy`, `SecretViolation`, msb fixtures under `src-tauri/src/provider/fixtures/` |
| Machine | `src-tauri/src/machine/msb.rs`, the msb and hypervisor-platform setup steps, the msb arm of the machine-manager factory in `src-tauri/src/machine/mod.rs` |
| Proxy | `create_msb_proxy`, `msb_attachment`, `ProxyAttachment::MsbChain`, `MIO_PROXY_IMAGE` and `mitmproxy_image`, the SOCKS password handling, `src-tauri/resources/mitm_flowlog_addon.py`, the `sha1` dependency in `src-tauri/Cargo.toml` if nothing else uses it |
| Process | `SystemMsbRunner`, `msb_command`, `msb_executable`, msb timeouts, `stage_msb` and its pruning, `MSB_METRICS_ARGV`, `set_msb_resource_dir` (`src-tauri/src/process/`, `src-tauri/src/bootstrap.rs`) |
| Mock | `ScriptedMsbRunner`, the `msb.*` entries of `KNOWN_CALLS` and their tracked-instance logic (`src-tauri/src/mockctl/`) |
| Backends and services | `ensure_msb_sandbox_alias`, `msb_sync_beta`, `msb_sync_session`, `msb_proxy_command`, the msb arm of the alias router, the msb branches in create and teardown, `recreate_sandbox_network`, the `net_policy_violations` command (`src-tauri/src/backends/mod.rs`, `src-tauri/src/services/`, `src-tauri/src/commands/net_policy.rs`) |
| Settings | the `default_provider` setting, `MIO_MACHINE_PROVIDER` (`src-tauri/src/settings/`) |
| Build | `fetch_msb`, `stage_msb`, the `msb` entries of the staged-binary table in `build.rs`, the `binaries/msb` resource in `src-tauri/tauri.conf.json` |
| Frontend | provider picker in `src/routes/settings/+page.svelte` and `src/routes/projects/CreateSandboxItem.svelte`, msb branches in `src/routes/projects/ProxyPanel.svelte` and `TrafficPanel.svelte`, the msb machine steps in `src/lib/machine/MachineSteps.svelte`, the ports column, violation-action buttons and violation list in `src/routes/projects/NetPolicyEditor.svelte`, the `microsandbox` value in `src/lib/providers.ts` |
| Locales | msb, hypervisor, violation, ports and `proxy_chained` strings in `messages/en.json`, `de.json`, `ru.json`; `src/lib/paraglide` regenerates |
| E2E | `e2e/test/specs/light/microsandbox-*.spec.ts`, `e2e/test/specs/standard/microsandbox-egress-allowlist.spec.ts`, the msb scenario of `e2e/test/specs/standard/github-placeholder-swap.spec.ts`, `MSB_HOME`/`MSB_PATH` in `e2e/lib/run-identity.ts` and `e2e/wdio.conf.ts`, provider and egress helpers in `e2e/test/pageobjects/projects.page.ts` |

Roughly 1,800 lines sit in four whole files; the rest is msb branches in about thirty others (`git grep -n -i -E "microsandbox|\bmsb\b|Msb|mitm"` builds the full list at implementation time).

## Proposed Model

### The seams that stay

- **Provider trait and neutral types.** `MachineProvider`, `InstanceSpec`, `SidecarSpec`, `ExecOpts`, `InstanceStats`, `EnginePool` stay unchanged in shape. Engine flags remain rendered only inside `PodmanProvider`. Doc comments that name msb behaviour ("Microsandbox ignores it") are rewritten to describe the field's contract for any provider.
- **Capabilities.** `ProviderCapabilities` keeps all five flags (`src-tauri/src/provider/types.rs`), with Podman's values constant, and `src/lib/providers.ts` keeps its mirror and `capabilitiesFor` gating (`src/lib/workspace/sections.ts`, `src/routes/projects/RemoteManager.svelte`). The frontend stops fetching capabilities for a provider list of two.
- **`ProviderId` and registry.** `ProviderId` keeps one variant, `Podman`, with the `podman:<id>` `MachineRef` form. `ProviderRegistry` keeps `provider_for`, `local` and `local_ref`, now with one arm. Adding a provider means a variant, an arm and an implementation.
- **Process seam.** `ProcessRunner`, `SystemPodmanRunner`, `SystemMutagenRunner` and the scripted podman and Mutagen mocks stay; only the msb runner and its mock go.
- **Transport.** SSH-alias-over-exec and Mutagen stay for remote deploy and forwards (`src-tauri/src/ssh/`, `src-tauri/src/sync/`). The alias writer keeps one engine arm. Mutagen sessions named `workspace-<sandbox>` (msb workspace sync) stop being created.
- **`SandboxBackend::recreate`.** The trait method, `MachineProvider::recreate_instance` and Podman's existing implementation stay as the base `docs/features/proxy-toggle-management/` builds on; the backend-level Podman refusal remains until that feature lifts it.

### Network policy

- **Schema and validation stay.** `NetPolicy`, `EgressPolicy`, `EgressRule`, `EgressMode`, the sidecar dialect and capability-based `validate` stay (`src-tauri/src/net_policy.rs`, `src-tauri/src/provider/types.rs`). Existing `.mio/net-policy.json` files parse unchanged. Fields only msb could enforce (`ports`, groups, `violationAction`, `denyMessage`) stay in the schema and remain refused by the sidecar dialect.
- **msb-only editor controls go.** The ports column, violation-action buttons, violation list and violation polling leave `NetPolicyEditor.svelte`. The capability-gating mechanism stays for a future engine-enforced provider.
- **`egress_policy` capability and `InstanceSpec.egress`.** Recommended: keep both as the seam for a future engine-enforced provider; Podman reports `false` and keeps refusing `Some`. Cost: `to_egress` and the `ChainedProxy`-free `EgressPolicy` stay compiled with no live consumer.
- **Reserved variable names.** `RESERVED_ENV` in `src-tauri/src/net_policy.rs` borrows the SOCKS password variable from `msb_policy.rs` and reserves the `MSB_` prefix; both reservations go.

### Proxy sidecar

`create_proxy` collapses to the Podman path. `sidecar_image` returns iron-proxy only. `ProxyStatus.host_port` and `healthy` (msb SOCKS fields in `src-tauri/src/proxy/types.rs`, `src/lib/proxy/status.svelte.ts`) go. `ProxyEngine::Mitmproxy` stays only to label pre-boundary Podman sidecars (Open Question 5). The unused `mitm_flowlog_addon.py` is deleted; `FLOW_LOG_MAX_LINES` comments that cite it are repointed to iron-proxy's audit translation.

### Existing msb sandboxes on user machines

The app stops listing them. Recommended: no migration, no data loss path to build. The workspace folder is mirrored to the host by Mutagen, so project files survive. One-time cleanup at startup, none of which needs the `msb` binary: terminate Mutagen sessions named `workspace-*`, delete proxy state directories whose `meta.json` names provider `msb`, delete the staged msb copy under the app-data root, and drop `defaultProvider` from `settings.json` (an unknown value would otherwise fail `ProviderId` deserialization and break settings load). Running microVMs and the `~/.microsandbox` store cannot be stopped without the binary; release notes carry the manual steps (Open Question 1).

### Dev tooling

The `build dev` path runs outside `cargo run` only because `msb create` needs to break away from Windows Job Objects (`run_dev_directly` and the `win_job` breakaway grant in `build.rs`). With msb gone, the constraint is gone: `build dev` can return to `tauri dev` with Rust auto-rebuild (Open Question 6). `process/mod.rs` comments about msb handle inheritance and `run_capturing_via_files` are reduced to the Podman rationale that remains.

### Docs

`docs/architecture/providers.md`, `network.md`, `transport.md`, `overview.md`, `inference.md`, `remote.md`, `testing.md` and `index.md`, plus the root `AGENTS.md`, `src-tauri/AGENTS.md` and `e2e/AGENTS.md` where they mention msb, are rewritten Podman-only, keeping the provider seam described as the extension point. Code comments citing `docs/features/microsandbox-*` are updated or removed where the cited constraint goes away; citations stay valid because the plans are not deleted.

## What Becomes Moot in Other Plans

- `docs/features/microsandbox-provider/` and `docs/features/microsandbox-network-policy/`: become `superseded` with `superseded_by: remove-microsandbox`, including their `04_improvements.md` files. Status change and log entries happen when `remove-microsandbox` reaches `implemented` (Open Question 7).
- `docs/features/podman-egress-boundary/04_improvements.md`: "microsandbox chained to iron-proxy" is moot; "Injected bindings" and "Require the placeholder on bound hosts" lose their blocker (msb could only swap placeholders; Resolved Decision 8). The decision to keep one binding format with two enforcers no longer applies.
- `docs/features/proxy-toggle-management/`: its `04_improvements.md` entry "Microsandbox provider support" is moot; its status note's claim that recreate exists "only for microsandbox" must be updated at implementation. Its Phase B and C still need `feature-dev` refresh and now own Podman's backend-level recreate.
- `docs/features/local-llm-reachability/04_improvements.md`: both stopped-msb-sandbox entries (existing forwards, port poller boots the VM) are moot. The `inference_reach` comment in `src-tauri/src/services/inference_reach.rs` is trimmed.
- `docs/features/sandbox-resource-monitor/` (in-progress): `msb metrics` parsing, `MSB_METRICS_ARGV`, the `msb.metrics` known call and the msb fixtures are removed; its `04_improvements.md` items on `msb create --memory` and msb disk volume use are moot. The monitor's trait seams (`instance_stats`, `engine_pool`) stay.
- `docs/features/host-tool-ssh-access/`, `docs/features/local-llm-reachability/02_dev.md`: remain valid for Podman; their msb sentences become history.
- Historical log entries mentioning skipped microsandbox checks stay as written.

## Technology Choices with Tradeoffs

### Remove the provider entirely

Chosen: delete the msb code, binary and docs surface. Alternatives: (a) keep msb behind a Cargo feature or hidden setting, rejected because every msb call site would still have to compile, be mocked and be tested, and the maintenance cost is the reason for the removal; (b) leave msb compiled but unmaintained, rejected because untested branches in `services` and `backends` rot silently and still force every change to consider a second engine.

### Keep the provider abstractions instead of collapsing onto concrete Podman

Chosen: keep the trait, capabilities, `ProviderId`, registry and process seam. Alternative: inline `PodmanProvider` everywhere and delete the registry. Rejected because the user requires adding a provider to stay a small task, the seams are already built, and the Podman code already sits behind them. Cost: a single-variant enum and a registry with one arm exercised by one implementation, so the seam is not proven by a second implementer until one is added.

### Keep the net-policy schema and validation, delete only the msb UI

Chosen: schema and capability-based validation stay (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 6). Alternative: shrink the schema to the sidecar dialect. Rejected because existing `.mio/net-policy.json` files may carry the msb fields, shrinking breaks their parsing, and a future engine would re-add the same fields.

### Accept the egress-rule loss rather than emulate it

Chosen: lose per-destination ports, address groups, hostname denies, UDP and raw TCP rules, violation actions and the custom deny message. iron-proxy (pinned 0.56.0 in `src-tauri/src/proxy/iron.rs`; 0.59.0 was latest on 2026-11-19, not adopted here) cannot do per-destination ports, hostname denies, UDP or raw TCP, or custom deny messages. Alternatives: (a) approximate groups through `upstream_deny_cidrs` and global ports through nftables on the sidecar, rejected for this feature as new enforcement code with its own verification, tracked in `04_improvements.md`; (b) a different sidecar proxy, rejected as out of scope. Podman sandboxes already could not use these rules, so no working Podman behaviour regresses.

### Lost isolation and workspace properties

- libkrun microVM isolation (a separate guest kernel per sandbox) is lost; Podman containers share the one WSL VM's kernel. The Podman egress boundary and the absence of published ports remain the isolation story.
- Recreate-keeping-workspace as an msb-only capability is not lost in substance: Podman's bind mount already survives a recreate.

### Remove the default-provider setting

Chosen: remove `default_provider`, the `MIO_MACHINE_PROVIDER` override and both provider pickers, since a one-option picker is dead UI. Alternative: keep the setting with one option so a future provider needs no setting migration. Rejected: adding a setting later is cheap, and keeping a field whose old value (`microsandbox`) must be tolerated forever is a standing deserialization hazard.

### Removing msb also removes host-global state

msb stored state in `~/.microsandbox` (host-global unless `MSB_HOME` is relocated, which the harness did per worktree) and swept sidecars per app-data root. Removal deletes that cost with no replacement; the remaining host-global dependency is the one Podman machine, already handled by `e2e/AGENTS.md`.

## Validation

No spike or manual run was performed. The claims in Current State come from reading the code on 2026-11-21: `PodmanProvider::recreate_instance`, `SyncStrategy::Volume`, the single writer of the flow-log addon (`create_msb_proxy`), and `create_instance` refusing `egress`. The iron-proxy limits come from the user's brief and from `docs/features/podman-egress-boundary/02_dev.md`; they were not re-tested against 0.59.0. `02_dev.md` should run `cargo check`, `cargo test` and `./build e2e light` after each phase, and a `standard` run at the end.

## Open Questions

1. **Existing msb sandboxes and leftovers.** Recommended: no migration; the app stops listing them; project files survive in the host workspace. At startup, terminate `workspace-*` Mutagen sessions, remove msb-provider sidecar state directories, remove the staged msb copy in the app-data root; release notes document stopping leftover VMs. Alternative: ship one transitional release that keeps `msb` solely to remove sandboxes, rejected as keeping the dependency.
2. **`recreate_instance` on the trait.** Recommended: keep the trait method and Podman's working implementation; leave `SandboxBackend::recreate` refusing Podman and let `proxy-toggle-management` lift it, because a bounded Podman recreate also needs sidecar and network handling that belongs to that feature.
3. **Keep the `egress_policy` capability and `InstanceSpec.egress`?** Recommended: keep the type, the flag (constant `false`) and Podman's refusal as the future-engine seam; delete `ChainedProxy`, `StartEnvSource` and the SOCKS-specific pieces, which are msb transport rather than policy.
4. **Drop msb-only schema fields?** Recommended: no; keep `ports`, groups, `violationAction` and `denyMessage` parseable so existing workspace files load; the sidecar dialect already refuses them with a clear error.
5. **Keep `ProxyEngine::Mitmproxy`?** Recommended: keep it as the "pre-boundary Podman sidecar" label until `proxy-toggle-management` provides the upgrade path; remove the mitmproxy image constant and addon.
6. **Return `build dev` to `tauri dev`?** Recommended: yes, after confirming a Podman-only dev session works under the cargo job object; keep the e2e job object but drop the breakaway grant.
7. **When to supersede the two microsandbox plans.** Recommended: at `implemented`, not at ideation. `docs/features/README.md` fixes no stage; the log shows both orders (`proxy-network-policy` at creation, `proxy-non-http-capture` after implementation). The msb code and architecture text stay live until the removal ships, so marking the plans history earlier would misstate current behaviour. At that point: set `status: superseded` and `superseded_by: remove-microsandbox` on both, add two `**Supersede**` log entries, and set this feature to `implemented`.
8. **`ProviderId` wire format.** Recommended: keep the lowercase serde form `podman` and the `podman:<id>` machine-ref prefix, so persisted data and IPC payloads need no migration.
9. **Persisted sandbox labels or aliases naming msb.** Recommended: ignore stale SSH alias blocks for removed sandboxes; they are inert and the alias writer rewrites per sandbox name. Verify at implementation that nothing lists them as sandboxes.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Settings fail to load after the upgrade | A `settings.json` written earlier holds `"defaultProvider": "microsandbox"`, and the single-variant `ProviderId` cannot deserialize it | Remove the `default_provider` field from `src-tauri/src/settings/types.rs` so the unknown key is ignored; never keep the field with a stricter enum |
| `cargo build` fails on a missing `binaries/msb` resource after the change | `src-tauri/tauri.conf.json` still lists the resource while `build.rs` no longer stages it | Remove the `binaries/msb` resource entry and the `msb` row of the staged-binary table in the same change |
| `./build e2e light` fails on `mock-known-calls` | `e2e/test/specs/light/mock-known-calls.spec.ts` pins the `KNOWN_CALLS` list including `msb.metrics` | Update the spec's expected list when `msb.*` keys leave `src-tauri/src/mockctl/mod.rs` |
| A Podman sandbox rejects a secret variable it previously accepted, or vice versa | `RESERVED_ENV` in `src-tauri/src/net_policy.rs` reserved the SOCKS password variable and the `MSB_` prefix for msb | Drop both reservations together with `msb_policy.rs`; keep `PATH` and `HOME` |
| The proxy panel misrenders or throws on undefined status fields | `proxy::types::ProxyStatus` loses `host_port` and `healthy` while `src/lib/proxy/status.svelte.ts` still reads them | Remove the fields on both sides in one change |
| `./build docs` warns about a missing `docs/...` citation | A code comment or doc cites a path the removal deleted | The microsandbox plans are kept, so their citations stay valid; only fix citations of removed spike or fixture files |
| Orphaned Mutagen sessions keep dialling a deleted msb alias | Sessions named `workspace-<sandbox>` outlive the sandbox listing | Terminate every `workspace-*` session in the startup cleanup; the `msb exec` ProxyCommand fails without the binary |
