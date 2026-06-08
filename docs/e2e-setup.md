# E2E Test Setup

## Overview

E2E tests use WebdriverIO with `tauri-driver` and `msedgedriver`. The test suite lives in `e2e/`, configured by `e2e/wdio.conf.js`. The Tauri app is built in `onPrepare` before any sessions start.

## Podman Machine Isolation

E2E tests run against a dedicated Podman machine (`mio-tests`) separate from the development machine (`podman-machine-default`). Separate machines prevent test containers from appearing in the developer's working environment.

The `mio-tests` machine is initialised and started automatically in `ensurePodmanMachine` inside `e2e/wdio.conf.js`. The `mio-tests` machine must be **rootful** — rootless mode has an incomplete Docker-compat API that causes event streaming and container lifecycle commands to fail.

To convert an existing machine to rootful: `podman machine set --rootful <name>`

## Connecting the App to the Test Machine

The Tauri app connects to Podman via a Windows named pipe. The named pipe path is derived from the `PODMAN_MACHINE` environment variable in `podman_pipe()` (`src-tauri/src/lib.rs`). The path format is `//./pipe/<PODMAN_MACHINE>`.

`e2e/wdio.conf.js` passes `PODMAN_MACHINE=podman-mio-tests` when spawning `tauri-driver` in `beforeSession`. `tauri-driver` propagates `PODMAN_MACHINE` to the Tauri app process. The named pipe for a machine named `mio-tests` is `podman-mio-tests` — note the `podman-` prefix, which differs from the default machine whose pipe is `podman-machine-default`.

To discover active Podman pipe names on the running system: `Get-ChildItem \\.\pipe\ | Where-Object { $_.Name -like "*podman*" }`

## Image Availability

Container images must be pre-pulled onto the `mio-tests` machine before tests run. Use `podman machine ssh` to pull images rather than setting `DOCKER_HOST` — `podman pull` does not reliably respect `DOCKER_HOST` on Windows. Image pre-pulling is handled in `ensurePodmanMachine` (`e2e/wdio.conf.js`) and is idempotent.

## Container Stop Behaviour

`bollard::stop_container` with default options sends SIGTERM then waits up to 10 seconds before sending SIGKILL. Shell containers running Alpine `/bin/sh` ignore SIGTERM, so container stops always take the full 10-second grace period. The `waitForExist` reverse check in `e2e/test/specs/project-start-stop.spec.js` uses a 20-second timeout to account for the 10-second grace period plus Docker event delivery time.

## WebdriverIO Selector Notes

Sub-element queries must be scoped off a parent element reference using `parentEl.$()`. Mixing selector strategies (e.g. partial text `*=` and button text `=`) in a single `$()` string is not supported by WebdriverIO. See `e2e/test/specs/project-start-stop.spec.js` for the scoped selector pattern.

UI status indicator changes are driven by the Docker event stream (`listen_events` in `src-tauri/src/lib.rs`), not by the Tauri IPC command response. Assertions on UI state changes after container operations must use `waitForExist` or `waitForDisplayed` with explicit timeouts rather than synchronous assertions.

---

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `error launching hyper legacy client` | Named pipe path is wrong; Tauri app cannot open the pipe | Check active pipe names with `Get-ChildItem \\.\pipe\`. Verify `PODMAN_MACHINE` in `wdio.conf.js` matches the real pipe suffix |
| `SendRequest` error from bollard | Pipe opens but HTTP request fails; usually caused by rootless mode | Convert the Podman machine to rootful: `podman machine set --rootful <name>` |
| `404 no such image` on container create | Container image not present on the test machine | Pull image via `podman machine ssh <name> podman pull <image>` inside `ensurePodmanMachine` in `wdio.conf.js` |
| Element not found after create | `invoke` is async; UI has not updated before WebdriverIO selector runs | Use `waitForDisplayed` with an explicit timeout on the target element rather than `toBeDisplayed` |
| Compound selector not found | WebdriverIO rejects mixed selector strategies in a single `$()` string | Get the row element with `$()`, then query children with `row.$()` |
| Green dot never disappears after stop | Alpine `/bin/sh` containers ignore SIGTERM; `stop_container` takes the full 10-second grace period | Set `waitForExist` reverse timeout to at least 20 seconds |
| Pull goes to wrong Podman machine via `DOCKER_HOST` | `podman pull` does not reliably honour `DOCKER_HOST` on Windows | Pull images using `podman machine ssh <name> podman pull <image>` instead |
