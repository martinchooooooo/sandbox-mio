# mise Runtime-Manager Spike Artifacts

Reference artifacts for the 2026-06-27 spike that replaced flox with [mise](https://mise.jdx.dev) to shrink the sandbox base image. The findings are written up in `docs/features/sandbox-runtime-management/02_dev.md` (Spike Validation and Resolved Decisions). The flox spike artifacts in the parent directory are kept for history; this directory is the mise replacement.

## Files

- `Containerfile.glibc` — the recommended Phase A base image: `debian:12-slim` + a pinned `mise` binary + a pinned `zellij` symlinked onto the global PATH. Built and validated on 2026-06-27; the resulting image measures **324 MB**.
- `Containerfile.musl` — the rejected Alpine variant, kept as the size data point. Built at **172 MB**, but glibc-only language runtimes (e.g. `node@22`) fail on musl, which is why the glibc base was chosen.

## Headline findings (2026-06-27)

- The pinned `mise v2026.6.14` static binary runs on both `alpine:3.24` (musl build) and `debian:12-slim` (glibc build). The mise binary itself is ~97 MB and is the floor of the base-image size.
- `mise install zellij@0.44.3` resolves to the `aqua:zellij-org/zellij` backend, downloads the musl asset, and provides the `zellij web` subcommand the in-app terminal requires (zellij ≥ 0.43.0). Symlinked to `/usr/local/bin/zellij`, it runs standalone off any per-sandbox env.
- `mise.toml` records intent (`jq = "latest"`); `mise.lock` records the resolved version with per-platform SHA256 checksums, URLs, and `github-attestations` provenance. A package installed on one container reproduces byte-identically on a fresh container from a **3 KB** `mise.toml` + `mise.lock` transfer.
- `mise use node@22` fails on Alpine (exit 127: nodejs.org ships glibc-only binaries) and succeeds on debian-slim (`node v22.23.1`). This is why the base is glibc, not musl.
- mise's documented musl-node workarounds were tested on Alpine and rejected, even though the prebuilt musl binaries exist (`unofficial-builds.nodejs.org/download/release/v22.9.0/node-v22.9.0-linux-x64-musl.tar.gz` is present). The blocker is mise `v2026.6.14`'s `core:node` backend: both `node.flavor=musl` and `libc=musl` are read by `mise settings` but ignored by the resolver, which compiles from source on musl (verbose log: `we will fetch the source and compile`) — needing a baked C++ toolchain and a slow per-install build. The glibc base needs none of these settings.
- Unauthenticated installs hit GitHub's 60-req/h version-listing rate limit (HTTP 403). Pin exact versions and pass `GITHUB_TOKEN`/`MISE_GITHUB_TOKEN` to avoid it.

## Image-size comparison

| Image | Size |
|---|---|
| `base:zellij` (shipped flox + Nix + zellij) | 1.84 GB |
| `base:flox` (flox without zellij) | 1.27 GB |
| `base:mise-glibc` (`Containerfile.glibc`) | 324 MB |
| `base:mise-musl` (`Containerfile.musl`) | 172 MB |

## Re-running

Build either Containerfile against a local connection, for example:

```
podman --connection podman-machine-default build -f Containerfile.glibc -t sandbox-mio/base:mise-glibc .
```

To re-test the manifest/reproduce flow: in a running container set `MISE_DATA_DIR`, `MISE_LOCKFILE=1`, run `mise use <tool>` in `/root/work`, `touch mise.lock` and `mise install` to populate the lockfile, then copy `mise.toml` + `mise.lock` to a fresh container and run `mise install` to confirm identical versions.
