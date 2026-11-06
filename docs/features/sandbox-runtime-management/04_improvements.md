---
type: improvements
title: "Lessons Learned: flox → mise"
feature: sandbox-runtime-management
---
# Lessons Learned: flox → mise

A short retrospective on the runtime-management engine. The build details are in `02_dev.md` and the test plan in `03_e2e.md`; this file records *why the engine changed* so a future agent does not relitigate it.

## What we shipped first: flox

The first engine was [flox](https://flox.dev) (built on Nix). It gave a manifest (`.flox/env/manifest.toml`) of human intent, a lockfile (`manifest.lock`) for reproducibility, and `flox install`/`activate` for the install/run surface. It worked: a package installed on the laptop reproduced byte-for-byte on a remote from the lockfile alone.

The problem was size. The Nix store made the sandbox base image **1.84 GB**. Every connection had to carry that image, and it dwarfed the actual user software inside.

## What we moved to: mise

A 2026-06-27 spike replaced flox with [mise](https://mise.jdx.dev): a single static binary that downloads prebuilt upstream release binaries (GitHub releases via aqua/ubi, plus language-runtime backends) instead of building a Nix closure. It keeps the same manifest-driven model — `mise.toml` (intent) + `mise.lock` (checksum-pinned closure) — so the `RuntimeManager` trait and the deploy flow survived almost unchanged.

The base image dropped to **324 MB** (5.7× smaller). The `RuntimeManager` surface was engine-agnostic enough that only `init` lost a parameter; the Tauri command wiring was otherwise untouched.

## Lessons

- **Keep the engine behind a trait.** Because software management lived behind `RuntimeManager` (not baked into the container backend), swapping flox for mise touched the engine module and the recipe, not the command layer. Design for the engine being replaceable.
- **The manifest+lockfile model was the durable part.** Both engines express "intent file + pinned lockfile, reproduce on the remote from the lockfile." That shape carried over verbatim; only the substrate underneath it changed. Deploy ships the ~3 KB manifest+lock, never a committed image — true for both engines.
- **Nix's reproducibility came at a fixed, large cost.** The 1.84 GB was the Nix store, not our software. If the win is "deterministic builds" but the price is a multi-GB base, weigh whether a checksum-pinned-binary tool (mise) buys most of the reproducibility for a fraction of the size.
- **glibc vs musl is a real fork in the road.** flox/Nix packages carried their own glibc, so the base could be tiny Alpine/musl. mise installs upstream prebuilts, and many language runtimes (e.g. Node from nodejs.org) ship glibc-only — they fail on musl with exit 127. Removing Nix removed the only reason to stay on musl, so the base became `debian:12-slim` (glibc). mise's documented musl-node workarounds (`node.flavor=musl`, `libc=musl`) did *not* rescue the musl base in `v2026.6.14`: the `core:node` backend compiles from source on musl regardless. Don't assume a tool-manager setting overrides its backend's platform logic — verify it.
- **Pin exact versions to dodge the GitHub rate limit.** Both engines resolved versions against the GitHub API (60 req/h unauthenticated → HTTP 403). Pin exact tool/release versions so the install addresses a known artifact with no version-listing call, and pass a `GITHUB_TOKEN` for cold installs.
- **Don't pipe an installer to a shell.** The reproducible (and policy-compliant) way to bake mise was downloading the pinned release tarball directly, not `curl https://mise.run | sh`. The direct download is also more deterministic.
