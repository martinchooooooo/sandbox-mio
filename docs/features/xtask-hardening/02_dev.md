---
type: dev-plan
title: "xtask Hardening — Build Plan"
feature: xtask-hardening
---
# xtask Hardening — Build Plan

Follow-up to Phase 5 of [codebase-restructure](../codebase-restructure/02_dev.md), which moved the build tool into the `xtask` Cargo package verbatim. An audit of the moved code found sloppy argument handling, a manifest bug that stages stale binaries, unverified downloads that feed the signed installer, and two hand-written parsers (YAML frontmatter, Markdown links) that misread valid input. Status: all six phases are implemented and reviewed.

## Resolved Decisions

1. **Dependencies replace parsers and hashing only.** `yaml-rust2`, `pulldown-cmark` and `sha2`, each with `default-features = false`. No CLI framework, process, filesystem-walking, HTTP, zip, JSON or date crate: std covers those at the tool's size. The Windows job-object code in `xtask/src/win_job.rs` stays hand-written.
2. **Frontmatter scalars keep their source text.** `xtask/src/docs_check.rs` builds its `Value` tree from `yaml-rust2`'s parser events rather than its typed `Yaml` enum, so `007`, `yes`, `1.0` and `true` stay strings exactly as the old parser returned them. The same event pass rejects duplicate keys, aliases, non-scalar keys and a second document. A frontmatter whose top level is not a mapping is an error.
3. **Words outside code blocks are counted over source text.** `markdown_facts()` in `xtask/src/docs_check.rs` counts whitespace-separated tokens of the body minus code-block ranges, not parsed text events, so the architecture word budget keeps its calibration.
4. **`e2e <tier>` accepts any tier name that does not start with `-`.** Names such as `llm` and `activity` map to `test:*` scripts in `e2e/package.json`, so the tier list is not closed. A leading `-` is rejected because the argument is forwarded into `pnpm run test:<tier>`.
5. **Checksum sources.** Mutagen: the release's published `SHA256SUMS`. Podman: the release's published `shasums` (equal to the GitHub API `digest`). llama.cpp publishes no checksum file; the value is the release asset's `digest` from the GitHub releases API. `fetch_llama()` supports x64 only, because the pinned release has no arm64 Vulkan build and the digest covers the x64 zip.
6. **The package carries no unit tests.** The user asked for every test in `xtask/` to be removed after the work was verified with them; behaviour is checked by running the tool (CLI exit codes, a forced re-fetch of each binary, a deliberately wrong digest, and `cargo xtask docs` output compared before and after). The step that would have added `cargo test` for `xtask/` to `ci_checks()` in `xtask/src/ci.rs` is therefore dropped.

## Implemented: Phase 1 — `exec.rs` Deduplication and Strict CLI

- `xtask/src/exec.rs`: one private `base_command()` builds the `cmd /C` command for `try_run`, `run_env` and `spawn_background`; no behaviour change.
- `xtask/src/main.rs`: `parse()` matches argument slice patterns into a `Cli` enum, so arity is strict. Unknown or extra arguments print the usage text to stderr and exit 1; `--help`, `-h` and `help` print it to stdout and exit 0. Every previously valid invocation is unchanged. `e2e --spec x` no longer runs `pnpm run test:--spec`.

## Implemented: Phase 2 — Staged-Version Manifest

- `xtask/src/binaries.rs`: `staged_action()` decides from the files present, the recorded version and the pin: fetch when files are missing or the recorded version differs, skip when it matches, adopt (record the pin once) when files exist with no record. `already_staged()` applies it for all three `fetch_*` functions. Bumping `LLAMA_VERSION` and rerunning `dev --setup` now re-fetches instead of recording the new version over old files, so `setup_worktree()` in `xtask/src/worktree.rs` no longer hard-links stale binaries.
- `fetch_llama()` extracts into `src-tauri/binaries/llama.partial`, checks `llama-server.exe`, then replaces `llama` with one rename, so an interrupted extraction never leaves a directory that looks staged.
- `install_file()` stages each Mutagen and Podman file the same way: copy to `<name>.partial`, then rename into place. `fetch_podman()` counts Podman as present only when all three of its files exist. Together these keep an interrupted first fetch, which has no manifest entry yet, from being adopted as a complete stage.

## Implemented: Phase 3 — Checksum Pins

