# E2E Test Setup

## Overview

E2E tests use WebdriverIO talking to `msedgedriver` directly, without `tauri-driver` (`docs/features/microsandbox-provider/03_e2e.md`, Step 0). The test suite lives in `e2e/`, configured by `e2e/wdio.conf.js`. The Tauri app is built in `onPrepare` before any sessions start.

## Podman Machine Isolation

E2E tests run against a dedicated Podman machine (`mio-tests`) separate from the development machine (`podman-machine-default`). Separate machines prevent test containers from appearing in the developer's working environment.

The `mio-tests` machine is initialised and started automatically in `ensurePodmanMachine` inside `e2e/wdio.conf.js`. Rootless mode is sufficient — the app talks to Podman's native API through the `podman` CLI (not the Docker-compat API), so the previous rootful requirement no longer applies.

## Connecting the App to the Test Machine

The Tauri app drives Podman by invoking `podman --connection <name>`, where `<name>` is a Podman **system connection**. The connection is selected by the `PODMAN_CONNECTION` environment variable (`src-tauri/src/backends/`), defaulting to `podman-machine-default`. Podman performs the SSH transport to the connection's socket, so the same code path serves the local machine and any remote host.

`e2e/wdio.conf.js` passes `PODMAN_CONNECTION=mio-tests` when spawning `msedgedriver` in `beforeSession`. `msedgedriver` propagates the variable to the Tauri app process. `podman machine init mio-tests` creates a connection named `mio-tests` automatically.

To list available connections and their SSH URIs: `podman system connection list`

## Image Availability

Container images must be pre-pulled onto the `mio-tests` machine before tests run. Use `podman machine ssh` to pull images rather than setting `DOCKER_HOST` — `podman pull` does not reliably respect `DOCKER_HOST` on Windows. Image pre-pulling is handled in `ensurePodmanMachine` (`e2e/wdio.conf.js`) and is idempotent. The backend also passes `--pull=missing` on create as a fallback.

## Workspace Directory Isolation

Each sandbox gets a host directory bind-mounted into the container at `/root/work`. The root is selected by `SANDBOX_WORKSPACE_ROOT` (`src-tauri/src/backends/`), defaulting to `<Documents>/sandbox-mio`. `e2e/wdio.conf.js` sets `SANDBOX_WORKSPACE_ROOT` to `~/.sandbox-mio-e2e` so tests do not write into the developer's real Documents folder, and cleans that directory in `onPrepare` and `onComplete`. The root is kept under `$HOME` because the podman machine only auto-mounts the user profile into its VM, which the bind mount requires.

`e2e/test/specs/workspace-mount.spec.js` asserts three layers: the host directory exists after create, the container reports a mount at `/root/work` (`podman inspect`), and a file written on the host is readable inside the running container (`podman exec`). The spec reads `SANDBOX_WORKSPACE_ROOT` from its own environment, which the WebdriverIO worker inherits from the launcher process.

## Container Stop Behaviour

`podman stop` sends SIGTERM then waits up to 10 seconds before sending SIGKILL. Shell containers running Alpine `/bin/sh` ignore SIGTERM, so container stops always take the full 10-second grace period. The `waitForExist` reverse check in `e2e/test/specs/project-start-stop.spec.js` uses a 20-second timeout to account for the 10-second grace period plus event delivery time.


## WebdriverIO Selector Notes

Sub-element queries must be scoped off a parent element reference using `parentEl.$()`. Mixing selector strategies (e.g. partial text `*=` and button text `=`) in a single `$()` string is not supported by WebdriverIO. See `e2e/test/specs/project-start-stop.spec.js` for the scoped selector pattern.

UI status indicator changes are driven by the Podman event stream (`listen_events`, which tails `podman events --format json`), not by the Tauri IPC command response. Assertions on UI state changes after container operations must use `waitForExist` or `waitForDisplayed` with explicit timeouts rather than synchronous assertions.

---

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `Error: unable to find connection named ...` | `PODMAN_CONNECTION` names a connection that does not exist | Run `podman system connection list`; ensure the machine was created (`podman machine init mio-tests` registers the `mio-tests` connection) |
| Commands hang or time out | The connection's Podman socket is not listening on the target | Confirm the machine is running (`podman machine start <name>`). Verify the socket: `podman machine ssh <name> "test -S /run/user/1000/podman/podman.sock && echo OK || echo MISSING"`. If missing, the `podman system service` background job was killed on SSH teardown — the `bash -c '... & disown'` pattern in `ensurePodmanMachine` handles this. |
| `404 no such image` on container create | Container image not present on the test machine | Pull image via `podman machine ssh <name> podman pull <image>` inside `ensurePodmanMachine` in `wdio.conf.js`; the backend also passes `--pull=missing` |
| Element not found after create | `invoke` is async; UI has not updated before WebdriverIO selector runs | Use `waitForDisplayed` with an explicit timeout on the target element rather than `toBeDisplayed` |
| Compound selector not found | WebdriverIO rejects mixed selector strategies in a single `$()` string | Get the row element with `$()`, then query children with `row.$()` |
| Container exits immediately after start | Shell containers need a TTY to stay alive; without `--tty` `/bin/sh` exits as soon as it has no terminal | The backend passes `--tty` on create; verify with `podman inspect <name> --format '{{.Config.Tty}}'` |
| Green dot never disappears after stop | Alpine `/bin/sh` containers ignore SIGTERM; `podman stop` takes the full 10-second grace period | Set `waitForExist` reverse timeout to at least 20 seconds |
| Pull goes to wrong Podman machine via `DOCKER_HOST` | `podman pull` does not reliably honour `DOCKER_HOST` on Windows | Pull images using `podman machine ssh <name> podman pull <image>` instead |
| Workspace dir missing or mount round-trip fails | `SANDBOX_WORKSPACE_ROOT` points outside `$HOME`, so the podman machine does not mount it into the VM | Keep the root under the user profile; `wdio.conf.js` uses `~/.sandbox-mio-e2e` |
| `SANDBOX_WORKSPACE_ROOT` undefined in the spec | Worker process did not inherit the launcher env | The variable is set at module scope in `wdio.conf.js` so forked workers inherit it; do not set it inside a hook only |
