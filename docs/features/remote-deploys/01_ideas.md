---
type: ideas
title: "Remote Deploys"
feature: remote-deploys
---
# Remote Deploys

This document covers the design for deploying a sandbox from the local Podman machine to a remote Podman host. The deploy flow itself — stopping the local container, provisioning the remote workspace and container, and handing off to the sync engine — is already implemented in `deploy` in `src-tauri/src/backends/mod.rs`. The gap this document captures is base-image provisioning on the remote: the mise base image (`sandbox-mio/base:mise`) is never published to a registry, so when the remote connection lacks it `podman create` falls back to a registry pull that fails. The fix (`ensure_image_on` in `src-tauri/src/backends/mod.rs`, called from `deploy` before `podman create`) closes that gap. The companion sandbox-runtime-management document (`docs/features/sandbox-runtime-management/01_ideas.md`) covers the mise engine and software reproduction; the remote-sync document (`docs/features/remote-sync/01_ideas.md`) covers workspace file sync via Mutagen.

## Current State

`deploy` in `src-tauri/src/backends/mod.rs` is the entry point for moving a sandbox from the local machine to a remote Podman host. It stops the local container, calls `resolve_workspace` to determine the remote bind-mount source and SSH endpoint, creates the remote workspace directory over SSH with `remote_mkdir`, registers an OpenSSH alias via `crate::ssh::write_alias`, and calls `podman create` followed by `podman start` on the target connection. The base image tag passed to `podman create` is `sandbox-mio/base:mise`, assembled by `base_image()` in the same file from `BASE_IMAGE` and `BASE_IMAGE_VERSION` constants. The `deploy` command in `src-tauri/src/lib.rs` calls `deploy` on the backend, then calls `reproduce_remote` on the runtime manager (in `src-tauri/src/runtime/mod.rs`) to seed `mise.toml`/`mise.lock` into the remote container and run `mise install`.

`ensure_base_image` in `src-tauri/src/backends/mod.rs` guards the local create path: it checks `podman image exists` on the local connection and builds from the bundled Containerfile if absent. Before the fix described in this document, no equivalent guard existed for the remote connection: `deploy` issued `podman create` on the target connection unconditionally, and Podman's `--pull=missing` default caused it to attempt a registry pull of `docker.io/sandbox-mio/base:mise` when the image was absent, which fails with "requested access to the resource is denied" because the image is private and not on any public registry.

## The Base Image Problem

The mise base image is a ~324 MB debian-slim layer with mise and zellij baked in, built from `src-tauri/resources/Containerfile`. It is built on demand on the local connection by `ensure_base_image` (which `create` calls before `podman create`) and pre-staged onto remote connections by the E2E harness (`ensureMiseBaseImage` in `e2e/lib/podman-machine.ts`). On a real remote host encountered for the first time — or after the E2E harness removes the image to exercise the bootstrap path — no staging has occurred and the image is absent.

The fix introduces `ensure_image_on(connection: &str)` as a static async method on `PodmanCliBackend` in `src-tauri/src/backends/mod.rs`. It checks `podman image exists` on the given connection and, when absent, writes the bundled Containerfile to a temp directory and calls `podman build --tag <image> <context>` on that connection. `ensure_base_image` (the local path, called under the `build_lock` mutex) is refactored to delegate to `ensure_image_on(&self.connection)`. `deploy` calls `ensure_image_on(target_connection)` before `podman create` on the remote.

## Software Reproduction Without a Committed Image

Software installed in a sandbox is recorded in `mise.toml` and locked in `mise.lock` in the workspace directory. `deploy` does not commit or transfer a container image for software — the commit-and-push path was removed when the runtime moved from an opaque `podman commit`/`podman save` model to the mise manifest model. `reproduce_remote` in `src-tauri/src/runtime/mod.rs` runs after `deploy` returns: it copies `mise.toml` and `mise.lock` into the remote container with `podman cp`, then runs `mise install` inside the container to reproduce the locked tool closures from their upstream release hosts. The absence of a `sandbox-mio/<name>:deployed` image on the remote after deploy is the explicit invariant the E2E regression test checks.

## Remote Workspace

A sandbox's workspace on the remote is a directory on the remote host's filesystem, not the laptop. `resolve_workspace` in `src-tauri/src/backends/mod.rs` computes the remote bind-mount source as `<home>/work/<name>` on the remote host (where `home` is `/root` for the root user). `remote_mkdir` creates the directory over the connection's SSH identity. Mutagen syncs the laptop workspace directory to the remote workspace directory bidirectionally; see `docs/features/remote-sync/01_ideas.md` for the sync design.

## Technology Choices With Tradeoffs

### Base image delivery to remote: build from bundled Containerfile

