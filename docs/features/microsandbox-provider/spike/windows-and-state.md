---
type: spike
title: "Spike: Windows host viability, state directories, networking policy"
feature: microsandbox-provider
---
# Spike: Windows host viability, state directories, networking policy

Desk research performed 2026-08-09 against microsandbox's published documentation, the `superradcompany/microsandbox` GitHub repository (issues, pull requests, releases, and raw source on `main`), and Microsoft's Windows Hypervisor Platform documentation. No `msb` binary was installed, no Windows optional feature was enabled, and no command was run against a live microsandbox host. The `gh` CLI is not installed on this development machine, so every GitHub lookup went through the public REST API with `curl` — the equivalent `gh` invocation is given alongside each result so the query can be reproduced.

Latest published release at time of writing: **v0.6.8**, 2026-07-29. Windows support first shipped in **v0.6.0**, 2026-06-27 — roughly six weeks old.

## Summary

**Is it safe to enable `HypervisorPlatform` on a host that already runs Podman over WSL2? Yes — CONFIRMED.** `HypervisorPlatform` and `VirtualMachinePlatform` are independent Windows optional features that both sit on top of the same Hyper-V hypervisor, and enabling one does not disable or degrade the other. The strongest single piece of evidence is microsandbox issue #1049, filed by a user running microsandbox, WSL2, and Docker Desktop concurrently on Windows 11 26200 and describing "the WHP/Hyper-V stack is healthy" while all three were active. Issue #1092 adds a second data point: a user toggled WHP, Virtual Machine Platform, Hyper-V, and WSL off and on across reboots and reported "WSL works perfectly for me though" throughout. Microsoft's own framing is that a third-party WHP client "can run on the hypervisor ... all while running alongside a Hyper-V managed partition with no overlap". In the wider Windows ecosystem, enabling `HypervisorPlatform` is the standard remedy that lets VirtualBox and VMware Workstation coexist with WSL2, not a cause of breakage. The risk to the Podman machine is therefore low, and the spike's stage-one gate should pass.

**That is the only good news in this report.** Three findings are more consequential for the feature than the hypervisor question:

1. **Published ports do not reach a guest's own loopback.** microsandbox dials the guest's network-interface IP, not `127.0.0.1` inside the guest. Both of the app's primary surfaces — the zellij web terminal on 8082 and the paseo agent UI on 6767 — bind loopback inside the container today and depend on Podman's `--network pasta:--host-lo-to-ns-lo` flag to be reachable. Under microsandbox they would have to be rebound to `0.0.0.0`, or fronted by an in-guest relay. See section 7(a).
2. **Two open, unfixed Windows bind-mount bugs make virtiofs unreliable for a dev workspace right now.** Issue #1219 (any Rust-built binary in the guest, including `ls` on Ubuntu 25.10+, fails on a virtiofs mount with a spurious `ELOOP`) and issue #1218 (closing a read-only file on a bind mount returns `EACCES`) are both open against 0.6.6/0.6.7 with the `Windows` label. See sections 6 and 8.
3. **`MSB_HOME` does relocate everything, so per-worktree e2e isolation is viable** — with one wiring caveat about the `msb` binary itself. See section 3.

---

## 1. WHP vs WSL2 coexistence — CONFIRMED (they coexist)

**Verdict: CONFIRMED.** Both features can be enabled on the same Windows 11 host. No report exists of either breaking the other, and there is direct evidence of all three stacks running simultaneously.

**The features are architecturally distinct, not alternatives.** microsandbox's troubleshooting page states it plainly: "`HypervisorPlatform` is separate from `VirtualMachinePlatform`, which Docker Desktop and WSL2 commonly enable. microsandbox needs `HypervisorPlatform` because that exposes the WHP API used by libkrun." The same distinction is baked into microsandbox's own diagnostic hints in `sdk/rust/lib/setup/windows.rs`, whose user-facing text reads: "HypervisorPlatform is the WHP API that libkrun uses; it is not the full Hyper-V management role. VirtualMachinePlatform for WSL2/Docker is separate, so it can be enabled while HypervisorPlatform is still off." That sentence establishes the two features are orthogonal in one direction; the issue evidence below establishes the other.

**Direct evidence of coexistence.** Issue #1049, "Windows: msb run crashes in winhvplatform.dll (0xc0000005) at VM entry (v0.6.0)", describes an environment of Windows 11 25H2 build 26200, Intel i9-9900K, with "Hyper-V hypervisor running; **WSL2 + Docker Desktop active and working** (the WHP/Hyper-V stack is healthy)". The reporter's analysis explicitly rules out host misconfiguration: "WHP itself is installed and functional on this host — WSL2 and Docker Desktop use it concurrently without issue". The bug in that issue was a microsandbox WHP-API misuse, fixed in v0.6.1 (maintainer comment, 2026-06-28: "nailed it down btw. fix ships in the next release, v0.6.1"). It is evidence *for* coexistence, not against it: WSL2 kept working while microsandbox failed.

Issue #1092 is the second data point. The reporter ran, in the course of debugging: "Tried disable -> restart -> reenable -> restart for WHP, Virtual Machine Platform, Hyper-V, and Windows Subsystem for Linux. No luck unfortunately. **WSL works perfectly for me though.**" Issue #1095 similarly reports a host where "`Microsoft-Hyper-V-Hypervisor` feature is enabled; `HypervisorPlatform` is enabled" at the same time.

