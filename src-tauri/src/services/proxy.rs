//! Resolves where a sandbox's active proxy sidecar runs, so `proxy/` functions take a resolved
//! [`SidecarSite`] and never touch the sync engine (`docs/features/proxy-remote-deploy/02_dev.md`,
//! The Concrete Model).

use crate::error::AppError;
use crate::proxy::types::SidecarSite;
use crate::services::routing::SandboxRouter;

/// The site of `name`'s active sidecar: its deploy target while a Mutagen session exists, else the
/// local machine. An error is never read as "local": a deployed sandbox's local sidecar is stopped
/// by design and must not be started or rewritten on a Mutagen failure.
pub async fn locate_sidecar(router: &SandboxRouter, name: &str) -> Result<SidecarSite, AppError> {
    let route = router.locate(name).await?;
    Ok(SidecarSite {
        provider: router.provider_for(&route.machine),
        local: router.is_local(&route.machine),
        machine: route.machine,
    })
}
