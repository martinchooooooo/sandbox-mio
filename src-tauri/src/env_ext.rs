//! Directory overrides read from the environment. The e2e suite redirects app state away from the
//! developer's real per-user directories with these, since debug and release builds share a bundle
//! identifier (`e2e/wdio.conf.ts` `onPrepare`).

use std::path::PathBuf;

/// The directory `var` names, or `None` when it is unset. Read at call time and never cached: the
/// e2e suite sets these per process and depends on the late read. An empty value counts as set.
pub(crate) fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var(var).ok().map(PathBuf::from)
}
