//! `.mio/net-policy.json`: the shared, provider-neutral outbound policy of one workspace, plus the
//! per-sandbox applied snapshot under the app data root. A plain file module like
//! [`crate::mio_files`], so `backends`, `proxy` and `services` may all use it. Schema:
//! `docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 12.
//!
//! `schema` holds the document types and file access, `validate` the provider-dialect checks,
//! `diff` the reviewable change list and `applied` the accepted-policy store.

pub mod applied;
pub mod diff;
pub mod schema;
pub mod validate;

use crate::domain::types::BackendError;
use crate::mio_files;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct NetPolicyError(pub String);

impl From<NetPolicyError> for BackendError {
    fn from(e: NetPolicyError) -> Self {
        BackendError::CommandFailed(e.0)
    }
}

impl From<mio_files::FileError> for NetPolicyError {
    fn from(e: mio_files::FileError) -> Self {
        NetPolicyError(e.to_string())
    }
}

/// Fixtures shared by the tests of `schema`, `validate` and `diff`.
#[cfg(test)]
mod test_support {
    use super::schema::{default_headers, NetRule, SecretBinding};
    use crate::provider::types::{ProviderCapabilities, RuleAction};

    /// A provider that enforces the policy with its own network stack; none ships today.
    pub const ENGINE: ProviderCapabilities = ProviderCapabilities {
        remote_targets: false,
        event_stream: false,
        host_bind_mount: false,
        egress_policy: true,
        sidecar_egress: false,
    };

    pub const PODMAN: ProviderCapabilities =
        ProviderCapabilities { egress_policy: false, sidecar_egress: true, ..ENGINE };

    pub fn binding(host: &str) -> SecretBinding {
        SecretBinding {
            secret_id: "gh".to_string(),
            env_var: "GH_TOKEN".to_string(),
            hosts: vec![host.to_string()],
            headers: default_headers(),
            query: false,
            body: false,
        }
    }

    pub fn rule(action: RuleAction) -> NetRule {
        NetRule { action, host: None, cidr: None, group: None, protocol: None, ports: None }
    }
}
