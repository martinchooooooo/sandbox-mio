---
type: spike
title: "Spike: Engine Resource Output"
feature: sandbox-resource-monitor
---
# Spike: Engine Resource Output

This spike captures what `podman stats`, `podman info`, `msb metrics` and `msb inspect` report for a running sandbox, so the Phase B parsers of [02_dev.md](../02_dev.md) are written against real output. The spike ran on 2026-11-17 on the Windows 11 development host with the bundled Podman client 5.6.2 against the `podman-machine-default` WSL machine, and the bundled `msb` 0.7.12 against the default `~/.microsandbox` store. Trimmed captures are the parser fixtures under `src-tauri/src/provider/fixtures/`.

## Setup

- **Podman:** a throwaway container `resmon-spike-podman` from `localhost/sandbox-mio/base:7` with `sleep infinity` as its entrypoint and no memory flag, like every app-created Podman sandbox. A pre-existing user container (`ggggg`) was running and appears in the multi-instance capture; it was not touched.
- **microsandbox:** a throwaway sandbox `resmon-spike-msb` from `sandbox-mio/base:7` with no `--memory` or `--cpus` flag, like `MsbProvider::create_instance`, and `--workdir /root` because the app's workspace volume was not mounted.
- **Load:** a CPU load of busy shell loops (two in Podman, one in microsandbox), then an anonymous-memory load (`tail` buffering a long newline-free stream: 1.5 GB in Podman, 300 MB in microsandbox), then a file written to disk to fill the page cache (1000 MB in Podman, 100 MB in microsandbox).
- Both throwaway instances were removed afterwards; `msb ls` reports no sandboxes and `podman ps -a` lists only `ggggg`.

## Host and WSL State

- The host has no `memory=` or `processors=` key in `%USERPROFILE%\.wslconfig` (the file sets only `nestedVirtualization=true`), so WSL sizes every VM by its defaults.
- `podman machine list` reports `podman-machine-default` with 4 CPUs and 2 GiB of memory, the values recorded by `podman machine init`.
- Inside the VM, `free -m` reports 15764 MiB total, and `nproc` reports 8.

## Podman

### `podman stats --no-stream --format json`

Human-formatted strings only: `cpu_time` (`"12.365229s"`), `cpu_percent`, `avg_cpu`, `mem_usage` (`"1.155MB / 16.53GB"`), `mem_percent`, `net_io`, `block_io`, `pids`. Every number is a rounded display string, so this shape is not parsed.

### `podman stats --no-stream --format '{{json .}}'`

The raw-struct template prints one JSON object per running container per line, with numeric fields. The fields the monitor uses:

| Field | Meaning, as observed |
|---|---|
| `Name` | Container name (the sandbox name) |
| `CPUNano` | Cumulative CPU time in nanoseconds since the container started |
| `SystemNano` | The engine's wall clock in nanoseconds since the Unix epoch at sampling time |
| `CPU`, `AvgCPU` | Percentage of one CPU averaged since container start; 37 % while two loops had been busy for a few seconds of a 30-second-old container |
| `MemUsage` | Memory in use, in bytes, excluding inactive file cache |
| `MemLimit` | 16529981440, the whole VM's memory, since the container has no limit |

- **CPU:** two busy loops raised `CPUNano` by 10.74 s across a `SystemNano` delta of 5.37 s, that is 2.0 CPUs. The fraction of allotted CPUs is the `CPUNano` delta divided by the `SystemNano` delta and by the machine's CPU count. `CPU` is unusable as a live reading, as Resolved Decision 4 expected.
- **Memory with cache:** with 1.58 GB of anonymous memory held, writing a 1000 MB file raised the cgroup's `file` counter by 1.05 GB but `MemUsage` only from 1584451584 to 1615650816 bytes. `MemUsage` therefore excludes the freshly written (inactive) file cache.
- **Limit:** `MemLimit` equals `podman info`'s `memTotal`, confirming Resolved Decision 2.

