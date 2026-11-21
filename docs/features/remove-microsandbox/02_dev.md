---
type: dev-plan
title: "Remove microsandbox: Build Plan"
feature: remove-microsandbox
---
# Remove microsandbox: Build Plan

This document is the build plan for deleting the microsandbox (`msb`) provider and making Podman the only provider, continuing `docs/features/remove-microsandbox/01_ideas.md`. The provider, capability, process, transport and net-policy seams stay so that adding a provider later is a small task. Status: Phases A to G, I and J implemented, Phase H dropped. Each phase leaves the tree compiling and the `light` tier green, so a phase can be implemented and committed on its own.

## The Concrete Model

- **One provider behind the full seam.** `ProviderId` keeps a single variant, `Podman`, with the lowercase serde form `podman` and the `podman:<id>` `MachineRef` form (`src-tauri/src/provider/types.rs`). `ProviderRegistry` (`src-tauri/src/provider/mod.rs`) keeps `local_ref`, `provider_for`, `local`, `provider_of`, `engine_of` and `all`; `all` returns a one-element array and `provider_of` keeps its loop, so a second provider is a variant, a registry field, one arm per match and a `MachineProvider` implementation. `MachineProvider`, `InstanceSpec`, `SidecarSpec`, `ExecOpts`, `InstanceStats`, `EnginePool` and `ProviderCapabilities` (all five flags) keep their shape; doc comments that describe microsandbox behaviour are rewritten as provider-neutral contracts.
- **Capabilities stay the gating mechanism.** `provider_capabilities` (`src-tauri/src/commands/image.rs`) and `capabilitiesFor`-style gating (`src/lib/providers.ts`, `src/lib/workspace/sections.ts`, `src/routes/projects/RemoteManager.svelte`) stay. The frontend `ProviderId` union becomes `"podman"`, `PROVIDERS` becomes `["podman"]`, and every IPC command that takes a `provider` parameter keeps it.
- **Process seam.** `ProcessRunner` (`src-tauri/src/process/types.rs`) keeps `SystemPodmanRunner` and `SystemMutagenRunner`; the mock director (`src-tauri/src/mockctl/`) keeps the scripted Podman and Mutagen runners. `SystemMsbRunner`, `ScriptedMsbRunner` and every `msb.*` known call go.
- **Transport.** SSH-alias-over-exec (`ensure_sandbox_alias` in `src-tauri/src/backends/mod.rs`, `src-tauri/src/ssh/mod.rs`) and Mutagen (`src-tauri/src/sync/mod.rs`) stay for remote deploy and forwards. A Mutagen sync session named exactly after a sandbox still means "deployed"; `workspace-<sandbox>` sessions are no longer created.
- **Network policy.** `NetPolicy`, `EgressPolicy`, `EgressRule`, `EgressMode`, `EgressTarget`, `to_egress` and capability-based `validate` stay (`src-tauri/src/net_policy.rs`, `src-tauri/src/provider/types.rs`). `InstanceSpec.egress` and the `egress_policy` flag stay as the engine-enforced seam; Podman reports `false` and `PodmanProvider::create_instance` keeps refusing `Some`. The sidecar dialect keeps refusing ports, groups, host denies, allow CIDRs and UDP.
- **Proxy sidecar.** `create_proxy` (`src-tauri/src/proxy/mod.rs`) is the Podman path only; the sidecar image is iron-proxy (`iron_proxy_image()` in `src-tauri/src/proxy/iron.rs`). `ProxyEngine::Mitmproxy` (`src-tauri/src/proxy/types.rs`) stays only as the label of a pre-boundary Podman sidecar.
- **Leftovers on user machines.** None are cleaned up: by user decision (2026-11-21) nobody uses microsandbox from this project, so there is no startup sweep (Phase H dropped) and no manual cleanup documentation.
- **Dev tooling.** `build dev` keeps launching the app directly (`run_dev_directly` in `build.rs`); only the Job Object breakaway grant goes (Resolved Decision 6).

## Resolved Decisions

