//! `package`: stages the bundled binaries and builds the release NSIS installer.

use crate::binaries::fetch_bundled_binaries;
use crate::exec::run;
use crate::paths::root_dir;

pub(crate) fn package() {
    // Bundling copies from src-tauri/binaries at build time (unlike `dev`, which needs
    // `stage_*` to mirror them into target/debug); make sure they're present first.
    fetch_bundled_binaries();
    run(root_dir(), "pnpm", &["tauri", "build"]);
}
