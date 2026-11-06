---
type: ideas
title: "Sandbox Runtime Management Plan"
feature: sandbox-runtime-management
---
# Sandbox Runtime Management Plan

This document is the ideation for managing the software a user installs inside a sandbox: recording what is installed, and reproducing that software on a remote host when the sandbox is deployed. The sandbox container itself (an `alpine:latest` Podman container) and the local→remote workload migration already exist; the migration today ships an opaque committed image, and there is no record of what software a sandbox contains. The proposed model replaces the image transfer of software with a declarative runtime manifest driven by flox (built on Nix), validated by a spike on 2026-06-22. Relevant files: the backend container lifecycle in `src-tauri/src/backends/mod.rs`, the backend contracts in `src-tauri/src/backends/types.rs`, and the command wiring in `src-tauri/src/lib.rs`. The companion remote-sync feature (`docs/features/remote-sync/01_ideas.md`) covers workspace file sync and is the lifecycle this feature plugs into.

## Current State

A sandbox is a single Podman container created from `alpine:latest`, pinned by `BASE_IMAGE` and `BASE_IMAGE_VERSION` in `src-tauri/src/backends/mod.rs` (lines 14–15) and assembled by `image()` in the same file. `create` in `src-tauri/src/backends/mod.rs` (line 248) runs `podman create` with the base image and `/bin/sh`; it sets up the bind-mounted workspace but installs no software. A user installs software interactively inside the running container (today via Alpine's `apk`, reached over `podman exec`). Nothing records which packages were installed, and the installed set lives only in the container's mutable filesystem layer.

Local→remote migration of installed software already works through `deploy` in `src-tauri/src/backends/mod.rs` (line 319). `deploy` stops the container, runs `podman commit` to snapshot the mutable filesystem into a `sandbox-mio/<name>:deployed` image (line 348), then streams the image to the target connection with `podman save` piped into `podman load` (lines 352–388). The comment at `src-tauri/src/backends/mod.rs` line 345 states that the commit captures installed packages and container state but not the bind-mounted workspace. The committed image is therefore the current software-transfer mechanism, and it is the mechanism this feature replaces.

Workspace files travel separately over the Mutagen sync session, not inside the committed image. `deploy` returns a `RemoteDeployment` (`src-tauri/src/backends/types.rs` line 55) whose `alpha`/`beta` endpoints the `deploy_sandbox` command in `src-tauri/src/lib.rs` hands to the sync engine. The frontend's view of a sandbox is the `Sandbox` struct (`src-tauri/src/backends/types.rs` line 41), which today carries `id`, `name`, `status`, and `workspace_path` — no software record.

## Decision: flox manifest replaces the image transfer of software

The chosen direction (selected 2026-06-22) is that a declarative flox manifest and lockfile become the source of truth for a sandbox's software, and `deploy` stops shipping the committed image for software. Instead `deploy` transfers the manifest and lockfile, and the remote host re-resolves and installs the same package closure from a Nix binary cache. The committed-image path in `deploy` (`src-tauri/src/backends/mod.rs` lines 344–388) is removed or reduced to non-software container state, because the manifest now reproduces the software. The remote-sync workspace transfer is unchanged.

## Runtime Manifest As Source Of Truth

The runtime manifest is the per-sandbox declarative list of installed software plus a lockfile pinning each package to an exact build. flox stores these as `manifest.toml` (human-readable intent) and `manifest.lock` (resolved closure). The manifest is both the record the user sees ("programs installed in this sandbox") and the input the remote re-resolves from, so the record and the reproduction mechanism are the same artifact rather than two things that can drift. The seam that surfaces the record to the frontend is the `Sandbox` struct in `src-tauri/src/backends/types.rs` (line 41), which gains a software-listing field or a sibling command that reads the manifest.

## Installing Software Through A Manifest-Updating Command

Interactive `apk` installs mutate only the container layer and leave no record, so software installation moves behind a new backend operation on the `SandboxBackend` trait (`src-tauri/src/backends/types.rs` line 110) that runs a flox install inside the container over `podman exec` and lets flox update `manifest.toml` and `manifest.lock`. Every install therefore writes the record as a side effect. The same trait is where a corresponding uninstall and a "list installed" operation hang, keeping the manifest authoritative rather than reconstructed after the fact.

## Deploy Reproduces From The Manifest

`deploy` in `src-tauri/src/backends/mod.rs` (line 319) changes from snapshot-and-push to manifest-and-resolve: it transfers `manifest.toml` and `manifest.lock` to the target connection and runs a flox activation/install on the remote container so the remote reproduces the locked closure from the Nix binary cache. The lockfile guarantees the remote resolves the identical builds the laptop has, not merely the latest matching versions. Because the closure is fetched from the binary cache on the remote rather than streamed from the laptop, the transfer is the size of the manifest and lockfile, not the size of the committed image.

## Reproducible Substrate On An Alpine/musl Base

The sandbox base image is Alpine, which uses musl libc and has no glibc (`BASE_IMAGE` in `src-tauri/src/backends/mod.rs` line 14). Nix is the substrate flox builds on, and Nix packages from nixpkgs carry their own glibc inside the Nix store closure, so glibc-linked binaries run on the musl host without a glibc being installed system-wide. The spike confirmed this: a Nix-installed `hello` pulled `glibc-2.42` into `/nix/store` and ran. Keeping the Alpine base is therefore viable and no base-image change is required for the runtime substrate.

## Bootstrapping Nix And flox Into The Sandbox

Nix and flox are not present in the stock `alpine:latest` image, so the sandbox must acquire them before the first install. Two seams can carry the bootstrap: `create` in `src-tauri/src/backends/mod.rs` (line 248) can install the runtime substrate at sandbox creation, or a pre-baked base image (a layer on top of `alpine:latest`) can ship Nix and flox so creation stays fast. The spike installed Nix from Alpine's own `apk` repository (`nix-2.31.5-r0`) inside a rootless container, which is the lightest bootstrap path for the Nix layer; flox itself installs on top of Nix from flox's flake and binary cache.

## Technology Choices With Tradeoffs

### Software provisioning engine: flox

flox is chosen as the provisioning and record engine. flox gives a single human-readable `manifest.toml` as the installed-software record, a `manifest.lock` for reproducible re-resolution, a `flox activate` UX for non-technical users, and FloxHub for optional cross-machine environment sharing. The alternatives evaluated:

- **`podman commit` + `podman image scp` (the current mechanism, `src-tauri/src/backends/mod.rs` lines 344–388)** — rejected as the software path because the committed image is opaque (it yields no readable list of what is installed), the transfer is the full mutated filesystem rather than a manifest, repeated commits accumulate layer bloat, and the image drifts from any intent the user had. It remains the fallback only for non-software container state that the manifest does not capture.
- **Plain Nix profiles (no flox)** — viable and proven in the spike (`nix profile add` installed and ran a package, and `nix profile list` printed a locked, reproducible record with a `narHash`). Rejected as the primary choice because it lacks flox's single `manifest.toml` intent file and `flox activate` UX, and has a weaker cross-machine story than FloxHub. Retained as the fallback engine if flox's binary-cache dependency proves too fragile (see Validation and Open Questions).
- **An `apk` package list as the manifest** — rejected because Alpine `apk` pins to mirror state rather than to an immutable lock, so a re-install on a remote at a later time can resolve different versions; it is Alpine-only and produces no closure/lockfile, so it cannot guarantee the reproducibility the deploy step needs.
- **A regenerated Containerfile/Dockerfile rebuilt per change** — rejected because a rebuild is not incremental and is slow, and a build-time recipe does not match the interactive "install software as you please" model the sandbox is meant to offer.

### Base image: keep Alpine/musl

Alpine is kept as the base image rather than switching to a glibc distribution (Debian, Ubuntu). A glibc base would remove any musl edge cases, but the spike showed Nix packages bring their own glibc closure and run on musl, so a base change buys little while enlarging the base image and diverging from the existing `create` path. The base-image decision is revisited only if a concrete musl incompatibility surfaces during the build.

### Nix substrate install: Alpine `apk` package

The Nix substrate is installed from Alpine's `apk` repository (`nix-2.31.5-r0`, confirmed in the spike) rather than the upstream Nix installer script. The upstream multi-user installer expects systemd and a daemon, which a rootless container does not have; the `apk` package is musl-native and installs without a daemon. The single-user Nix it provides requires `sandbox = false` in the Nix configuration inside the container, because nested user namespaces for Nix's own build sandbox are not available in the rootless Podman container (see Common Gotchas).

## Validation

A feasibility spike was run by hand on 2026-06-22 inside a rootless `alpine:latest` Podman container (`podman run -d alpine:latest`, driven over `podman exec`) on the local `podman-machine-default` connection. The spike answered the central unknown — whether the Nix/flox substrate runs inside the actual sandbox base image — and recorded which risks cleared.

Cleared risks:

- **musl base has no glibc** — confirmed the risk exists (`/lib/ld-musl-x86_64.so.1` present, no `/lib/x86_64-linux-gnu/libc.so*`), then cleared it: a Nix-installed package carries its own glibc. Command: `nix profile add nixpkgs#hello` fetched `glibc-2.42` into `/nix/store` and `hello` printed `Hello, world!`.
- **Nix installable in a rootless Alpine container** — cleared. Command: `apk add nix` installed `nix-2.31.5-r0` and `nix --version` reported `nix (Nix) 2.31.5`.
- **Rootless Nix can build/install** — cleared after setting `sandbox = false` and `experimental-features = nix-command flakes` in `/etc/nix/nix.conf`. Without `sandbox = false`, Nix's build sandbox cannot create nested namespaces in the rootless container.
- **A reproducible record exists** — cleared. Command: `nix profile list` printed the package with a locked flake URL and `narHash`, the per-sandbox record this feature needs.

Blocker (not cleared in the spike window):

- **flox CLI itself did not install.** flox's flake builds from source unless its binary cache supplies the closure; building from source first failed for lack of `git` (`error: Failed to fetch git repository https://github.com/flox/httpmock.git : error: executing 'git': No such file or directory`). After `apk add git` and configuring flox's substituter, the binary-cache fetch failed twice: first with `error: unable to download 'https://cache.flox.dev/<hash>.narinfo': HTTP error 503`, then, after `https://cache.flox.dev/nix-cache-info` returned HTTP 200, with `error: download of 'https://cache.flox.dev/<hash>.narinfo' was interrupted` alongside `warning: ignoring substitute ... as it's not signed by any of the keys in 'trusted-public-keys'`. The blocker is flox binary-cache availability plus trusted-key configuration, not a fundamental incompatibility with the Alpine/rootless sandbox; the underlying Nix substrate flox depends on is proven working.

The validation therefore confirms the substrate and the reproducible-record model, and isolates the remaining flox-specific work to its binary-cache substituter and trusted-key configuration.

## Open Questions

- **flox versus plain Nix as the shipped engine.** The spike proved plain Nix profiles work end-to-end inside the sandbox but left the flox CLI uninstalled because of a `cache.flox.dev` outage. Whether the build commits to flox (and hardens against its binary-cache availability) or ships plain Nix profiles as the engine while keeping flox's manifest format is undecided; `02_dev.md` resolves it after a flox install clears against a healthy `cache.flox.dev`.
- **Where the manifest lives and how it reaches the remote.** Whether `manifest.toml`/`manifest.lock` live inside the synced workspace (so the existing Mutagen session in the remote-sync feature carries them for free), in a dedicated synced location separate from user files, or are transferred by a dedicated step inside `deploy` (`src-tauri/src/backends/mod.rs` line 319) is undecided.
- **Reconciling pre-existing `apk` installs.** Whether interactive `apk` installs are disabled in favour of the manifest-updating install command, or tolerated and reconciled into the manifest, is undecided; tolerating them reintroduces the drift the manifest is meant to remove.
- **Bootstrap location.** Whether Nix and flox are installed by `create` (`src-tauri/src/backends/mod.rs` line 248) at sandbox creation, or baked into a base image layered on `alpine:latest` so `BASE_IMAGE` (`src-tauri/src/backends/mod.rs` line 14) points at the pre-baked image, is undecided; baking trades a larger base image for faster creation.
- **FloxHub dependency.** Whether cross-machine reproduction relies on FloxHub or stays self-contained with a transferred `manifest.lock` plus a configured Nix binary cache is undecided.
- **Manifest in the destroy guard.** Whether the manifest counts as unsynced work that the remote-sync destroy guard (`docs/features/remote-sync/01_ideas.md`) must protect before a remote sandbox is destroyed is undecided.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Installed software lost or unrecorded after a sandbox is recreated | Interactive `apk` installs mutate only the container's filesystem layer and write no record | Install software through the manifest-updating backend operation on the `SandboxBackend` trait (`src-tauri/src/backends/types.rs` line 110) so every install writes `manifest.toml`/`manifest.lock` |
| Deploy transfers a large image when only software changed | `deploy` snapshots the filesystem with `podman commit` and streams it via `podman save`/`podman load` (`src-tauri/src/backends/mod.rs` lines 344–388) | Replace the software-transfer path with manifest-and-resolve: transfer `manifest.toml`/`manifest.lock` and have the remote re-resolve the locked closure from a Nix binary cache |
| `nix` build fails with a namespace/sandbox error inside the sandbox | Rootless Podman containers cannot create the nested user namespaces Nix's build sandbox needs | Set `sandbox = false` (and `experimental-features = nix-command flakes`) in `/etc/nix/nix.conf` inside the container before running Nix |
| glibc-linked Nix packages assumed broken on the Alpine/musl base | The Alpine base image (`BASE_IMAGE` in `src-tauri/src/backends/mod.rs` line 14) has musl and no system glibc | No fix needed: Nix packages carry their own glibc in the `/nix/store` closure and run on musl, confirmed by the 2026-06-22 spike |
| flox flake install fails with `executing 'git': No such file or directory` | The flox flake fetches Git inputs (e.g. `github.com/flox/httpmock.git`) and stock `alpine:latest` has no `git` | Install `git` (`apk add git`) before installing flox from its flake |
| flox install fails with `HTTP error 503` or `download ... was interrupted` from `cache.flox.dev` | flox's binary cache was unavailable/unstable during the install, so Nix could not substitute flox's prebuilt closure | Retry against a healthy `cache.flox.dev` (probe `https://cache.flox.dev/nix-cache-info` for HTTP 200), or fall back to plain Nix profiles as the engine |
| flox cache substitutes ignored as `not signed by any of the keys in 'trusted-public-keys'` | flox's public key is not in the container's trusted-public-keys, and a flake-supplied key needs explicit acceptance | Add flox's public key to `trusted-public-keys` in `/etc/nix/nix.conf` (not only `extra-trusted-public-keys` via flake config) so substitutes from `cache.flox.dev` are trusted |
| Reproducing software on the remote resolves different versions than the laptop | Re-resolution used package names/intent without the lockfile, so the remote picked the latest matching builds | Transfer and resolve from `manifest.lock` in `deploy` (`src-tauri/src/backends/mod.rs` line 319) so the remote reproduces the identical locked closure |