### `podman info --format json` and `--format '{{json .Host}}'`

The full document has `host.cpus`, `host.memTotal`, `host.memFree`, `host.swapTotal` and `host.swapFree`. The `{{json .Host}}` template returns the `host` object alone with the same camelCase keys, which keeps the monitor's call shape distinct from the existing `info --format json` callers.

- `cpus` is 8 and `memTotal` is 16529981440 (15764 MiB), matching the VM's `nproc` and `free -m`, not the 4 CPUs and 2 GiB of `podman machine list`. `podman info` reflects the real WSL VM.
- `memFree` is the kernel's `MemFree`, not `MemAvailable`. With the loads running, `memFree` was 12990918656 while `/proc/meminfo` in the VM reported `MemFree` 12680376 kB (12.98 GB) and `MemAvailable` 13887992 kB (14.22 GB). `memFree` therefore counts reclaimable page cache as used, and understates what the VM can still give out.

### Not captured

The output of `podman stats --no-stream --format '{{json .}}'` with no running container was not captured, because the only other running container belonged to the user. The parser treats empty output as no instances.

## microsandbox

### `msb metrics --format json`

With no name, a JSON array with one object per running sandbox; with none running, `[]`. The fields the monitor uses:

| Field | Meaning, as observed |
|---|---|
| `name` | Sandbox name |
| `vcpu_time_ns` | Cumulative guest vCPU time in nanoseconds |
| `timestamp` | RFC 3339 sampling time, millisecond precision |
| `cpus` | Allotted vCPUs |
| `cpu_percent` | Live percentage over msb's own 1 s sample interval (`metrics_sample_interval_ms` in `msb inspect`) |
| `memory_bytes` | Memory in use: `memory_limit_bytes` minus `memory_available_bytes` |
| `memory_available_bytes` | The guest kernel's available memory |
| `memory_limit_bytes` | The microVM's memory size |
| `memory_host_resident_bytes` | Host-resident size of the VM, including guest page cache |

- **CPU:** one busy loop raised `vcpu_time_ns` by 4.83 s across 5.10 s of `timestamp`, that is 0.95 of the 1 vCPU, while `cpu_percent` read 88 % and 96 %. Both fields are live; the monitor uses the cumulative one, as Resolved Decision 4 prefers.
- **Memory with cache:** holding 300 MB raised `memory_bytes` to 379256832; writing a 100 MB file then raised it only to 386412544, while `memory_host_resident_bytes` rose from 371601408 to 481046528. `memory_bytes` excludes reclaimable cache; `memory_host_resident_bytes` includes it.
- `memory_bytes` counts against the full 512 MiB limit, though the guest kernel sees 482 MiB total, so an idle sandbox reads about 60 MB in use.

### `msb inspect <name> --format json`

`active_config.resources` is `{"cpus": 1, "max_cpus": 1, "memory_mib": 512, "max_memory_mib": 512}`. **The default microsandbox sandbox has 512 MiB of memory and 1 vCPU.**

## Consequences for the Plan

- Phase B parses `podman stats --no-stream --format '{{json .}}'`, `podman info --format '{{json .Host}}'` and `msb metrics --format json`, and derives CPU from `CPUNano`/`SystemNano` and `vcpu_time_ns`/`timestamp` deltas.
- Both engines' memory-in-use figures exclude reclaimable cache, so the history records the cache-excluding definition for both providers (Resolved Decision 5).
- `podman info` has no available-memory figure. Its `memFree` counts page cache as used, so a pool rule of "under 10 % available" fed from `memFree` would mark every Podman sandbox strained whenever the VM's page cache fills, which a build or a large checkout does. The pool rule in Health (`02_dev.md`) cannot be fed as planned; see the Phase B and Phase D sections of `02_dev.md`.
- With 512 MiB and 1 vCPU, a microsandbox sandbox is far smaller than a Podman sandbox's share of a 15 GiB VM; the history will answer whether that is enough.