**Does Hyper-V being enabled block or enable WHP?** It enables it. WHP is a user-mode API layered on the Hyper-V hypervisor, not a competing hypervisor. Microsoft's Hyper-V API overview states that a third-party WHP client "such as QEMU can run on the hypervisor while maintaining its management, configuration, guest/host protocols and guest supported drivers. All while running alongside a Hyper-V managed partition with no overlap." microsandbox's own error vocabulary reflects the dependency: `WindowsHostSetupError::HypervisorNotPresent` renders as "WHP is installed but the hypervisor is not active for this Windows boot session" — WHP needs the hypervisor to be running, which a WSL2 host already guarantees. Because WSL2 implies the Hyper-V hypervisor is loaded at boot, a Podman-over-WSL2 host is in fact the *easy* case for WHP, not the hard one.

The broader ecosystem pattern points the same way: the standard fix for VirtualBox or VMware Workstation failing on a WSL2 host is to enable `HypervisorPlatform` so the third-party VMM rides WHP instead of demanding exclusive VT-x. Enabling the feature is the coexistence mechanism.

**Residual risk, and what would still need checking on our host.** Nothing found suggests enabling the feature breaks WSL2. Two secondary risks remain, neither of which is about coexistence:

- Enabling `HypervisorPlatform` with `-All` pulls in its dependency features. On a host where WSL2 already runs, those dependencies are already satisfied, so the change should be a no-op beyond the WHP API surface. On a host where they are not, the reboot is where the risk sits.
- Hardware-specific WHP faults exist independently of WSL2. Issue #1095 documents every microsandbox VM triple-faulting on an AMD Ryzen 9800X3D (Windows 11 26200) with WHP correctly enabled and `msb doctor` reporting a healthy host, traced to missing CPUID masking in the WHP backend. It was closed the same day it was opened, so a fix landed, but it shows `msb doctor` reporting green does not prove the host can actually boot a VM. Stage one of the spike must run an actual `msb run`, not just `msb doctor`.

Sources:
- <https://docs.microsandbox.dev/troubleshooting/windows.md>
- <https://github.com/superradcompany/microsandbox/issues/1049> (`gh issue view 1049 --repo superradcompany/microsandbox`)
- <https://github.com/superradcompany/microsandbox/issues/1092> (`gh issue view 1092 --repo superradcompany/microsandbox --comments`)
- <https://github.com/superradcompany/microsandbox/issues/1095>
- <https://raw.githubusercontent.com/superradcompany/microsandbox/main/sdk/rust/lib/setup/windows.rs>
- <https://learn.microsoft.com/en-us/virtualization/api/>
- Searches run (`gh search issues --repo superradcompany/microsandbox <term>`): `WSL2` (7 hits), `HypervisorPlatform` (2), `hyper-v` (4), `VirtualMachinePlatform` (0), `wsl broke OR broken OR conflict` (6). No issue in any of these describes WSL2 or Docker Desktop breaking as a result of microsandbox or of enabling `HypervisorPlatform`.

## 2. Reboot requirement and what `msb doctor --fix` actually does — CONFIRMED

**Verdict: CONFIRMED. A reboot is required, and `msb doctor --fix` does not reboot — it only enables the feature.**

The documented manual command is `Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform -All -NoRestart`, and the troubleshooting page instructs the reader to "run this from an elevated PowerShell and **reboot after it completes**". The same page adds a follow-up case: "If the feature is enabled but `msb doctor` still reports that the hypervisor is unavailable, confirm that virtualization is enabled in firmware/UEFI, then reboot once."

`msb doctor --fix` runs that exact command, unchanged, including the `-NoRestart` flag. The CLI source (`crates/cli/lib/commands/self_cmd.rs`) builds a `Start-Process ... -Verb RunAs -Wait -PassThru` wrapper around the constant `ENABLE_HYPERVISOR_PLATFORM_COMMAND` defined in `sdk/rust/lib/setup/windows.rs`, which is the literal string above. It reports "Windows Hypervisor Platform enable command completed", re-runs the diagnosis, and exits. It never calls `Restart-Computer` and never prompts for a reboot. The re-check after the fix will therefore still fail on a host that has not rebooted, and the process exits non-zero.

Three behaviours of `--fix` matter for driving it from our app or from an e2e harness:

- **It is UAC-gated.** The fix opens a *new* elevated PowerShell window via `-Verb RunAs`, which raises a consent dialog. It cannot be applied silently from an unelevated process.
- **It prompts interactively by default.** `apply_windows_fix` asks `Apply fix — enable Windows Hypervisor Platform? [y/N]` unless `--yes` is passed.
- **It refuses to run non-interactively without `--yes`.** The confirmation helper bails with "non-interactive terminal; pass `--yes` to apply fixes". Any automation must pass `--yes` and still survive the UAC prompt.

Plain `msb doctor` (and its aliases `msb self doctor` and `msb self check`) is read-only: "`msb doctor --fix` keeps the normal doctor command read-only."

Implication for the readiness contract proposed in `01_ideas.md`: the microsandbox `prepare(Channel)` step for `HypervisorPlatform` cannot complete within a single app session on a host that has never had the feature enabled. It must surface a "reboot required" terminal state, in the same shape the Podman WSL-install step already needs.

Sources:
- <https://docs.microsandbox.dev/troubleshooting/windows.md>
- <https://raw.githubusercontent.com/superradcompany/microsandbox/main/crates/cli/lib/commands/self_cmd.rs> (`run_doctor`, `apply_windows_fix`, `enable_windows_hypervisor_platform`)
- <https://raw.githubusercontent.com/superradcompany/microsandbox/main/sdk/rust/lib/setup/windows.rs> (`ENABLE_HYPERVISOR_PLATFORM_COMMAND`)

## 3. `MSB_HOME` — CONFIRMED as a real, honoured variable that relocates all state

