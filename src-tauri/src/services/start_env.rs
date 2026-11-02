//! The production [`StartEnvSource`]: per-sandbox environment for start-capable `msb` calls,
//! resolved from the applied policy snapshot, the secret-value store and the proxy state
//! (`docs/features/microsandbox-network-policy/02_dev.md`, Phase C). Values never leave the
//! returned list; callers log variable names only.

use tauri::AppHandle;

use crate::provider::msb_policy::SOCKS_PASSWORD_ENV;
use crate::provider::types::{ProviderError, StartEnvSource};

pub struct AppStartEnv {
    app: AppHandle,
}

impl AppStartEnv {
    pub fn new(app: AppHandle) -> Self {
        AppStartEnv { app }
    }
}

impl StartEnvSource for AppStartEnv {
    fn env_for(&self, instance: &str) -> Result<Vec<(String, String)>, ProviderError> {
        let applied = crate::net_policy::read_applied(&self.app, instance)
            .map_err(|e| ProviderError::CommandFailed(e.to_string()))?;
        let Some(applied) = applied else {
            return Ok(Vec::new());
        };
        let mut env = Vec::new();
        if !applied.policy.secrets.is_empty() {
            let values = crate::proxy::load_secrets(&self.app)
                .map_err(|e| ProviderError::CommandFailed(e.to_string()))?;
            for binding in &applied.policy.secrets {
                // Refuse rather than omit (Resolved Decision 17).
                let value = values.get(&binding.secret_id).ok_or_else(|| {
                    ProviderError::CommandFailed(format!(
                        "secret {:?} has no stored value",
                        binding.secret_id
                    ))
                })?;
                env.push((binding.env_var.clone(), value.clone()));
            }
        }
        if applied.chained_proxy {
            let password = crate::proxy::read_socks_password(&self.app, instance).ok_or_else(|| {
                ProviderError::CommandFailed(format!(
                    "the traffic viewer for {instance:?} has no recorded proxy password"
                ))
            })?;
            env.push((SOCKS_PASSWORD_ENV.to_string(), password));
        }
        Ok(env)
    }
}