- `xtask/src/binaries.rs`: `MUTAGEN_SHA256`, `LLAMA_SHA256` and `PODMAN_SHA256` sit beside their version constants. `download_verified()` downloads with curl, hashes the file with `sha2`, and on a mismatch deletes the download and panics with the expected and actual digests, before bsdtar extracts anything.
- Bumping a version constant means bumping the digest beside it; the root `AGENTS.md` Common Gotchas table records the rule. `e2e/lib/mutagen.ts` pins its own Mutagen version and downloads it without a digest check; it is outside this feature.

## Implemented: Phase 4 — `yaml-rust2` for Frontmatter

- `xtask/src/docs_check.rs`: `parse_frontmatter()` replaces the hand-written subset parser (Resolved Decision 2). `split_frontmatter()` is rewritten line by line and now tolerates a leading UTF-8 BOM and trailing whitespace after a `---` fence. `is_date()` rejects impossible days such as `2026-02-31` and handles leap years.
- Newly valid input: an indented top-level mapping, block scalars, a trailing `# comment` after a value, quoted values followed by a comment, quoted flow-list items containing commas, double-quoted values wrapped over two lines. Newly rejected: a plain scalar containing `: `.

## Implemented: Phase 5 — `pulldown-cmark` for Markdown

- `xtask/src/docs_check.rs`: `markdown_facts()` makes one CommonMark pass (tables enabled) and returns link targets, inline code spans and the prose word count; each `Doc` stores the result so a document is parsed once. `strip_code()`, `relative_links()`, `cited_doc_paths()` and `prose_words()` are gone.
- Fixed misreads: links with titles, reference-style links (also for index reachability), parenthesised and `<angle>` destinations, `~~~` and longer-than-three-backtick fences, indented code blocks, double-backtick spans, `f(x](y)` prose, and backtick parity across fences when collecting cited `docs/...` paths. Email autolinks are skipped.
- Over the docs corpus at the time of the change, the frontmatter trees, link sets, cited paths and word counts were identical before and after, and `cargo xtask docs` printed the same output.

## Implemented: Phase 6 — CI Cache

- `.github/workflows/ci.yml` (both jobs) and `.github/workflows/release.yml`: `swatinem/rust-cache` caches `xtask` beside `src-tauri`, since the tool now has dependencies to compile. Triggers and every other step are unchanged. No `cargo test` step is added (Resolved Decision 6).

## Known Leftovers

- `e2e/lib/mutagen.ts` downloads Mutagen without a digest check and pins its version separately from `MUTAGEN_VERSION` in `xtask/src/binaries.rs`.
- `cargo clippy --all-targets` in `xtask/` still reports the ten `map_or` simplifications and one pointer cast carried over from Phase 5.
- `setup_worktree()` links `llama` with adds and overwrites only, so a file dropped between llama.cpp releases lingers in a target worktree until its `src-tauri/binaries/llama` is deleted.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `cargo xtask dev --setup` panics with `SHA-256 mismatch for ...` | A `*_VERSION` constant in `xtask/src/binaries.rs` was bumped without its `*_SHA256`, or the download was corrupted or tampered with | Take the new digest from the release (`SHA256SUMS` for Mutagen, `shasums` for Podman, the asset `digest` in the GitHub releases API for llama.cpp) and set the constant; the failed download is already deleted, so rerun |
| `cargo xtask dev --setup` prints `staged at <old> but <new> is pinned; re-fetching` | The manifest `src-tauri/binaries/.staged-versions.json` records a version other than the pin | Expected after a version bump; let the fetch finish rather than editing the manifest |
| `cargo xtask e2e --spec <file>` prints the usage text and exits 1 | `parse()` in `xtask/src/main.rs` accepts only a tier name after `e2e`, never a flag | Run a single spec from `e2e/` with `pnpm exec wdio run wdio.conf.ts --spec ./test/specs/light/<name>.spec.ts` |
| A docs frontmatter the old parser accepted now reports `frontmatter does not parse` | The old subset parser took any text after `key:`; `yaml-rust2` follows YAML, where a plain scalar cannot contain `: ` | Quote the value |
| `cargo xtask docs` reports a `broken link` for a link that was previously skipped | Links with titles, reference-style links and `<angle>` destinations are now read | Fix the target, or put the example in a code block |
| The first `cargo xtask` run in a fresh checkout takes several seconds | `xtask/target/` compiles three dependencies | Expected once per checkout; CI caches `xtask/target` through `swatinem/rust-cache` |
