//! The production [`ToolchainSecrets`]: the GitHub token from the credential store, exported to
//! in-sandbox `mise` calls (`docs/features/techdebt-sweep/02_dev.md`, Resolved Decision 3). Values
//! never leave the returned list; callers log variable names only.

use tauri::AppHandle;

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
    fn env(&self) -> Vec<(String, String)> {
        crate::credentials::github_secret_env(&self.app)
    }
}
