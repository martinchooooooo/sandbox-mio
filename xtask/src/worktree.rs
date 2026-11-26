//! `dev --setup-worktree`: stages the gitignored assets of a fresh worktree, hard-linking them from
//! a sibling worktree when the pinned versions agree.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::binaries::{
    fetch_llama, fetch_mutagen, fetch_podman, record_staged_version_in, staged_version,
    LLAMA_VERSION, MUTAGEN_VERSION, PODMAN_VERSION,
};
use crate::exec::{run, try_run};
use crate::paths::root_dir;

/// Sibling git worktrees of `target_root` (`git worktree list --porcelain`), `target_root` itself
/// excluded. Empty on any git failure rather than an error — a missing sibling just means
/// `setup_worktree()` falls back to `fetch_*`.
fn sibling_worktrees(target_root: &Path) -> Vec<PathBuf> {
    let Ok(output) = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(target_root)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let target_canonical = fs::canonicalize(target_root).ok();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .filter(|p| fs::canonicalize(p).ok() != target_canonical)
        .collect()
}

/// Hard-links `dst` from `src` (falling back to a byte copy across volumes), removing an existing
/// `dst` first. Hard links make staged assets shared mutable state between worktrees; without the
/// removal, `fs::copy` on Windows would write into an existing destination in place rather than
/// replacing it, silently rewriting through the link (Resolved Decision 11).
fn link_or_copy(src: &Path, dst: &Path) {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let _ = fs::remove_file(dst);
    if fs::hard_link(src, dst).is_err() {
        fs::copy(src, dst)
            .unwrap_or_else(|e| panic!("failed to stage {} -> {}: {}", src.display(), dst.display(), e));
    }
}

/// Recursively `link_or_copy`s every entry of `src` into `dst`.
///
/// Adds and overwrites only — a file present in `dst` but absent from `src` is left in place, so a
/// renamed or dropped asset lingers in a target worktree that was staged before the rename. Harmless
/// today (every consumer resolves assets by name), but a full mirror would have to prune.
fn link_or_copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            link_or_copy_tree(&from, &to);
        } else {
            link_or_copy(&from, &to);
        }
    }
}

/// One independently stageable set of files under `src-tauri/binaries`. Probed and linked per group
/// rather than as one tree: a sibling commonly has Mutagen staged but not llama (`fetch_llama` skips
/// when already present, and the heavy tier treats llama as optional), and linking the whole tree off
/// a single sentinel silently produced a partially staged target whose `fetch_*` fallback never ran.
struct AssetGroup {
    key: &'static str,
    version: &'static str,
    /// Existence check, relative to `src-tauri/binaries`.
    sentinel: &'static str,
    /// Files and directories to link, relative to `src-tauri/binaries`.
    paths: Vec<String>,
    fetch: fn(),
}

fn asset_groups() -> Vec<AssetGroup> {
    let mutagen_exe = format!("mutagen-{}-pc-windows-msvc.exe", env::consts::ARCH);
    vec![
        AssetGroup {
            key: "mutagen",
            version: MUTAGEN_VERSION,
            sentinel: "mutagen-agents.tar.gz",
            paths: vec!["mutagen-agents.tar.gz".into(), mutagen_exe],
            fetch: fetch_mutagen,
        },
        AssetGroup {
            key: "llama",
            version: LLAMA_VERSION,
            sentinel: "llama/llama-server.exe",
            paths: vec!["llama".into()],
            fetch: fetch_llama,
        },
        AssetGroup {
            key: "podman",
            version: PODMAN_VERSION,
            sentinel: "podman/podman.exe",
            paths: vec!["podman".into()],
            fetch: fetch_podman,
        },
    ]
}

/// Stages `tools/msedgedriver.exe` and all of `src-tauri/binaries/` into `target` (defaulting to
/// the invoking worktree), plus `pnpm install` (one install covers `e2e/` too, via
/// `pnpm-workspace.yaml`). A fresh worktree has neither: both are `.gitignore`d, so the e2e harness
/// has no `msedgedriver` to spawn and the Mutagen agent-bundle copy in `e2e/wdio.conf.ts`'s
/// `onPrepare` has no source file (docs/features/parallel-worktree-e2e/02_dev.md, Phase F).
///
/// Per asset group, prefers a hard link from the first sibling worktree whose `STAGED_VERSIONS`
/// manifest agrees with this checkout's pinned version — costs no disk for the large
/// llama.cpp/Podman/agent-bundle payloads. The manifest is what makes the link safe: parallel
/// feature branches are exactly the case where two worktrees sit on different commits, and linking
/// on mere existence would hand this checkout a binary its own `MUTAGEN_VERSION`/`LLAMA_VERSION`/
/// `PODMAN_VERSION` does not pin, which no later `fetch_*` would correct.
///
/// Falls back to `fetch_mutagen`/`fetch_llama`/`fetch_podman` per group when no sibling qualifies —
/// those are not safe to run concurrently with another worktree's fetch, since each extracts through
/// a fixed, shared `env::temp_dir()` path it removes first.
pub(crate) fn setup_worktree(target: Option<String>) {
    let target_root = target.map(PathBuf::from).unwrap_or_else(root_dir);
    let target_root = fs::canonicalize(&target_root)
        .unwrap_or_else(|e| panic!("cannot resolve target worktree {}: {}", target_root.display(), e));

    run(target_root.clone(), "pnpm", &["install"]);

    let siblings = sibling_worktrees(&target_root);

    let target_tools = target_root.join("tools");
    match siblings
        .iter()
        .map(|s| s.join("tools").join("msedgedriver.exe"))
        .find(|p| p.exists())
    {
        Some(src) => link_or_copy(&src, &target_tools.join("msedgedriver.exe")),
        None => {
            // `setup_e2e()` installs msedgedriver-tool into ~/.cargo/bin globally, so a missing
            // sibling is a fetch away rather than a dead end — mirroring how the binaries groups
            // below fall back to their own `fetch_*`.
            fs::create_dir_all(&target_tools).unwrap();
            println!("no sibling worktree has tools/msedgedriver.exe staged; fetching it directly");
            if !try_run(target_tools, "msedgedriver-tool.exe", &[]) {
                println!(
                    "could not run msedgedriver-tool.exe; run `cargo xtask e2e --setup` \
                     in {} to install it and fetch the driver",
                    target_root.display()
                );
            }
        }
    }

    let target_binaries = target_root.join("src-tauri").join("binaries");
    for group in asset_groups() {
        let source = siblings
            .iter()
            .map(|s| s.join("src-tauri").join("binaries"))
            .find(|b| {
                b.join(group.sentinel).exists()
                    && staged_version(b, group.key).as_deref() == Some(group.version)
            });
        match source {
            Some(src) => {
                for rel in &group.paths {
                    let from = src.join(rel);
                    let to = target_binaries.join(rel);
                    if from.is_dir() {
                        link_or_copy_tree(&from, &to);
                    } else if from.exists() {
                        link_or_copy(&from, &to);
                    }
                }
                record_staged_version_in(&target_binaries, group.key, group.version);
                println!("Linked {} {} from {}", group.key, group.version, src.display());
            }
            None => {
                println!(
                    "no sibling worktree has {} {} staged; fetching directly into {} — do not run \
                     this concurrently with another worktree's fetch (02_dev.md Phase F)",
                    group.key,
                    group.version,
                    target_root.display()
                );
                let cwd = env::current_dir().unwrap();
                env::set_current_dir(&target_root).unwrap();
                (group.fetch)();
                env::set_current_dir(cwd).unwrap();
            }
        }
    }

    println!("Worktree staged: {}", target_root.display());
}
