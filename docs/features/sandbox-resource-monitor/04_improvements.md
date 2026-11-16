---
type: improvements
title: "Sandbox Resource Monitor Improvements"
feature: sandbox-resource-monitor
---
# Sandbox Resource Monitor Improvements

Work noticed while planning the sandbox resource monitor ([02_dev.md](02_dev.md)) and deferred out of its scope.

## Configurable default memory and CPU per provider

Pass `--memory` and `--cpus` to `msb create` in `MsbProvider::create_instance` (`src-tauri/src/provider/msb.rs`), apply them to existing sandboxes with `msb modify`, and decide whether Podman sandboxes get a per-container `--memory` limit in `PodmanProvider::create_instance` (`src-tauri/src/provider/mod.rs`) or guidance on the WSL VM's `.wslconfig` size. Deferred because the right values should come from the usage history the monitor collects, which is the reason the monitor exists.

## Out-of-memory kill detection

Count kernel out-of-memory kills inside each sandbox (a cgroup `memory.events` read for Podman, guest kernel messages for microsandbox) and record them in the history rollups. An out-of-memory kill is the strongest evidence that a sandbox needs more memory. Deferred because both mechanisms need an exec per sandbox, which Resolved Decision 1 of `02_dev.md` avoids on the 10 s tick; a once-a-minute exec is the likely shape.

## Health indicator on the Home page cards

Show the health chip on the sandbox cards in `src/routes/projects/SandboxItem.svelte`. Deferred to keep the first version discrete (Resolved Decision 11 of `02_dev.md`).

## Slower sampling while the window is hidden

Lengthen the tick while the main window is minimized or hidden, keeping the once-a-minute rollup. Deferred until the cost of the 10 s tick is measured.

## Deployed sandbox resources

Sample sandboxes deployed to a remote Podman host by running `podman stats` over the remote connection. Deferred because every remote call crosses SSH and the remote host's capacity is not the local sizing question.

## Disk use

Report each sandbox's workspace volume use (microsandbox disk volumes have a fixed size set in `src-tauri/src/provider/msb.rs`). Deferred because the user asked for CPU and memory first.

## A cache-aware Podman pool figure

Read the Podman machine's `MemAvailable` (from `/proc/meminfo` inside the VM, once a minute, through a quiet monitor call shape) so the Health pool rule of [02_dev.md](02_dev.md) can be turned on. `podman info` reports only `MemFree`, which counts page cache as used ([spike/engine-output.md](spike/engine-output.md)), so the rule is off. Deferred because choosing the call (a `podman machine ssh`, or an exec into a running sandbox, whose `/proc/meminfo` shows the VM's) is a design decision the plan did not make.

## Quieter sandbox-name refresh

The monitor refreshes its sandbox name set every 60 s through each provider's `list_instances`, which logs `podman ps`, `msb ls` and one `msb inspect` per microsandbox sandbox at `info`: about 5 to 15 `mio.log` lines a minute, several thousand a day, which Resolved Decision 7 meant to avoid. Two shapes would fix it: a quiet name-only list call for the monitor, or deriving the running set from the quiet `podman stats` and `msb metrics` calls themselves and refreshing names only when a reading holds an unknown, non-sidecar name. Deferred because both change Resolved Decisions 6 and 10.

## A popover component for the health chip

The repository has no `popover` component, so the health chip's details open in the `dropdown-menu` primitive. Pulling `popover` from the retro-ui-svelte registry needs the user's approval (root `AGENTS.md`, Adding new components). Either way the overlay falls under the agent web UI pane's native webview while that pane is shown; showing the details in the Manage Overview, or hiding the pane while the menu is open, would avoid it.

## One machine-list probe per Podman stats call

Every connection-scoped Podman call, the monitor's included, first runs the shared `podman machine list` probe in `src-tauri/src/process/mod.rs`, so each 10 s Podman sample costs two processes. A short-lived cache of the probe result for monitor calls would halve that. Deferred until the tick's cost is measured.