**Verdict: CONFIRMED, and stronger than `01_ideas.md` assumed.** `MSB_HOME` is not merely the root the SSH authorised-keys path resolves against; it is *the* root for every on-disk subsystem, and per-process isolation is an explicitly documented design goal of the variable.

The published documentation only mentions `MSB_HOME` in the SSH command reference — "The default authorization file is `<MSB_HOME>/ssh/authorized_keys`, or `~/.microsandbox/ssh/authorized_keys` when `MSB_HOME` is unset" — which is exactly the narrow framing that made this a load-bearing open question. The source settles it.

`microsandbox_utils::resolve_home` (`crates/utils/lib/lib.rs`) is the single resolver, documented as: resolution order is (1) the `MSB_HOME` environment variable, used as-is with no `.microsandbox` suffix appended, (2) `~/.microsandbox`, (3) `./.microsandbox` if no home directory is available. Its doc comment states the intent directly: "`MSB_HOME` lets CI and integration tests isolate microsandbox state (db, sandboxes, cache, logs) per process without disturbing other `$HOME`-rooted tooling." A unit test asserts the override is honoured verbatim, and its comment identifies `resolve_home` as "the sole reader of `MSB_HOME` in this binary".

Every directory accessor on `LocalConfig` (`sdk/rust/lib/config/mod.rs`) falls back to `self.home().join(<SUBDIR>)`, where `home()` is `resolve_home()` unless the `config.json` `home` field overrides it. That covers `sandboxes_dir`, `volumes_dir`, `snapshots_dir`, `logs_dir`, `cache_dir`, `secrets_dir`, `ssh_dir`, and `run_dir`. The persisted config file path itself is `resolve_home()/config.json` unless `MSB_CONFIG_PATH` overrides it, so the config follows the home too. The SQLite database defaults to living under `home` when no `database.url` is set.

Two further details confirm the isolation is intentional rather than incidental:

- The live-metrics shared-memory registry name is derived by hashing the resolved home directory, with the stated rationale "so concurrent `MSB_HOME`-isolated environments do not collide". Running two microsandbox homes in parallel on one host is a supported scenario, not an accident.
- `msb doctor` prints the resolved home as a first-class diagnostic fact. Issue #1092's output shows `info MSB_HOME: C:\Users\fdafd\.microsandbox`, which also confirms the CLI binary itself — not just the SDK — routes through the same resolver.

**The one caveat that will bite the e2e harness.** Relocating `MSB_HOME` also relocates where microsandbox looks for its *own* binary and kernel library. The `msb` binary resolution ladder is `MSB_PATH` env, SDK-provided runtime path, `config.paths.msb`, debug-only workspace probes, `{home}/bin/msb`, then a `PATH` lookup. `libkrunfw` resolves via `MSB_LIBKRUNFW_PATH`, SDK runtime path, `config.paths.libkrunfw`, a sibling of the resolved `msb` binary, `../lib/` next to the resolved `msb` binary, then `{home}/lib/`. A fresh per-run `MSB_HOME` directory contains neither `bin/` nor `lib/`, so resolution falls through to the `PATH` lookup for `msb` and then to `../lib/` relative to whatever that found. That works when the real install is on `PATH`, but it is a fallback, not a guarantee.

Recommended harness wiring, alongside the existing `MIO_APP_DATA_DIR` and `SANDBOX_WORKSPACE_ROOT` roots: set `MSB_HOME` to the `RUN_SLUG`-keyed directory *and* pin `MSB_PATH` to the real install at `%USERPROFILE%\.microsandbox\bin\msb.exe`, so binary resolution never depends on `PATH` ordering.

With that in place, nothing found suggests a host-wide lock survives the relocation: the sandbox registry is the SQLite database under `home`, the maintenance lease is a row in that database, per-sandbox agent channels are named pipes keyed by a per-sandbox hash, and the metrics shared-memory name is keyed by home. Microsandbox e2e specs should therefore *not* need the host-wide lock that `e2e/AGENTS.md` imposes on the `mio-tests` Podman machine.

Sources:
- <https://raw.githubusercontent.com/superradcompany/microsandbox/main/crates/utils/lib/lib.rs> (`resolve_home`, the `*_SUBDIR` constants, `test_resolve_home_respects_env_override`)
- <https://raw.githubusercontent.com/superradcompany/microsandbox/main/sdk/rust/lib/config/mod.rs> (`LocalConfig::home` and the directory accessors, `config_path`, `resolve_msb_path_for_config`, `resolve_libkrunfw_path_for_config`)
- <https://docs.microsandbox.dev/configuration.md>
- <https://docs.microsandbox.dev/cli/ssh-commands.md>
- <https://github.com/superradcompany/microsandbox/issues/1092> (`msb doctor` output showing `info MSB_HOME:`)

## 4. On-disk state layout under the microsandbox home — CONFIRMED

**Verdict: CONFIRMED**, from both the published configuration reference and the constants in `crates/utils/lib/lib.rs`. Default root on Windows is `%USERPROFILE%\.microsandbox`; the base directory name is the constant `.microsandbox`.