1. **Existing msb sandboxes and leftovers.** No migration and no cleanup: the app stops listing microsandbox sandboxes, and their project files survive in the host workspace that Mutagen mirrored. The startup sweep this decision originally recommended (Mutagen `workspace-<host>` sessions, `"provider": "msb"` proxy state, the staged `<app_data_root>/msb/` copy) was dropped by user decision on 2026-11-21: nobody uses microsandbox from this project, so no leftover cleanup code or manual-cleanup documentation is needed. Running microVMs, the `~/.microsandbox` store and stale Mutagen sessions are left untouched. `defaultProvider` in `settings.json` needs no rewrite: once the field leaves `AppSettings` (no `deny_unknown_fields`), the key is ignored on load and dropped on the next save.
2. **`recreate_instance` on the trait.** Kept: `MachineProvider::recreate_instance`, `PodmanProvider::recreate_instance` and `SandboxBackend::recreate` stay. `CliBackend::recreate` keeps refusing Podman and pointing to `docs/features/proxy-toggle-management/`, which owns the bounded Podman recreate. The microsandbox-only `services::sandbox::reapply_network`, its `reapply_sandbox_network` command (`src-tauri/src/commands/net_policy.rs`) and `TaskKind::NetworkReapply` (`src-tauri/src/activity/types.rs`, mirrored as `"network-reapply"` in `src/lib/activity/types.ts`) are deleted rather than kept: its body is the msb chain and workspace-volume handling, and `01_ideas.md` named it `recreate_sandbox_network`. `proxy-toggle-management` adds its own command and task kind.
3. **`egress_policy` capability and `InstanceSpec.egress`.** Kept as the future engine-enforced seam: the flag (constant `false` for Podman), the field and Podman's refusal stay. `EgressPolicy` loses its `proxy: Option<ChainedProxy>` field; `ChainedProxy`, `StartEnvSource`, `NoStartEnv`, `SidecarReadiness`, `SecretViolation` and `ProviderError::PodmanUnavailable` are deleted as msb transport, not policy.
4. **msb-only schema fields.** Kept parseable: `ports`, groups, `violationAction` and `denyMessage` stay in the `NetPolicy` schema, and the sidecar dialect of `validate` keeps refusing them with its existing errors. Existing `.mio/net-policy.json` files load unchanged.
5. **`ProxyEngine::Mitmproxy`.** Kept as the pre-boundary Podman sidecar label until `proxy-toggle-management` provides the upgrade path. `mitmproxy_image()`, the `MIO_PROXY_IMAGE` override and `src-tauri/resources/mitm_flowlog_addon.py` are deleted. `is_sidecar` needs no change: it keys on the name suffix and an unversioned image, which still covers pre-boundary mitmproxy sidecars.
6. **`build dev` and the Job Object breakaway.** Changed from `01_ideas.md`: `build dev` keeps `run_dev_directly` and does not return to `tauri dev`. Only the breakaway grant goes: `JOB_OBJECT_LIMIT_BREAKAWAY_OK` leaves `win_job::setup` in `build.rs`, which keeps kill-on-close. Verification showed the breakaway itself is msb-only: no code in `src-tauri/src/` or `e2e/` spawns with `CREATE_BREAKAWAY_FROM_JOB`, and commit `4aba521` added the grant only for `msb create`. Returning to `tauri dev` is not safe on that evidence alone: `run_dev_directly` also carries Podman-era duties, namely the locked-file move-aside and retry around `cargo build` (the Mutagen daemon's `podman exec` relays lock `target/debug/podman/`), stdin redirected to the null device (`docs/features/podman-process-isolation/02_dev.md`, Resolved Decision 5) and the opt-in `devtools` feature. The `process/mod.rs` doc comment above `run_capturing_via_files`'s blocking sibling also records `tokio::process` hanging on `podman machine list` under the `cargo tauri dev` Job Object. Returning to `tauri dev` is deferred to `04_improvements.md` pending a hands-on check. The e2e harness keeps spawning `msedgedriver` directly (`spawnDriver` in `e2e/wdio.conf.ts`) instead of going back to tauri-driver: the direct spawn works, carries the port-retry logic, and reverting is unrelated churn. Its comment is rewritten to drop the msb reason.
7. **When to supersede the two microsandbox plans.** At `implemented` (Phase J): `docs/features/microsandbox-provider/feature.md` and `docs/features/microsandbox-network-policy/feature.md` get `status: superseded` and `superseded_by: remove-microsandbox`, with two `**Supersede**` entries and one `**Implement**` entry in `docs/features/log.md`.
8. **`ProviderId` wire format.** Kept: lowercase serde `podman` and the `podman:<id>` machine-ref prefix. `FromStr` stops accepting `msb`, and the `ProviderId` doc comment's note about two diverging serializations is reduced to the one remaining form.
9. **Stale SSH alias blocks.** Ignored. Verified: `src-tauri/src/ssh/mod.rs` exposes only `probe_alias`, `ensure_include`, `write_alias` and `remove_alias`, and nothing enumerates aliases, so a stale microsandbox alias is never listed as a sandbox; `write_alias` rewrites a block when a Podman sandbox reuses the name.
10. **Default-provider setting.** Removed (`01_ideas.md`, Technology Choices): `AppSettings::default_provider` (`src-tauri/src/settings/types.rs`), `resolve_default_provider` and the `MIO_MACHINE_PROVIDER` override (`src-tauri/src/settings/mod.rs`), and both pickers. The create flow always sends `provider: "podman"`.
11. **Parallel-worktree impact.** The feature adds no path, port, environment variable or bundled binary. The harness-set `MSB_HOME` goes. A leftover `~/.sandbox-mio-e2e-msb-*` directory on a developer machine is not deleted by the harness, since deleting host-global state needs a tier gate for no benefit.

## Implemented: Phase A — Frontend provider surface and msb specs

Removes the user-visible provider choice and microsandbox machine steps. The backend still contains msb code and still accepts `provider: "microsandbox"`; nothing sends it any more.

- `src/lib/providers.ts`: `ProviderId = "podman"`, `PROVIDERS = ["podman"]`, drop the `microsandbox` label entry; keep `ProviderCapabilities`, `providerCapabilities` and `allCapabilities`. Rewrite the `egressPolicy` doc comment as "engine-enforced policy (no current provider)".
- `src/routes/projects/CreateSandboxItem.svelte`: remove the `data-provider-picker` block, the `defaultProvider` read from settings and the `provider === "microsandbox"` proxy branch; `provider` becomes the constant `"podman"`.
- `src/routes/settings/+page.svelte`: remove the default-provider field (`data-default-provider`), the `defaultProvider` state and its save key.
- `src/lib/machine/MachineSteps.svelte`: remove the `msb-binary`, `msb-version`, `hypervisor-platform` and `msb-doctor` title and explanation entries and the microsandbox comment. `src/lib/welcome/CreateVmStep.svelte`: drop the Phase J picker comment.
- `src/lib/machine/readiness.ts`, `src/lib/machine/types.ts`: drop microsandbox citations; the per-provider keying stays.
- E2E: delete `e2e/test/specs/light/microsandbox-provider-ui.spec.ts`, `e2e/test/specs/light/microsandbox-network-policy-ui.spec.ts` and `e2e/test/specs/standard/microsandbox-egress-allowlist.spec.ts`; remove the msb scenario from `e2e/test/specs/standard/github-placeholder-swap.spec.ts`; remove the provider-picker and msb egress helpers from `e2e/test/pageobjects/projects.page.ts` and any msb selector in `e2e/test/pageobjects/workspace-dashboard.page.ts`.
- Locale keys stay until Phase B, so this phase does not touch `messages/`.
- Verify: `pnpm check`, `./build e2e light`.

## Implemented: Phase B — Network-policy editor, proxy panels and locales

As built: the editor lost its `sidecar` flag, `viewerOn` and `onReapplied` props, not only the msb branches, because `ProxyPanel.svelte` mounts it for an iron-proxy sidecar alone; `TrafficPanel.svelte` lost its `provider` prop and `StatusSummary.svelte` its unhealthy state, both fed by the deleted `healthy` field. The msb-only locale keys were deleted together with the now-unused `netpolicy_*` keys of the non-sidecar branches.

- `src/routes/projects/NetPolicyEditor.svelte`: remove the ports column, violation-action buttons, violation list and its `net_policy_violations` polling, and the `reapply_sandbox_network` path (the microsandbox recreate). The `apply_sandbox_policy` path and the capability-gating structure stay.
- `src/routes/projects/ProxyPanel.svelte`, `src/routes/projects/TrafficPanel.svelte`, `src/lib/proxy/status.svelte.ts`, `src/lib/proxy/traffic.svelte.ts`: remove msb branches and stop reading `ProxyStatus.hostPort` and `healthy` (the backend fields go in Phase D; an unread field is harmless meanwhile).
- `src/lib/activity/types.ts`, `src/lib/activity/activity.svelte.ts`: remove `"network-reapply"` and its label.
- `messages/en.json`, `messages/de.json`, `messages/ru.json`: delete the microsandbox, hypervisor, msb machine-step, violation, ports, `proxy_chained`, `provider_microsandbox_label`, `settings_default_provider_label` and `task_network_reapply_label` keys, after confirming with `git grep` that no `src/` file still calls them. `src/lib/paraglide` regenerates.
- Check `src/lib/models/catalogue.ts`, `types.ts`, `recommend.ts` and `src/lib/agents/types.ts`: their `git grep` hits are on "mitm"/"msb" substrings; change only real microsandbox references.
- Verify: `pnpm check`, `./build e2e light`.

## Implemented: Phase C — Services, commands and settings

As built: `create_sandbox` still takes `provider: Option<ProviderId>` and defaults to Podman; `services::sandbox::create` lost its unused sync-engine parameter; `mark_secret_hosts` went with the msb block.

Removes msb branches above the provider layer. `ProviderId::Microsandbox` still exists, so `if`-guarded branches are deleted and `match` arms stay until Phase E.

- `src-tauri/src/services/sandbox.rs`: in `create`, delete the `provider == ProviderId::Microsandbox` block (start, `msb_sync_session`, `msb_sync_beta`, `engine.create`, its rollback); in `destroy`, delete the `engine.terminate(&backends::msb_sync_session(name))` line and its comment; delete `reapply_network` entirely.
- `src-tauri/src/commands/net_policy.rs`: delete `reapply_sandbox_network` and `net_policy_violations`; reword `set_net_policy`'s comment. `apply_sandbox_policy` keeps its `provider != ProviderId::Podman` guard until Phase E removes the variant.
- `src-tauri/src/commands/mod.rs`: remove both commands from `tauri::generate_handler![]`.
- `src-tauri/src/activity/types.rs`: delete `TaskKind::NetworkReapply`.
- `src-tauri/src/services/routing.rs`: `resolve_machine` loses the non-Podman early return and its comment; `ensure_forward_alias` keeps its `match` until Phase E.
- `src-tauri/src/settings/types.rs`: delete `default_provider`. `src-tauri/src/settings/mod.rs`: delete `resolve_default_provider`, the `MIO_MACHINE_PROVIDER` handling and their tests; fix every caller (`git grep resolve_default_provider`).
- `src-tauri/src/commands/sandbox.rs`: trim the microsandbox clause of the stop-ordering comment, keeping the forward-before-stop order.
- Root `AGENTS.md`: delete the `MIO_MACHINE_PROVIDER` bullet.
- Verify: `cargo check`, `cargo test` (in `src-tauri/`), `pnpm check`, `./build e2e light`.

## Implemented: Phase D — Proxy msb path and the start environment

As built: `sidecar_provider_id` always returns Podman (it no longer reads `meta.json`); `restart_proxy_container` lost its `PortInUse` arm; `write_secret_hosts` and the `base64` dependency went with the msb path; `podman.save` left `KNOWN_CALLS` with the msb provider that alone used it (Phase F). Phases D to F were compiled together once the msb modules were deleted.

- `src-tauri/src/proxy/mod.rs`: `create_proxy` keeps only the Podman path; delete `create_msb_proxy`, `msb_readiness`, `msb_attachment`, `allocate_socks_port`, the SOCKS password generation, `read_socks_password`, `read_meta_host_port` and `meta_host_port`, the microsandbox branch of `sweep` and its comment, and the `host_port`/`healthy` derivation in `status`. `proxy_image(provider_id)` returns iron-proxy only; delete `mitmproxy_image()` and the `MIO_PROXY_IMAGE` read. `sidecar_provider_id` loses its `Some("msb")` arm, so a leftover `meta.json` naming `msb` falls through to Podman like a missing value. Rewrite the module doc comment.
- `src-tauri/src/proxy/types.rs`: delete `ProxyAttachment::MsbChain`, `ProxyStatus::host_port` and `ProxyStatus::healthy`; keep `ProxyEngine::Mitmproxy`.
- `src-tauri/src/proxy/flowlog.rs`, `src-tauri/src/proxy/audit.rs`: repoint `FLOW_LOG_MAX_LINES` and related comments from the mitmproxy addon to iron-proxy's audit translation. Delete `src-tauri/resources/mitm_flowlog_addon.py` and its `include_str!`. Check `src-tauri/src/proxy/fixtures/podman-logs-timestamps.txt`: its `git grep` hits are log content, not code; keep the fixture unless a test that used it is deleted.
- `src-tauri/src/backends/mod.rs`: the msb create arm passes no chained proxy now that `MsbChain` is gone (the arm itself is deleted in Phase E).
- Delete `src-tauri/src/services/start_env.rs` and its `mod` line in `src-tauri/src/services/mod.rs`. `default_registry` (`src-tauri/src/provider/mod.rs`) is passed `NoStartEnv` from `src-tauri/src/bootstrap.rs` until Phase E deletes the parameter.
- `src-tauri/Cargo.toml`: remove `sha1` and its comment if `git grep -n sha1 src-tauri/src` finds no other use.
- Root `AGENTS.md`: delete the `MIO_PROXY_IMAGE` bullet.
- Verify: `cargo check`, `cargo test`, `pnpm check`, `./build e2e light`.

## Implemented: Phase E — The provider variant and the msb provider

As built: `AppliedPolicy::chained_proxy` was deleted with `ChainedProxy` (an old `applied.json` key is ignored on load); `CliBackend::recreate` became a plain refusal; `split_attachment` is infallible and takes only the attachment; `ProviderError::PortInUse` and `MachineProvider::recreate_instance` stay with `allow(dead_code)` as seam contracts; `probe_alias` and its helper in `src-tauri/src/ssh/mod.rs` were deleted as msb-only diagnostics.

Compiler-driven: delete the variant and the modules, then fix every error `cargo check` reports with the Podman arm alone. Do not replace a `match` with an `if let` or a direct call: a one-arm `match ProviderId` is the seam a future provider extends.

- `src-tauri/src/provider/types.rs`: delete `ProviderId::Microsandbox` (and its `tag`/`FromStr` arms), `ProviderError::PodmanUnavailable` and its `From` arm, `ChainedProxy`, `EgressPolicy::proxy`, `SecretViolation`, `SidecarReadiness`, `StartEnvSource`, `NoStartEnv`, and the `msb:local` round-trip test. Rewrite field docs on `InstanceSpec`, `SidecarSpec` and `ProviderCapabilities` that say "Microsandbox ignores it" as neutral contracts.
- Delete `src-tauri/src/provider/msb.rs`, `src-tauri/src/provider/msb_policy.rs`, `src-tauri/src/provider/probe.rs` and `src-tauri/src/provider/fixtures/msb-metrics-*.json`; remove their `pub mod` lines.
- `src-tauri/src/provider/mod.rs`: delete `MSB_LOCAL_ID`, the `msb` registry field, its arms in `local_ref` and `provider_for`, and its `all` entry (`all` returns `[_; 1]`); `default_registry` takes only the Podman runner. Rewrite the `provider_of` doc comment without the `msb inspect` remark.
- `src-tauri/src/net_policy.rs`: remove the SOCKS password variable and the `MSB_` prefix from `RESERVED_ENV`, keeping `PATH` and `HOME`; update tests that asserted them. `to_egress` stays.
- Delete `src-tauri/src/machine/msb.rs`; in `src-tauri/src/machine/mod.rs` remove `pub mod msb`, the `msb` field of the runners struct and the factory arm; trim microsandbox wording in `src-tauri/src/machine/types.rs`.
- `src-tauri/src/backends/mod.rs`: delete `ensure_msb_sandbox_alias`, `msb_proxy_command`, `msb_literal_path`, `msb_sync_beta`, `msb_sync_session`, `MSB_WORKSPACE_MOUNT` and their tests, the `ProviderId::Microsandbox` arm of the alias router, and microsandbox branches in create and teardown. `src-tauri/src/backends/types.rs`: drop the msb wording; delete `SyncStrategy::Volume` only if nothing references it (it is already `allow(dead_code)`).
- `src-tauri/src/services/routing.rs`: drop the msb arm of `ensure_forward_alias`. `src-tauri/src/services/resources.rs`: the `running` array and `ProviderId` matches lose the msb entries; delete the msb accumulator tests.
- `src-tauri/src/commands/net_policy.rs`: `apply_sandbox_policy`'s Podman guard becomes unreachable; remove it.
- Verify: `cargo check`, `cargo test`, `./build e2e light`. Unused-code warnings for the msb runner are expected until Phase F.

## Implemented: Phase F — msb runner, mock and staging plumbing

As built: `run_capturing_via_files` and its two tests in `src-tauri/src/process/mod.rs` were deleted too, since the msb runner was its only caller (the Podman runner uses the blocking path); the doc comment on the blocking path keeps the `cargo tauri dev` hang note. `sha256_of` in `build.rs` went with `fetch_msb`.

- `src-tauri/src/process/types.rs`: delete `SystemMsbRunner` and its `ProcessRunner` impl. `src-tauri/src/process/mod.rs`: delete `MSB_RESOURCE_DIR`, `set_msb_resource_dir`, `msb_executable`, `msb_command`, `MSB_CALL_TIMEOUT`, `MSB_CREATE_TIMEOUT`, `msb_call_timeout`, `stage_msb`, `prune_staged_msb`, `MSB_METRICS_ARGV` and their tests. Reduce the doc comments that cite msb handle inheritance or `msb_command` (the blocking-run doc comment near `run_capturing_via_files`) to their Podman rationale; keep the `tokio::process` hang under the `cargo tauri dev` Job Object as stated, since it is Podman-observed. `log_call_outcome!` keeps the Mutagen use.
- `src-tauri/src/mockctl/types.rs`: delete `ScriptedMsbRunner`. `src-tauri/src/mockctl/mod.rs`: delete the `msb.*` entries of `KNOWN_CALLS`, `derive_msb_key`, the msb defaults and the tracked-instance logic; update the module doc. `e2e/test/specs/light/mock-known-calls.spec.ts`: drop the `msb.*` keys from the expected list; `e2e/lib/mock.ts`: change the `msb.create` example.
- `src-tauri/src/bootstrap.rs`: delete the `msb_runner` field and construction, `staged_msb_dir`, the `set_msb_resource_dir` call, and the `msb` entry passed to the machine-manager runners.
- `build.rs`: delete `MSB_VERSION`, `msb_bundle_asset`, `fetch_msb`, `stage_msb`, the `msb` asset group (key, sentinel, paths, fetch), `"msb"` in the staged-versions key list and in `move_aside_locked_dev_files`'s resource directories, and the `fetch_msb`/`stage_msb` calls in `dev --setup`, `dev` and `package`. Drop `JOB_OBJECT_LIMIT_BREAKAWAY_OK` from `win_job::setup` and rewrite `win_job`'s doc comment. Rewrite `run_dev_directly`'s doc comment and the `build dev` usage text to the reasons that remain (Resolved Decision 6). Remove the microsandbox wording from the `dev --setup` usage line.
- `src-tauri/tauri.conf.json`: remove the `binaries/msb` resource in the same change as the `build.rs` group (Common Gotchas).
- `e2e/lib/run-identity.ts`, `e2e/wdio.conf.ts`: delete `MSB_HOME`, `MSB_PATH` and the `process.env.MSB_HOME` export; drop `msb` from `killStaleDevProcesses`'s process names (the `target\debug\` prefix filter stays); rewrite the `spawnDriver` comment to say `msedgedriver` is spawned directly for its port-retry control, not for msb.
- Rebuild `build.exe` (`rustc build.rs`) after editing `build.rs`.
- Verify: `cargo check`, `cargo test`, `./build docs`, `./build e2e light`; `git status` shows no new untracked asset under `src-tauri/binaries/`.

## Implemented: Phase G — Residual comment and citation sweep

As built: the Containerfile ENTRYPOINT comment was shortened without a `BASE_IMAGE_VERSION` bump, since a comment-only change alters no image behaviour.

- Run `git grep -n -i -E "microsandbox|\bmsb\b|Msb|mitm" -- ':!docs'` and resolve every hit. Allowed residuals: `ProxyEngine::Mitmproxy` and its comments, and log or fixture content.
- Comments citing `docs/features/microsandbox-*` are deleted when the constraint they explain went away, and kept when it still holds for Podman (the plans stay, so citations remain valid). Known sites: `src-tauri/src/commands/image.rs`, `src-tauri/src/commands/machine.rs`, `src-tauri/src/runtime/types.rs`, `src-tauri/src/credentials/mod.rs`, `src-tauri/src/ssh/mod.rs`, `src-tauri/src/services/inference_reach.rs`, `src-tauri/resources/Containerfile` (the Podman-only ENTRYPOINT comment).
- `src-tauri/AGENTS.md`, `e2e/AGENTS.md`: remove msb rows and sentences.
- Verify: `cargo check`, `pnpm check`, `./build docs`, `./build e2e light`.

## Dropped: Phase H — Startup leftover sweep

Dropped by user decision on 2026-11-21: nobody uses microsandbox from this project, so no leftover cleanup is built (Resolved Decision 1). Nothing from this phase exists in the code.

## Implemented: Phase J — Final verification and bookkeeping

As built: two full `standard` runs each failed three specs, a different set each time; `expose-port`, the only repeat, passes alone. The manual check was skipped, since nobody had a microsandbox sandbox.

- Run `./build e2e` (`standard`) once, host-locked against the other real-infra runs per `e2e/AGENTS.md`. Expected: every spec that remains passes; no spec references microsandbox.
- Manual check on a machine that had a microsandbox sandbox, if available: start the app, confirm the sandbox list shows only Podman sandboxes and settings load.
- Bookkeeping: in this feature's `feature.md`, set `status: implemented` and update the status note; in `docs/features/microsandbox-provider/feature.md` and `docs/features/microsandbox-network-policy/feature.md`, set `status: superseded`, add `superseded_by: remove-microsandbox` and a one-line status note; add one `**Implement**` and two `**Supersede**` entries at the top of `docs/features/log.md`. Run `./build docs --fix` and `./build docs`.

## Lifecycle and Cross-Cutting Concerns

- **Phase order is load-bearing.** The frontend stops sending msb values (A, B) before the backend stops accepting them (C to F).
- **Persisted data.** `settings.json` (`defaultProvider` ignored), `.mio/net-policy.json` (schema unchanged), proxy `meta.json` (an unrecognised provider maps to Podman), and resource history lines under `paths::resource_history_dir` whose `provider` is `microsandbox` are silently skipped by `read_rollups`' per-line parse until pruned by age. No persisted file holds a `MachineRef`; `SandboxRoute.machine` (`src-tauri/src/services/routing.rs`) and `src-tauri/src/terminal/types.rs` carry it only in memory and over IPC.
- **Tests.** Unit tests deleted with their modules are not replaced; the Podman equivalents already exist. No new tests are added.
- **Out of scope.** Recorded in `04_improvements.md`: returning `build dev` to `tauri dev` and the iron-proxy items already listed there.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Settings fail to load after the upgrade | A `ProviderId`-typed field still reads `"defaultProvider": "microsandbox"` | Delete `default_provider` from `AppSettings` (`src-tauri/src/settings/types.rs`); never keep it with the narrower enum |
| `cargo build` fails on a missing `binaries/msb` resource | `src-tauri/tauri.conf.json` lists the resource while `build.rs` no longer stages it | Remove the resource entry and the `build.rs` asset group in one change (Phase F) |
| `./build e2e light` fails on `mock-known-calls` | `e2e/test/specs/light/mock-known-calls.spec.ts` pins `KNOWN_CALLS` including `msb.*` | Update the spec in the same change as `src-tauri/src/mockctl/mod.rs` (Phase F) |
| A proxy status or flow lookup errors for a sandbox whose `meta.json` says `msb` | Code that parses the `meta.json` provider into `ProviderId` with `FromStr`, which no longer accepts `msb` | Read it through `sidecar_provider_id`, which falls back to Podman for a missing or unknown tag |
| `build dev` starts failing with locked files or a hung `podman machine list` | Someone switched `build dev` back to `tauri dev` because msb is gone | Keep `run_dev_directly`; only the breakaway grant was msb-only (Resolved Decision 6) |
| A future agent collapses `match provider` into a direct Podman call | One-arm matches look redundant | Keep them: the one-arm matches and `ProviderRegistry::all` are the extension points this feature preserves |
| A Podman sandbox rejects or accepts a secret variable differently | `RESERVED_ENV` still holds the SOCKS password variable or the `MSB_` prefix | Remove both in Phase E; keep `PATH` and `HOME` |
| `./build docs` warns about a missing citation | A comment cites a deleted file such as a msb fixture or the mitmproxy addon | The microsandbox plans are kept, so plan citations stay valid; fix only citations of deleted files |
| A developer checkout still has `src-tauri/binaries/msb` and `target/debug/msb` | `build.rs` no longer fetches or stages them, and does not delete them | Harmless; delete by hand. Nothing reads them |
