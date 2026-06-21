# E2E Testing

This document covers how end-to-end tests are written and run in this project. Spec files live in `e2e/test/specs/`, harness configuration lives in `e2e/wdio.conf.js`, and shared helpers live in `e2e/lib/`.

## Two Test Styles

The repository contains two distinct styles that must not be confused.

**App-driven WebdriverIO specs** (`e2e/test/specs/*.spec.js`) drive the real Tauri application through `tauri-driver`. These specs exercise actual Tauri commands and are the authoritative lifecycle tests. All new feature coverage belongs here.

**Standalone transport tests** (`e2e/test/remote-workspace-sync.test.js`) run a vendored Mutagen binary directly against a Podman machine without involving the application. These tests prove only the sync transport in isolation and are kept as a fast smoke test, not as application coverage.

## Harness Setup

`e2e/wdio.conf.js` provisions infrastructure in `onPrepare` before any spec runs and tears it down in `onComplete` afterwards. `onPrepare` calls `provisionMachine` from `e2e/lib/podman-machine.js` for each required Podman machine, builds the debug application with `pnpm tauri build --debug --no-bundle`, and sets `SANDBOX_WORKSPACE_ROOT` to an isolated directory under `$HOME` so specs never touch the developer's real Documents folder. `onComplete` removes the workspace root, strips the application-written `Include sandbox-mio/config` line from `~/.ssh/config`, deletes `~/.ssh/sandbox-mio/config`, and stops or removes the remote machine depending on `MIO_REMOTE_KEEP` and `MIO_REMOTE_REMOVE`.

The `mochaOpts.timeout` in `e2e/wdio.conf.js` is set to 180 000 ms because the deploy flow involves an image save/load pipe and Mutagen session startup that together can exceed 60 seconds.

## Assertions

All plain JS value assertions use the `assert(condition, message)` helper defined at the top of each spec file rather than `expect().withMessage()`.

Assertion messages follow the pattern: state the observed value, name the expected value, then explain what application condition could cause the discrepancy. Example message shape: `"container is in state '${state}' instead of 'running'. Maybe …"`. This gives the reader the actual value and a starting point for diagnosis without having to re-run the test.

`waitForDisplayed` and `waitForExist` timeouts are set per-element, not globally, so each wait reflects the expected latency of that specific operation (e.g. 120 000 ms for deploy, 30 000 ms for undeploy, 10 000 ms for UI navigation).

## Interacting with Elements Near the Bottom of the Page

The projects page in `src/routes/projects/+page.svelte` has a `position: fixed` bottom bar containing the Event Log button. As more sandboxes accumulate across specs in a single session, list items near the bottom of the viewport fall under the fixed bar and become unclickable, producing `element click intercepted`. Every button click inside a project row must be preceded by `scrollIntoView()` on the target element before calling `click()`. This applies to start, stop, deploy, confirm-deploy, undeploy, destroy, and sync-toggle buttons in all spec files.

## Podman Machine Helpers

`e2e/lib/podman-machine.js` provides `provisionMachine`, `stopMachine`, `removeMachine`, `startPodmanSocket`, `waitForConnection`, and `connectionInfo`.

`provisionMachine` is idempotent: it inits the machine if absent, starts the machine if stopped, activates the rootless Podman API socket via `startPodmanSocket`, polls readiness with `waitForConnection`, and pre-pulls Alpine. The socket is launched with `setsid` and `--time=0` so it survives the SSH channel closure and runs indefinitely.

`startPodmanSocket` must be called with `shell: false`. The launch script contains shell metacharacters (`||`, `&&`, `>`, `$(...)`). Passing the script through `cmd.exe` via `shell: true` causes the metacharacters to be interpreted by `cmd.exe` before the SSH client receives them, breaking the remote shell command.

Podman machine SSH ports are assigned dynamically at each boot. `connectionInfo` in `e2e/lib/podman-machine.js` reads the current port from `podman system connection list` every call rather than caching it, for the same reason `remote_connection_info` in `src-tauri/src/backends/mod.rs` does.

## Remote Deploy Spec

`e2e/test/specs/deploy-remote.spec.js` tests the full deploy/undeploy/destroy lifecycle using `mio-tests-remote` as the target Podman machine. The spec is sequential: each `it` block depends on the state left by the previous one. The `after` block force-removes containers on both `mio-tests` and `mio-tests-remote` as best-effort cleanup so a failed run does not leave residue.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `element click intercepted: Element <button> is not clickable … Other element would receive the click: <div class="flex justify-end …">` | The fixed Event Log bar in `src/routes/projects/+page.svelte` overlaps project list rows near the bottom of the viewport as the sandbox list grows across specs. | Call `scrollIntoView()` on the target button before calling `click()` in every spec that interacts with project row buttons. |
| `TypeError: expect(...).withMessage is not a function` | `withMessage` in `expect-webdriverio` is only supported on async element matchers, not on plain JS boolean or string values. | Use the `assert(condition, message)` helper defined at the top of each spec file instead of chaining `withMessage` onto a plain-value `expect`. |
| `podman --connection mio-tests-remote …` fails right after machine start | The rootless `podman system service` socket is not auto-started on the WSL image used for Podman machines. | Call `provisionMachine` from `e2e/lib/podman-machine.js` before issuing any connection-qualified Podman command. `provisionMachine` runs `startPodmanSocket` and polls until the connection is ready. |
| `deploy_sandbox` fails at `engine.create` in the e2e build | `pnpm tauri build --debug --no-bundle` does not fetch the Mutagen sidecar. | Stage the sidecar at `src-tauri/binaries/mutagen-x86_64-pc-windows-msvc.exe` before running the build. The sidecar path is printed by `pnpm tauri build` when `externalBin` is configured in `src-tauri/tauri.conf.json`. |
| Remote container still exists on `mio-tests-remote` after undeploy | `undeploy_sandbox` in `src-tauri/src/lib.rs` terminated the Mutagen session before calling `remove_remote`, causing the machine's SSH server to be briefly unresponsive when `podman rm` was attempted. | `undeploy_sandbox` must call `store.remove_remote` before `engine.terminate` so the remote container is removed while the Mutagen SSH connection to the machine is still active. |
| `podman image scp conn1::image conn2::` fails with `lookup <name>: Temporary failure in name resolution` | Podman treats connection names as DNS hostnames in the dual-remote `image scp` form; two-Podman-machine transfers are unsupported this way on Windows. | The `deploy` method in `src-tauri/src/backends/mod.rs` uses `podman save` piped to `podman load` between the two connections instead of `image scp`. |
| `remote_mkdir` fails intermittently on a freshly booted target machine | `ssh -o BatchMode=yes` without `StrictHostKeyChecking=accept-new` fails closed when the machine's dynamic SSH port changes between boots, making the cached `known_hosts` entry invalid. | `remote_mkdir` in `src-tauri/src/backends/mod.rs` passes `-o StrictHostKeyChecking=accept-new` alongside `BatchMode=yes`. |
