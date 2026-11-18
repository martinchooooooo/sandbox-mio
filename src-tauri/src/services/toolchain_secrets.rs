//! The production [`ToolchainSecrets`]: the GitHub token from the credential store, exported to
//! in-sandbox `mise` calls (`docs/features/techdebt-sweep/02_dev.md`, Resolved Decision 3). Values
//! never leave the returned list; callers log variable names only.
//!
//! A sandbox behind an iron-proxy sidecar gets the placeholder only while its GitHub binding is
//! staged in the sidecar, and nothing when opted out or the binding is missing; the binding's hosts include `raw.githubusercontent.com`, where mise
//! sends the token (`docs/features/proxy-credential-secrets/02_dev.md`, Resolved Decision 15).

use tauri::AppHandle;

use crate::credentials::github::GITHUB_PLACEHOLDER;
use crate::runtime::types::ToolchainSecrets;

pub struct AppToolchainSecrets {
    app: AppHandle,
}

impl AppToolchainSecrets {
    pub fn new(app: AppHandle) -> Self {
        AppToolchainSecrets { app }
    }
}

impl ToolchainSecrets for AppToolchainSecrets {
    fn env(&self, sandbox: &str) -> Vec<(String, String)> {
        if super::github_access::github_sidecar_mode(&self.app, sandbox) {
            let live = crate::proxy::iron::github_binding_live(&self.app, sandbox);
            return sidecar_env(sandbox, live);
        }
        crate::credentials::github_secret_env(&self.app)
    }
}

/// The placeholder only when the sidecar has the binding staged. Otherwise nothing, so mise goes
/// anonymously instead of sending a placeholder GitHub rejects with 401.
fn sidecar_env(
    sandbox: &str,
    live: Result<bool, crate::proxy::types::ProxyError>,
) -> Vec<(String, String)> {
    match live {
        Ok(true) => crate::credentials::github_env(GITHUB_PLACEHOLDER),
        Ok(false) => Vec::new(),
        Err(e) => {
            tracing::warn!(target: "proxy", %sandbox, error = %e, "GitHub binding state unreadable; mise runs without a token");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::types::ProxyError;

    #[test]
    fn the_placeholder_is_exported_only_for_a_live_binding() {
        assert!(!sidecar_env("s", Ok(true)).is_empty());
        assert!(sidecar_env("s", Ok(false)).is_empty());
        assert!(sidecar_env("s", Err(ProxyError::CommandFailed("x".into()))).is_empty());
    }
}