| Subdirectory | Contents | Individually overridable in `config.json`? |
|---|---|---|
| `bin/` | The `msb` executable (`msb.exe` on Windows) | Yes, via `paths.msb` (and the `MSB_PATH` env var) |
| `lib/` | The VM kernel library (`libkrunfw.dll` on Windows) | Yes, via `paths.libkrunfw` (and `MSB_LIBKRUNFW_PATH`) |
| `cache/` | Image layer cache — this is where pulled OCI images live | Yes, via `paths.cache` |
| `sandboxes/` | Per-sandbox state | Yes, via `paths.sandboxes` |
| `snapshots/` | Snapshot artefacts | Yes, via `paths.snapshots` |
| `volumes/` | Named volumes | Yes, via `paths.volumes` |
| `logs/` | Sandbox logs | Yes, via `paths.logs` |
| `secrets/` | Secrets; registry credentials under `secrets/registries/` | Yes, via `paths.secrets` |
| `ssh/` | Host-side SSH state, including `ssh/authorized_keys` | **No** — always `{home}/ssh` |
| `tls/` | TLS interception CA material | **No** — always `{home}/tls` |
| `db/` | SQLite database, when no `database.url` is configured | Only indirectly, via `database.url` |
| `run/` | Ephemeral runtime artefacts, including `run/metrics/` | **No** — always `{home}/run` |
| `config.json` | Global config shared by the CLI and every SDK | Via the `MSB_CONFIG_PATH` env var |

Note that there is no `images/` directory: OCI images are layers in `cache/`, and the artefacts a snapshot-based base image would produce live in `snapshots/`. That matters for `MsbProvider::base_image_ready`, which would query the snapshot store rather than an image directory.

The `ssh`, `tls`, and `run` directories are the ones with no per-path override, which is a second reason `MSB_HOME` (rather than individual `paths.*` overrides) is the right isolation lever for the e2e harness.

Sources:
- <https://docs.microsandbox.dev/configuration.md> (the `paths` table)
- <https://raw.githubusercontent.com/superradcompany/microsandbox/main/crates/utils/lib/lib.rs> (`BASE_DIR_NAME`, `BIN_SUBDIR`, `LIB_SUBDIR`, `DB_SUBDIR`, `CACHE_SUBDIR`, `SANDBOXES_SUBDIR`, `VOLUMES_SUBDIR`, `SNAPSHOTS_SUBDIR`, `LOGS_SUBDIR`, `SECRETS_SUBDIR`, `TLS_SUBDIR`, `SSH_SUBDIR`, `RUN_SUBDIR`, `METRICS_RUN_SUBDIR`)
- <https://raw.githubusercontent.com/superradcompany/microsandbox/main/sdk/rust/lib/config/mod.rs> (`ssh_dir` and `run_dir` join `home()` unconditionally)

## 5. Windows host paths in `--mount-dir` — CONFIRMED, both separators work

**Verdict: CONFIRMED.** Both `C:\Users\me\work:/root/work` and `C:/Users/me/work:/root/work` parse correctly on a Windows host. The mount-spec parser has explicit drive-letter handling.

The published documentation is silent on Windows path syntax — the CLI reference only gives the generic `SOURCE:DEST[:OPTIONS]` shape — so this was resolved from source and from issue repro scripts.

`crates/cli/lib/commands/common.rs` splits a mount specification at the first colon that both (a) is not a Windows drive separator and (b) is immediately followed by `/`, i.e. the start of an absolute guest path. The drive-separator test lives in `microsandbox_utils::is_windows_drive_separator_at` and treats a colon as a drive separator when it sits at index 1, the first byte is ASCII alphabetic, and the byte after the colon is **either a backslash or a forward slash**. Forward-slashed Windows paths are therefore handled identically to backslashed ones — which is exactly the form `workspace_path()` in `src-tauri/src/backends/mod.rs` already emits for Podman. No change to that helper is needed.

A Windows-gated unit test in the same file, `test_apply_explicit_dir_mount_with_windows_drive_path`, builds a spec from a real temp directory path plus `:/work:ro` and asserts the resulting bind mount's host path, guest path, and read-only flag. A sibling test covers the same drive-letter handling for the `--copy-file` patch flag.

Independent confirmation from the field: the repro scripts in issues #1218 and #1219 both pass backslashed Windows paths straight through, e.g. `msb run --mount-dir "${dir}:/w" debian -- cat /w/f.txt` where `$dir` is `C:\Users\<user>\AppData\Local\Temp\msb-repro-flush`, and a commenter on #1219 uses the `-v` shorthand with `C:\tmp\btime-repro:/mnt`. In every case the mount is created; the reported failures are filesystem-behaviour bugs after the mount succeeds, not parse failures.

Two constraints the parser imposes:

- **The guest path must be absolute.** The separator search only accepts a colon followed by `/`, so `C:\work:work` will not parse.
- **The drive-colon exemption is `#[cfg(windows)]` only.** On a non-Windows host the same specification string would misparse. Irrelevant for our Windows-first app, but it means a mount specification cannot be built once and shared across host platforms verbatim.

