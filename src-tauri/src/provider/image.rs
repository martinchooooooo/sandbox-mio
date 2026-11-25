//! The base image tag the provider builds and every sandbox is created from.

// `e2e/lib/podman-machine.ts` regex-parses the two declarations below, so their literal form
// (name, type, `const`) is load-bearing beyond Rust.
const BASE_IMAGE: &str = "sandbox-mio/base";
/// Bump whenever the Containerfile changes: `ensure_base_image` short-circuits on an existing
/// tag, so an unchanged version silently keeps stale images.
pub(crate) const BASE_IMAGE_VERSION: u32 = 10;

pub(crate) fn base_image() -> String {
    format!("{BASE_IMAGE}:{BASE_IMAGE_VERSION}")
}
