//! Repository paths. Every path derives from the current directory, which `cargo run` leaves at
//! the invocation directory, so the tool must be run from the repository root. Not derived from
//! `CARGO_MANIFEST_DIR`: that would pin the checkout the tool was compiled in, not the worktree
//! it is run for.

use std::{env, path::PathBuf};

pub(crate) fn root_dir() -> PathBuf { env::current_dir().unwrap() }
pub(crate) fn e2e_dir() -> PathBuf { root_dir().join("e2e") }
pub(crate) fn tools_dir() -> PathBuf { root_dir().join("tools") }
pub(crate) fn binaries_dir() -> PathBuf { root_dir().join("src-tauri").join("binaries") }
pub(crate) fn debug_dir() -> PathBuf { root_dir().join("src-tauri").join("target").join("debug") }