No open or closed issue reports a Windows mount path failing to parse. Search `mount windows path` returned three issues, all filesystem-behaviour bugs (#1218, #1219, #1191), none a parsing problem.

Sources:
- <https://raw.githubusercontent.com/superradcompany/microsandbox/main/crates/cli/lib/commands/common.rs> (`find_guest_path_separator`, `is_windows_drive_separator`, `test_apply_explicit_dir_mount_with_windows_drive_path`)
- <https://raw.githubusercontent.com/superradcompany/microsandbox/main/crates/utils/lib/lib.rs> (`is_windows_drive_separator_at`)
- <https://github.com/superradcompany/microsandbox/issues/1218> and <https://github.com/superradcompany/microsandbox/issues/1219> (repro scripts using backslashed Windows paths)
- <https://docs.microsandbox.dev/cli/sandbox-commands.md>, <https://docs.microsandbox.dev/sandboxes/volumes.md>

## 6. virtiofs on Windows for a bind-mounted source tree — CONTRADICTED for our use case

**Verdict on documented caveats: CONTRADICTED** — the documentation actively markets bind mounts as the fast path ("Mounts provide direct filesystem access and are significantly faster than the filesystem API"; "For bulk operations, volumes are significantly faster") and records no Windows-specific performance caveat at all. That optimistic framing does not survive contact with the open issue tracker.

**Verdict on correctness: two open, unfixed, Windows-only bind-mount bugs.**

- **Issue #1219 (open, labelled `Filesystems`/`Investigate`/`Windows`, filed 2026-07-27 against 0.6.6 and 0.6.7).** On a Windows host, unimplemented FUSE opcodes are answered with the *host's* `ENOSYS` value. Windows' `ENOSYS` is 40, which is Linux's `ELOOP`, so the Linux guest reads "function not implemented" as "too many levels of symbolic links". The specific missing opcode is `FUSE_STATX`, so every `statx` call on a virtiofs mount fails permanently — the guest kernel never falls back to `GETATTR` because it never sees a real `-ENOSYS`. Concretely: `ls -la /` shows `d?????????` for every virtiofs mount including microsandbox's own `/.msb` runtime share, and any binary built on Rust's standard library (which uses `statx`) breaks on the mount. Debian's GNU coreutils use `fstatat` and are unaffected; Ubuntu 25.10+ ships uutils and is affected. A commenter demonstrated Node's `fs.statSync(...).birthtime` failing with `ELOOP` on a mount while succeeding on the guest's own filesystem in the same process. Practical impact for us: a dev sandbox running Rust or Node tooling against a bind-mounted workspace will hit this, and the failure mode is a confusing `ELOOP` rather than an honest "unsupported".
- **Issue #1218 (open, labelled `Filesystems`/`Windows`, filed 2026-07-27 against 0.6.6 and 0.6.7).** FUSE `FLUSH` is sent on every `close()`, including for read-only handles, and the Windows passthrough backend implements it with `FlushFileBuffers()`, which is documented to fail on a handle without write access. Every `close()` of a read-only file on a bind mount therefore returns `EACCES`. Reads return correct data and writes succeed; only the close fails, which means the symptom is tool-dependent — GNU `cat` reports `Permission denied` after printing the file, Rust's `File` drop swallows it. A fix pull request (#1214) was merged on 2026-07-26 and a commenter linked it, but the issue remains open, so whether the released fix is complete is unverified.

**Related non-Windows evidence that the mount layer is weak under real build tooling.** Issue #949 (closed 2026-07-10) reports Gradle failing unpredictably on a virtiofs passthrough mount because the mount does not support file locking — lock files created on the host were entirely invisible inside the sandbox, leaving host and guest filesystem views desynchronised. `stat-virt=off` did not help. The reporter's workaround was moving the cache onto a disk-image mount. That was a Linux host, but it is the same passthrough design, and it is directly relevant to a sandbox running build tooling against a mounted repository. Issue #1115 (closed) records a memory leak in virtio-fs `readdir`, and #741 (closed) records bind mounts leaking host uid/gid into the guest and blocking non-root workflows.

**Guest-write quota.** Not a bug, but easy to miss: since v0.6.0, "bind mounts default to a 4 GiB guest-write quota that can be overridden per mount", enforced against new file growth only, with guest `df` reporting the budget rather than the host's disk. A sandbox doing a large build into the mounted workspace will hit that ceiling silently unless `quota=` is set explicitly on the mount.

**File watching / inotify propagation — UNKNOWN, and probably absent.** No documentation, changelog entry, issue, or source comment addresses whether host-side writes to a bind mount generate inotify events inside the guest. A search for `inotify` across the repository's issues returns zero results, and searches for `fsnotify`/`watch`/`watcher` return only issue #537, "Support directory watch" (open since 2026-04-11), which requests a *host-side SDK* directory-watch API and is explicitly not about guest-side inotify propagation. The Windows passthrough backend source contains no notify or invalidation machinery. This matters a great deal for us: the sandboxes run dev servers and editors against the mounted workspace, and the general virtiofs design does not propagate host-side changes into guest inotify unless the daemon sends explicit FUSE invalidation notifications. **Treat host-to-guest inotify propagation as not working until measured.** It is the single most important thing for spike stage 3 to test empirically — start a dev server with hot reload inside the sandbox, edit a file from Windows, and see whether the reload fires.

**Performance — UNKNOWN.** No published benchmark, no Windows-specific performance caveat, and no issue reporting slow bind-mount I/O on Windows. Absence of reports is weak evidence given Windows support is six weeks old. Section 6's answer for the ideation document's open question "virtiofs performance for a large repository under a Windows host path is unmeasured" is: still unmeasured, and now with two open correctness bugs stacked on top of it. The `stat-virt=strict|relaxed|off` and `host-perms=private|mirror` mount options exist and are the likely tuning knobs, but the documentation does not explain their performance implications and issue #949 records `stat-virt=off` failing to help with lock-file visibility.

Sources:
- <https://github.com/superradcompany/microsandbox/issues/1219> (`gh issue view 1219 --repo superradcompany/microsandbox --comments`)
- <https://github.com/superradcompany/microsandbox/issues/1218>, and pull request #1214
- <https://github.com/superradcompany/microsandbox/issues/949>, <https://github.com/superradcompany/microsandbox/issues/1115>, <https://github.com/superradcompany/microsandbox/issues/741>
- <https://github.com/superradcompany/microsandbox/issues/537>
- <https://docs.microsandbox.dev/sandboxes/volumes.md>, <https://docs.microsandbox.dev/sandboxes/filesystem.md>
- <https://docs.microsandbox.dev/changelog/2026-06-26.md> (the 4 GiB bind-mount guest-write quota)
- Searches returning nothing: `is:issue inotify` (0 hits), `is:issue windows performance` (2 hits, neither relevant), `is:issue slow performance mount` (1 hit, an OCI image-layer performance issue)

## 7. Default outbound network policy — CONFIRMED, with a critical consequence for our port model

**Verdict: CONFIRMED**, and documented in more detail than `01_ideas.md` assumed.

**Exactly what is blocked.** Egress is deny-by-default with a single allowance: the public internet, plus DNS through the sandbox gateway. The denied set is enumerated precisely: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `100.64.0.0/10`, `fc00::/7` (RFC 1918, carrier-grade NAT, IPv6 ULA); `127.0.0.0/8` and `::1` (loopback); `169.254.0.0/16` and `fe80::/10` (link-local); `169.254.169.254` (cloud metadata, taking precedence over link-local); and the per-sandbox gateway, which is the `host` group. Ingress reaches only published ports, and published ports bind `127.0.0.1` on the Windows host by default. All traffic terminates in a host-side user-space stack with no host kernel routing or NAT in the path.

For the app's needs, the important part is what is *allowed*: the public internet and DNS. Everything the base image and mise need — apt repositories, the mise release host, GitHub, crates.io, npm — is public-internet egress and works under the default policy with no relaxation. That answers the ideation document's open question in the affirmative.

### 7(a) Does the block affect the sandbox's own loopback? No — but published ports still cannot reach a loopback-bound guest server

This has two separate answers, and conflating them is the trap.

**The policy does not interfere with in-guest loopback traffic.** The documentation is unambiguous: "Inside the guest, `localhost` and `127.0.0.1` are the sandbox's own loopback, handled entirely by the guest kernel, so the request never reaches the host stack at all. ... The `Loopback` group exists only to catch the unusual case of a guest crafting a raw packet addressed to `127.0.0.1` out its network interface." A process inside the sandbox connecting to another process inside the same sandbox over `127.0.0.1` is unaffected by the network policy. The networking overview restates it: "`loopback` means the sandbox's own `127.0.0.1`, not your laptop's localhost."

**But a published port dials the guest's interface IP, not the guest's loopback.** This is the finding that actually matters, and it is not in the documentation — it comes from `crates/network/lib/publisher.rs`, whose module header describes "host-side listeners that forward connections into the guest VM via smoltcp". The `PortPublisher` struct carries a field documented as "Guest IP that inbound connections are dialed to. Prefers IPv4 (the common case — **most services bind `0.0.0.0` or dual-stack `::`, both of which accept v4**) and falls back to IPv6 for v6-only sandboxes." Inbound connections are relayed to the guest's network-interface address. A server bound only to `127.0.0.1` inside the guest will not accept them.

Corroborating evidence: the repro in issue #914 (published-port throughput) binds its in-guest test server to `("0.0.0.0", PORT)`, which is the shape every published-port example in the codebase and documentation uses.

**Consequence for this feature.** The app's two primary surfaces bind loopback inside the container today, and Podman's `--network pasta:--host-lo-to-ns-lo` flag — called out in `01_ideas.md` as "Podman-specific and load-bearing for loopback-bound in-container servers" — is what makes them reachable. microsandbox has no equivalent flag. Under `MsbProvider`, either:

- the zellij web terminal (8082) and the paseo agent web UI (6767) must be started bound to `0.0.0.0` rather than `127.0.0.1`, which is a change to `prepare_terminal` and `prepare_agent_web` in `src-tauri/src/runtime/mod.rs` and possibly to how those tools are invoked; or
- an in-guest relay (the `socat` already installed by `src-tauri/resources/Containerfile`) bridges the guest's interface address to the loopback listener, which is a per-port supervised process.

The first option is cleaner but changes the security posture inside the guest — though far less than it sounds, because the microVM boundary is the real isolation and the host-side listener still binds `127.0.0.1` on Windows. **This should be promoted to a first-class item in spike stage 3**, ahead of the base-image question: if the terminal cannot be reached, the provider has no workspace surface.

### 7(b) Can a guest process reach the host? Not by default, and `localhost` is not the route

Denied by default. The host is reachable as the DNS name `host.microsandbox.internal`, which resolves to the per-sandbox gateway, and the gateway is in the denied set under the default policy. The documentation warns explicitly that `curl http://localhost:5432` from inside a sandbox does *not* reach a service on the developer's machine — it reaches the sandbox's own loopback — and that `curl http://host.microsandbox.internal:5432` is dropped at the gateway unless the `host` group is allowed.

For our purposes this default is mostly convenient: nothing the sandbox runs should be reaching back into Windows. It becomes relevant only if a sandboxed agent needs to talk to a host-side service such as a local model server, in which case the `host` profile is the switch.

### 7(c) The exact flags to relax the policy

Composable high-level profiles, repeatable and comma-accepting, via `--net`:

- `msb create python --name devbox --net "public,host"` — public internet plus the host gateway.
- `msb run alpine --net "public,private"` — public internet plus RFC 1918 and the local LAN.
- `public`, `private`, and `host` compose. `all` and `none` are terminal whole-policy choices and cannot be mixed with the three positive profiles. Every non-empty profile set adds narrow DNS access through the gateway automatically, exactly once.
- `--no-net` disables network access entirely (retaining the device but denying traffic both ways).

Low-level rules, for finer control, via `--net-default-egress allow|deny` plus repeatable `--net-rule`, where the first matching rule wins and explicit rules may be placed before generated profile rules to override them. Example from the documentation: `--net-default-egress deny --net-rule "allow@public:tcp:443,allow@dns"`. Rules target the groups `public`, `private`, `host`, `meta`, `dns`, or specific IPs, CIDRs, domains, and port ranges.

Note for a future hardening pass: `--net-rule "deny@host:tcp:22"` composes with `--net host`, so the host group can be opened selectively rather than wholesale.

Sources:
- <https://docs.microsandbox.dev/security/network.md> (default posture, the exact denied CIDR list, the `localhost` warning)
- <https://docs.microsandbox.dev/networking/overview.md> (profiles, `--no-net`, low-level policy syntax, port mapping, `host.microsandbox.internal`)
- <https://docs.microsandbox.dev/cli/sandbox-commands.md> (the `--net`, `--net-rule`, `--net-default-egress`, and `-p` flags)
- <https://raw.githubusercontent.com/superradcompany/microsandbox/main/crates/network/lib/publisher.rs> (`PortPublisher::guest_ip` — published ports dial the guest interface IP)
- <https://github.com/superradcompany/microsandbox/issues/914>

## 8. Windows support maturity — CONFIRMED as preview, six weeks old, with four open Windows defects

**Verdict: CONFIRMED preview status.** "Windows support is currently in preview. Windows 11 is the tested path, and local sandboxes require Windows Hypervisor Platform."

**Timeline.** Windows support landed in v0.6.0 on 2026-06-27 (pull requests #1017 and #1019), described in the changelog for the week of 26 June 2026 as running "as a first-class local host on Windows on top of the libkrun and libkrunfw Windows ports", with a Windows-specific passthrough filesystem backend. Bind-mount support arrived one day later in v0.6.1 (pull request #1055) — it was **not** in the initial Windows release. The latest release is v0.6.8 (2026-07-29); a 0.6.9 version bump is open as pull request #1267. Nineteen issues and pull requests mention Windows in their title, seventeen of them since 2026-06-23. The Windows port is under active weekly development and is roughly six weeks old.

**Documented limitations from the troubleshooting page.**

- Windows 11 is the only tested path; local sandboxes require `HypervisorPlatform`, and a reboot is needed after enabling it.
- If the feature is enabled but the hypervisor still reports unavailable, firmware/UEFI virtualization must be checked and the host rebooted once.
- Stale `MSB_LIBKRUNFW_PATH` or `config.json` overrides referencing `libkrunfw.so` or `libkrunfw.dylib` must be cleared or repointed at `libkrunfw.dll`.
- Publishing a port opens a host listening socket and can raise a Windows Defender Firewall prompt for `msb.exe` on first use. Keep published ports on `127.0.0.1`; only allow private/public network access when intentionally binding beyond loopback.
- Interactive commands must be run from a real console, not a redirected CI shell or background task, so that `msb` can read input, forward resize events, and restore the console on exit. **This is a direct constraint on our app**, which drives child processes with piped stdio from a Tauri backend, and on the e2e harness.
- The v0.6.0 changelog adds a blanket caveat: "Pieces that still depend on Unix-only behavior surface as explicit unsupported errors instead of silently misbehaving." No published enumeration of which pieces those are exists — **UNKNOWN**, and worth capturing empirically in spike stage 2 by exercising every `msb` subcommand `MsbProvider` needs.

**Open issues labelled `Windows` (all four still open at time of writing).**

| Issue | Title | Labels | Filed | Impact on this feature |
|---|---|---|---|---|
| #1230 | Windows PTY attach corrupts UTF-8 console output | `SDK`, `CLI`, `Windows` | 2026-07-28 | Box-drawing characters, symbols, and spinner glyphs render as mojibake through the interactive PTY attach path; the `--no-tty` path is unaffected. Directly relevant to a zellij-based terminal and to any TUI agent session. |
| #1219 | Unimplemented FUSE opcodes answered with the host's `ENOSYS`, read by the guest as `ELOOP` | `Filesystems`, `Investigate`, `Windows` | 2026-07-27 | Every Rust-built binary in the guest breaks on virtiofs mounts. See section 6. |
| #1218 | Closing a read-only file on a bind mount fails with `EACCES` | `Filesystems`, `Windows` | 2026-07-27 | Every read-only `close()` on a bind mount errors. A fix pull request merged 2026-07-26 but the issue is still open. See section 6. |
| #1191 | Custom filesystem backends served from the embedding process | `Filesystems`, `Plugins`, `RFC` | 2026-07-21 | An architecture RFC rather than a defect; noted because it touches the same passthrough layer. |

**Closed Windows issues worth knowing about**, because they establish the shape of failures to expect and the maintainer's response time:

- #1049 (WHP access violation in `winhvplatform.dll` at VM entry, x86-64 only) — filed 2026-06-27, fixed in v0.6.1 the next day.
- #1092 ("sandbox process exited before agent relay became available") — filed 2026-07-03, closed 2026-07-08.
- #1095 (WHP vCPU triple-fault on AMD Ryzen 9800X3D; missing CPUID masking in the WHP backend) — filed and closed 2026-07-04.
- #1085 removed a Visual C++ runtime requirement from Windows installs; #1148 fixed guest paths being mangled by host path rules when sent from a Windows host; #1119 moved the live control channel onto a Windows named pipe; #1274 deferred Windows release swaps until process exit.

The pattern is encouraging on responsiveness — the three hard WHP boot failures were each closed within a day or five — and discouraging on stability: three separate classes of "no sandbox boots at all" on Windows in the six weeks since launch, on three different CPUs. Combined with the project's self-declared beta status and expected breaking changes, this supports the ideation document's leaning toward shipping the provider default-off, behind a settings flag not surfaced in the welcome flow.

Sources:
- <https://docs.microsandbox.dev/troubleshooting/windows.md>
- <https://docs.microsandbox.dev/changelog/2026-06-26.md>, <https://docs.microsandbox.dev/changelog/2026-07-05.md>, <https://docs.microsandbox.dev/changelog/2026-07-10.md>, <https://docs.microsandbox.dev/changelog/2026-07-24.md>
- `gh search issues --repo superradcompany/microsandbox 'label:Windows is:issue'` (7 results, 3 open with the `Windows` label) and `gh search issues --repo superradcompany/microsandbox 'windows in:title'` (19 results)
- `gh release list --repo superradcompany/microsandbox` and `gh release view --repo superradcompany/microsandbox` (latest v0.6.8, 2026-07-29)

## What this changes for the plan in `01_ideas.md`

Not edits to that document — observations for whoever writes `02_dev.md`.

- **Spike stage 1 gets cheaper.** The coexistence question, framed as "the single risk that would end the feature outright", is answered affirmatively by desk research. Stage 1 reduces to confirming this host in particular boots a VM, since `msb doctor` reporting green does not prove that (issue #1095).
- **Spike stage 3 gets a new first item, ahead of the base image**: can a published port reach the zellij web terminal at all? Under the current design it cannot, because zellij binds loopback inside the guest. This is a larger design question than the base image and should be answered before any base-image work.
- **The `MSB_HOME` gamble paid off.** Per-worktree isolation is supported and explicitly designed for, so microsandbox e2e specs should not inherit the host-wide lock. Pin `MSB_PATH` alongside `MSB_HOME` so binary resolution does not fall through to `PATH`.
- **`ProviderCapabilities` needs a `loopback_publish` flag**, or equivalent, distinguishing Podman (which reaches a guest loopback listener via pasta) from microsandbox (which does not).
- **The interactive-console constraint is a real risk to the mock and e2e tiers.** microsandbox documents that interactive commands must run from a real console, and `msb doctor --fix` refuses to run non-interactively without `--yes`. Both bear on how a Tauri backend with piped stdio drives `msb`.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `msb doctor` reports the host ready but every `msb run` fails to boot | `msb doctor` only probes the WHP API surface; it does not start a VM. Three separate CPU-specific WHP boot failures shipped on Windows between v0.6.0 and v0.6.3 | Gate microsandbox readiness on an actual sandbox boot, not on `msb doctor` alone, and treat `msb doctor` as necessary but not sufficient in `MachineManager::readiness()` |
| `msb doctor --fix` succeeds but the next `msb run` still reports the hypervisor unavailable | The fix runs `Enable-WindowsOptionalFeature ... -NoRestart` and never reboots; the feature only takes effect after a restart | Surface a terminal "reboot required" state from the microsandbox `prepare(Channel)` step rather than looping on a re-check |
| `msb doctor --fix` fails immediately when driven from the app or an e2e run | The confirmation helper bails with "non-interactive terminal; pass `--yes` to apply fixes", and the fix itself opens a UAC-elevated PowerShell window | Pass `--yes`, and accept that the UAC consent dialog cannot be suppressed — the step is inherently user-interactive |
| A per-worktree e2e run under a fresh `MSB_HOME` cannot find `msb.exe` or `libkrunfw.dll` | Binary resolution probes `{home}/bin/msb` before falling back to a `PATH` lookup, and a freshly created `MSB_HOME` has no `bin/` or `lib/` | Set `MSB_PATH` to `%USERPROFILE%\.microsandbox\bin\msb.exe` alongside `MSB_HOME` in `e2e/wdio.conf.ts`'s `beforeSession` |
| The zellij terminal or paseo agent UI is unreachable on its published port even though the sandbox is running and the process is listening | Published ports are relayed to the guest's network-interface IP, not the guest's `127.0.0.1`; microsandbox has no equivalent of Podman's `--network pasta:--host-lo-to-ns-lo` | Start the in-guest servers bound to `0.0.0.0`, or bridge the interface address to the loopback listener with the `socat` already in `src-tauri/resources/Containerfile` |
| `ls` in a sandbox reports "Too many levels of symbolic links" on the mounted workspace, and every Rust-built tool fails there | Open issue #1219: on Windows hosts an unimplemented `FUSE_STATX` is answered with the host's `ENOSYS` (40), which the Linux guest reads as `ELOOP` | No upstream fix yet. Prefer a Debian-based guest (GNU coreutils use `fstatat` and are unaffected) and treat Rust and Node tooling on bind mounts as broken until the issue closes |
| Reading a file from the mounted workspace prints the contents and then "Permission denied" | Open issue #1218: FUSE `FLUSH` on a read-only handle calls `FlushFileBuffers()`, which fails on a handle without write access | Track pull request #1214 and re-test on a release after 0.6.7; the data is correct, only `close()` errors |
| A build inside the sandbox fails with no-space errors well before the host disk is full | Bind mounts carry a default 4 GiB guest-write quota since v0.6.0, and guest `df` reports the budget rather than the host disk | Set `quota=` explicitly on the workspace mount when building the `--mount-dir` specification in `MsbProvider::create_instance` |
| A dev server inside the sandbox never hot-reloads when a file is edited from Windows | Host-to-guest inotify propagation over virtiofs is undocumented, untested, and unlikely to work — no notify or invalidation machinery appears in the Windows passthrough backend | Measure it in spike stage 3 before committing to bind mounts; the fallback is `msb ssh serve` plus Mutagen sync, which `01_ideas.md` already records as the alternative |
| A guest process cannot reach a service running on Windows | Default egress policy denies the `host` group, and `localhost` inside the guest is the sandbox's own loopback, not the developer's machine | Create the sandbox with `--net "public,host"` and have the guest use `host.microsandbox.internal`, never `localhost` |
| A TUI or agent session renders box-drawing characters as mojibake | Open issue #1230: the Windows PTY attach path writes guest UTF-8 bytes to the console with `WriteFile` rather than `WriteConsoleW` | No upstream fix yet. The `--no-tty` execution path is unaffected, so prefer it wherever an interactive PTY is not strictly required |
