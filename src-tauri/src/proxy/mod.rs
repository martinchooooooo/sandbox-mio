//! Per-sandbox proxy sidecar: traffic inspection, egress policy and secret swap
//! (`docs/features/network-proxy/02_dev.md`). Free functions taking `&Arc<dyn MachineProvider>` and
//! a [`store::ProxyStore`] for the state directory, modelled on `crate::credentials` rather than the
//! trait+mock subsystem pattern — every external call already crosses the `ProcessRunner` seam
//! through a provider, so the e2e `light` tier is covered with no new trait. A function takes
//! `&AppHandle` only for what is not proxy state: events, the activity registry, the credential
//! store and the accepted policy.
//!
//! A Podman sidecar runs iron-proxy as the only exit of the sandbox's internal network
//! (`docs/features/podman-egress-boundary/02_dev.md`). Its `meta.json` records the engine and the
//! provider; a sidecar without an engine is treated as iron-proxy.
//!
//! The files: `store` (state-directory layout), `sidecar` (container lifecycle and status), `apply`
//! (the policy apply pipeline), `secret_files` (local secret-file staging), `remote_push` (the
//! remote sidecar's scripts and pushes), `github` (the GitHub token binding), `migrate` (the
//! legacy secret split), `iron` and `iron_config` (the engine pin and its config), `audit` and
//! `flowlog` (traffic capture).

pub mod apply;
pub mod audit;
pub mod flowlog;
pub mod github;
pub mod iron;
pub mod iron_config;
pub mod migrate;
mod remote_push;
mod secret_files;
pub mod sidecar;
pub mod store;
pub mod types;

use std::sync::Arc;

use mio_domain::types::ProviderId;
use mio_provider::types::MachineProvider;
use types::ProxyError;

pub const PROXY_PORT: u16 = 8080;

/// In-sandbox path the proxy's CA cert is installed at. Referenced by both `inject_ca` and
/// `proxy_env` — the two must name the same path.
pub const CONTAINER_CA_PATH: &str = "/usr/local/share/ca-certificates/mio-proxy.crt";

/// The sidecar image for `provider_id`: iron-proxy
/// (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 9).
pub fn proxy_image(provider_id: ProviderId) -> String {
    match provider_id {
        ProviderId::Podman => iron::iron_proxy_image(),
    }
}

pub fn proxy_container_name(sandbox: &str) -> String {
    format!("{sandbox}{}", mio_domain::PROXY_CONTAINER_SUFFIX)
}

pub fn proxy_network_name(sandbox: &str) -> String {
    format!("mio-proxy-{sandbox}")
}

/// Forward-slashed absolute path: `podman -v` rejects backslash Windows paths
/// (`src-tauri/AGENTS.md`).
fn host_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn io_err(path: &std::path::Path, e: std::io::Error) -> ProxyError {
    ProxyError::Io { path: path.to_string_lossy().to_string(), error: e.to_string() }
}

/// Writes `bytes` beside `path` and renames over it, so a reader never sees a partial file.
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), ProxyError> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}

/// The full `--env` list for a sandbox behind its sidecar: the proxy URL under both casings, `no_proxy`
/// under both spellings (loopback included, which also carries local-LLM inference through the
/// sandbox's own inference forward), and the per-runtime CA trust overrides pointing at
/// `CONTAINER_CA_PATH` (Resolved Decision 8). `CONTAINER_CA_PATH` is also referenced by `inject_ca`;
/// the two must name the same path.
pub fn proxy_env(proxy_ip: &str) -> Vec<(String, String)> {
    let proxy_url = format!("http://{proxy_ip}:{PROXY_PORT}");
    let no_proxy = "localhost,127.0.0.1,::1";
    let mut env = vec![
        ("http_proxy".to_string(), proxy_url.clone()),
        ("https_proxy".to_string(), proxy_url.clone()),
        ("HTTP_PROXY".to_string(), proxy_url.clone()),
        ("HTTPS_PROXY".to_string(), proxy_url),
        ("no_proxy".to_string(), no_proxy.to_string()),
        ("NO_PROXY".to_string(), no_proxy.to_string()),
    ];
    for var in [
        "NODE_EXTRA_CA_CERTS",
        "REQUESTS_CA_BUNDLE",
        "PIP_CERT",
        "SSL_CERT_FILE",
        "CURL_CA_BUNDLE",
        "GIT_SSL_CAINFO",
    ] {
        env.push((var.to_string(), CONTAINER_CA_PATH.to_string()));
    }
    env
}

/// Pipes the proxy's CA cert into the sandbox and runs `update-ca-certificates`, the transport
/// `credentials::tar_inject` already established. Starts the sandbox first, as
/// `credentials::write_file_via_stdin` does. `CONTAINER_CA_PATH` is also referenced by `proxy_env`;
/// the two must name the same path.
pub async fn inject_ca(
    sandbox_provider: &Arc<dyn MachineProvider>,
    sandbox_name: &str,
    pem: &[u8],
) -> Result<(), ProxyError> {
    sandbox_provider.start(sandbox_name).await?;
    let script = format!("cat >{CONTAINER_CA_PATH} && update-ca-certificates");
    let out = sandbox_provider.exec_piped(sandbox_name, &["sh", "-c", &script], pem).await?;
    if !out.status.success() {
        return Err(ProxyError::CommandFailed(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(())
}

/// Whether `instance` is a proxy sidecar rather than a sandbox. The name suffix alone is not enough:
/// a sandbox created before the suffix was reserved can be called `x-proxy`. Sandboxes run the
/// versioned `sandbox-mio/base:<n>` image and so carry an `image_version`; the sidecar image tags
/// (iron-proxy `0.56.0`) must never parse as one
/// (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 10).
pub fn is_sidecar(instance: &mio_domain::types::Sandbox) -> bool {
    instance.name.ends_with(mio_domain::PROXY_CONTAINER_SUFFIX) && instance.image_version.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(name: &str, image_version: Option<u32>) -> mio_domain::types::Sandbox {
        mio_domain::types::Sandbox {
            id: name.to_string(),
            name: name.to_string(),
            status: "running".to_string(),
            workspace_path: String::new(),
            image_version,
            provider: ProviderId::Podman,
        }
    }

    /// A sandbox named `x-proxy` (allowed before the suffix was reserved) is not `x`'s sidecar.
    #[test]
    fn sidecar_needs_the_suffix_and_an_unversioned_image() {
        assert!(is_sidecar(&instance("micro-proxy", None)));
        assert!(!is_sidecar(&instance("micro-proxy", Some(6))));
        assert!(!is_sidecar(&instance("micro", None)));
        assert!(!is_sidecar(&instance("micro", Some(6))));
    }

    #[test]
    fn new_names_may_not_end_in_the_sidecar_suffix() {
        assert!(matches!(
            mio_domain::validate_new_name("micro-proxy"),
            Err(mio_domain::types::BackendError::ReservedName { .. })
        ));
        assert!(mio_domain::validate_new_name("proxy-micro").is_ok());
        assert!(mio_domain::validate_new_name("micro").is_ok());
    }
}
