//! The production [`ToolchainSecrets`]: the GitHub token from the credential store, exported to
//! in-sandbox `mise` calls (`docs/features/techdebt-sweep/02_dev.md`, Resolved Decision 3). Values
//! never leave the returned list; callers log variable names only.
//!
//! A sandbox behind an iron-proxy sidecar gets the placeholder while its GitHub binding is rendered,
//! and nothing when opted out; the binding's hosts include `raw.githubusercontent.com`, where mise
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
            // Same opt-out logic as the binding, so the placeholder is exported only when swapped.
            return match crate::proxy::iron::github_binding(&self.app, sandbox) {
                Ok(Some(_)) => crate::credentials::github_env(GITHUB_PLACEHOLDER),
                Ok(None) | Err(_) => Vec::new(),
            };
        }
        crate::credentials::github_secret_env(&self.app)
    }
}
