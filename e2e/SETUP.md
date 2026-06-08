# E2E Test Setup

## Overview

Tests use WebdriverIO with `tauri-driver` and `msedgedriver`. The test suite lives in `e2e/`, configured by `e2e/wdio.conf.js`. The Tauri app is built in `onPrepare` before any sessions start.

## Podman Machine Isolation

Tests run against a dedicated Podman machine (`mio-tests`) separate from the development machine (`podman-machine-default`). This prevents test containers from appearing in the developer's working environment.

The machine is initialised and started automatically in `ensurePodmanMachine` inside `wdio.conf.js`. The machine must be **rootful** — rootless mode has an incomplete Docker-compat API that causes event streaming and container lifecycle commands to fail.

To convert an existing machine: `podman machine set --rootful <name>`

## Connecting the App to the Test Machine

The Tauri app connects to Podman via a Windows named pipe. The pipe name is derived from the `PODMAN_MACHINE` environment variable in `podman_pipe()` (`src-tauri/src/lib.rs`). The format is `//./pipe/<PODMAN_MACHINE>`.

`wdio.conf.js` passes `PODMAN_MACHINE=podman-mio-tests` when spawning `tauri-driver` in `beforeSession`. `tauri-driver` propagates this to the Tauri app process. The actual pipe name for a machine named `mio-tests` is `podman-mio-tests` — note the `podman-` prefix, which differs from the default machine whose pipe is `podman-machine-default`.

To discover pipe names on the running system: `Get-ChildItem \\.\pipe\ | Where-Object { $_.Name -like "*podman*" }`

## Image Availability

Container images must be pre-pulled onto the test machine. Use `podman machine ssh` rather than `DOCKER_HOST` env var tricks — `podman pull` does not reliably respect `DOCKER_HOST` on Windows. The pull is done in `ensurePodmanMachine` and is idempotent.

## Container Stop Behaviour

The default `stop_container` call sends SIGTERM then waits up to 10 seconds before SIGKILL. Shell containers (like the Alpine `/bin/sh` used here) ignore SIGTERM, so stops always take the full 10 seconds. The test's `waitForExist` for the green dot disappearing uses a 20-second timeout to account for this.

## WebdriverIO Selector Notes

Sub-element queries must be scoped off a parent element reference using `parentEl.$()`. Mixing selector strategies (e.g. partial text `*=` and button text `=`) in a single `$()` string does not work. See `project-start-stop.spec.js` for the pattern used.

Status indicator changes are driven by the Docker event stream (`listen_events` in `lib.rs`), not by the IPC command response. Waits for UI state changes must use `waitForExist` or `waitForDisplayed` with appropriate timeouts rather than synchronous assertions.

---

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `error launching hyper legacy client` | Named pipe path is wrong; app cannot open the pipe | Check actual pipe names with `Get-ChildItem \\.\pipe\`. Verify `PODMAN_MACHINE` matches the real pipe suffix |
| `SendRequest` error from bollard | Pipe opens but HTTP request fails; usually rootless mode | Convert machine to rootful: `podman machine set --rootful <name>` |
| `404 no such image` on container create | Image not present on test machine | Pull via `podman machine ssh <name> podman pull <image>` in `ensurePodmanMachine` |
| Element not found after create | `invoke` is async; UI hasn't updated before selector runs | Use `waitForDisplayed` with an explicit timeout rather than `toBeDisplayed` |
| Compound selector not found | WebdriverIO rejects mixed selector strategies in one string | Chain selectors: get the row with `$()`, then call `row.$()` for children |
| Green dot never disappears after stop | Shell containers ignore SIGTERM; stop takes 10 s | Set `waitForExist` reverse timeout to at least 20 s |
| `toHaveTextContaining` not a function | Not a valid WebdriverIO matcher | Use `toHaveText(/pattern/)` with a regex |
| Pull goes to wrong machine via `DOCKER_HOST` | `podman pull` does not reliably honour `DOCKER_HOST` on Windows | Use `podman machine ssh <name> podman pull <image>` instead |