The base image is built on the remote from the Containerfile embedded in the binary at compile time (`include_str!("../../resources/Containerfile")` in `src-tauri/src/backends/mod.rs`). The alternatives evaluated:

- **`podman image scp` (direct machine-to-machine copy)** — rejected. `podman image scp` fails between machine connections on Windows (documented in `e2e/lib/podman-machine.ts` in `ensureMiseBaseImage`), which is the primary development platform. It is also specific to Podman machines and does not generalise to a real remote host.
- **`podman save | podman load` piped over SSH (the E2E harness path)** — rejected for the application deploy path because it requires a working SSH pipe from the laptop to the remote and streams ~324 MB synchronously before the container can be created. The build path streams the Containerfile context (negligibly small, since the recipe COPYs nothing) and pulls the base layers from the upstream Debian registry on the remote, which is the same network the remote host uses to fetch mise tools and is already expected to be reachable.
- **Publishing the image to a public registry (e.g. Docker Hub)** — rejected because the image is project-specific, may contain tooling not appropriate for a public image, and the registry-pull path is exactly the failure mode this fix addresses.
- **Embedding the full image tarball in the binary** — rejected because the ~324 MB image would roughly double the binary size on every release, and the binary is already distributed as a desktop installer.

### Software reproduction: mise lockfile, not committed image

Software is reproduced on the remote from `mise.toml`/`mise.lock` via `mise install` in `reproduce_remote` (`src-tauri/src/runtime/mod.rs`). The alternative — `podman commit` + `podman save`/`podman load` to transfer the full container filesystem including installed tools — was the original mechanism and was removed because: the committed image is opaque (no human-readable record of what is installed), each commit accumulates layer bloat, and streaming the image to the remote transfers all installed tool binaries even when only one tool changed. The mise lockfile transfers kilobytes of text and lets the remote resolve identical pinned binaries from upstream release hosts, which the remote already reaches for its own tool installs.

## Open Questions

- **SSH host-key verification on first connect.** `remote_mkdir` passes `StrictHostKeyChecking=accept-new` in `src-tauri/src/backends/mod.rs`, which auto-accepts the remote host key on first connection. Whether this is acceptable for production use or whether the user should be prompted to verify the host fingerprint is undecided.
- **Base image build progress reporting.** Building the base image on the remote can take several minutes on a cold machine. Whether `deploy` streams build progress to the frontend (via the existing `Channel<serde_json::Value>` progress pattern used by `provision_host`) or runs silently is undecided.
- **Base image version drift between local and remote.** If the local connection builds `sandbox-mio/base:mise` from a newer binary (bumped `BASE_IMAGE_VERSION`) while the remote retains an older build under the same tag, `ensure_image_on` sees the tag as present and skips the rebuild. Whether tag-bumping is sufficient to force a remote rebuild, or whether a content digest check is needed, is undecided.
- **Undeploy and remote container lifecycle.** `remove_remote` in `src-tauri/src/backends/mod.rs` removes the remote container but does not remove the remote workspace directory. Whether undeploy should also tear down the Mutagen sync session and leave the remote workspace intact for a future redeploy, or remove it, is undecided.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `podman create` on the remote fails with "requested access to the resource is denied" | The mise base image is absent on the remote, so Podman falls back to a registry pull of `docker.io/sandbox-mio/base:mise`, which is not on any public registry | `ensure_image_on(target_connection)` in `src-tauri/src/backends/mod.rs` builds the image from the bundled Containerfile before `podman create` is called |
| Deploy appears to succeed but the remote container exits immediately | The base image was built or transferred in a partially written state (e.g. a prior interrupted build left the tag present but the image broken) | Remove the broken image manually on the remote (`podman rmi --force sandbox-mio/base:mise`) to force a clean rebuild on next deploy |
| `podman save \| podman load` works in the E2E harness but not in the app | The harness pre-stages the image with a shell pipe; the app uses `--connection`-routed `podman` which cannot pipe between two connections in a single command | Use `ensure_image_on` (build on the target from the embedded Containerfile) rather than save/load for the app-side path |
| Base image on the remote is stale after a Containerfile recipe change | `ensure_image_on` short-circuits on `podman image exists`; if the tag is present but the recipe has changed the old image is used | Bump `BASE_IMAGE_VERSION` in `src-tauri/src/backends/mod.rs` when the Containerfile recipe changes; the new tag forces `ensure_image_on` to rebuild |
| Software installed locally is absent on the remote after deploy | `deploy` does not commit or transfer the container image; software lives in the mise manifest | `reproduce_remote` in `src-tauri/src/runtime/mod.rs` seeds `mise.toml`/`mise.lock` and runs `mise install` on the remote; ensure it is called after `deploy` returns in `src-tauri/src/lib.rs` |
