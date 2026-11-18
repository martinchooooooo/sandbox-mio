//! The handoff a host SSH client needs to attach to a sandbox through its managed `~/.ssh` alias.

use crate::error::AppError;
use crate::services::routing::SandboxRouter;
use crate::ssh::types::SshAccess;

/// Rewrites `name`'s alias block, since its `ProxyCommand` follows deploys, and reports its values.
/// Never starts a stopped sandbox (`docs/features/host-tool-ssh-access/02_dev.md`, Resolved Decisions).
pub async fn access(
    name: &str,
    router: &SandboxRouter,
    app: &tauri::AppHandle,
) -> Result<SshAccess, AppError> {
    // Before any key is created: the name names files on the host and a block in `~/.ssh`.
    crate::backends::validate_name(name)?;
    let route = router.resolve(name, app).await?;
    let identity_path = crate::backends::ensure_sandbox_identity(app, name)?;
    if route.running {
        // Also heals the empty pinned file of a sandbox created before host keys were seeded.
        let provider = router.provider_for(&route.machine);
        crate::backends::seed_sandbox_host_keys(&provider, name, &identity_path).await;
    }
    Ok(SshAccess {
        alias: route.forward_alias,
        known_hosts_path: crate::backends::sandbox_known_hosts_path(&identity_path),
        proxy_command: crate::backends::sandbox_proxy_command(name, &route.machine),
        identity_path,
        running: route.running,
    })
}

/// Pins `name`'s host keys after a start, in the background. A Podman sandbox is stopped at create,
/// so this is its first chance; without it, a strict-checking client (Claude Desktop) fails its
/// first connect unless the panel happened to load while the sandbox ran.
pub fn spawn_pin_host_keys(app: &tauri::AppHandle, name: &str) {
    let app = app.clone();
    let name = name.to_string();
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        if let Err(e) = access(&name, &app.state::<SandboxRouter>(), &app).await {
            tracing::warn!(target: "ssh", sandbox = %name, error = %e, "host key pinning after start failed");
        }
    });
}
