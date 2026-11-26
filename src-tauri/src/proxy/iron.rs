//! The iron-proxy engine of the Podman sidecar: image pin, CA, and the config file the sidecar reads
//! at boot (`docs/features/podman-egress-boundary/02_dev.md`, Phase D). A restart applies the last
//! written state.

use std::collections::HashMap;
use std::path::Path;

use super::secret_files::planned_files;
use super::store::config_dir;
use super::types::ProxyError;
use super::{io_err, iron_config, write_atomic};
use crate::net_policy::schema::NetPolicy;

/// Pinned to the version Phase A ran, never `latest` or an `-rc.N` tag. The tag must not parse as
/// an integer, or `is_sidecar` mistakes the sidecar for a sandbox. An upgrade re-runs the Phase A
/// spikes first (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 10).
pub const IRON_PROXY_IMAGE_DEFAULT: &str = "docker.io/ironsh/iron-proxy:0.56.0";

/// Caps the sidecar's `k8s-file` log, which is truncated to zero at the cap, so it must hold far
/// more than one poll's worth of audit rows (about 1 KB each; Resolved Decision 4).
pub const SIDECAR_LOG_MAX_SIZE: &str = "4mb";

/// The directory the sidecar keeps `config/`, `certs/` and `secrets/` under.
pub const CONTAINER_STATE_DIR: &str = "/etc/iron-proxy";
pub const CONTAINER_CONFIG_DIR: &str = "/etc/iron-proxy/config";

pub fn iron_proxy_image() -> String {
    std::env::var("MIO_IRON_PROXY_IMAGE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| IRON_PROXY_IMAGE_DEFAULT.to_string())
}

/// A fresh ECDSA P-256 CA with ten-year validity, as `ca.pem` and `ca-key.pem` in `certs_dir`
/// (Resolved Decision 11). The same `ca.pem` is mounted into the sidecar and piped into the
/// sandbox by `inject_ca`.
pub fn generate_ca(certs_dir: &Path) -> Result<(), ProxyError> {
    use rcgen::{
        BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
        KeyUsagePurpose,
    };
    let failed = |e: rcgen::Error| ProxyError::CommandFailed(format!("CA generation failed: {e}"));
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(failed)?;
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Sandbox-mio proxy CA");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    // Backdated a day so a guest clock slightly behind the host still accepts it.
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(3650);
    let cert = params.self_signed(&key).map_err(failed)?;
    write_atomic(&certs_dir.join("ca-key.pem"), key.serialize_pem().as_bytes())?;
    write_atomic(&certs_dir.join("ca.pem"), cert.pem().as_bytes())
}

/// Writes the bootstrap config the sidecar boots with (Resolved Decision 18).
pub fn write_bootstrap(state: &Path, proxy_ip: &str) -> Result<(), ProxyError> {
    let dir = config_dir(state);
    std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    write_config(&dir.join("config.yaml"), &iron_config::bootstrap(proxy_ip))
}

/// JSON in a `.yaml` file: YAML 1.2 is a superset of JSON (Resolved Decision 12).
pub(super) fn write_config(path: &Path, config: &serde_json::Value) -> Result<(), ProxyError> {
    let body = serde_json::to_vec_pretty(config)
        .map_err(|e| ProxyError::CommandFailed(format!("failed to render config: {e}")))?;
    write_atomic(path, &body)
}

/// The app-owned bindings to render for a sandbox.
pub(super) fn app_bindings(github: bool) -> Vec<iron_config::AppBinding> {
    if github { vec![iron_config::github_app_binding()] } else { Vec::new() }
}

/// The config an apply would write, with the workspace bindings that have a stored value.
pub(super) fn rendered(
    policy: &NetPolicy,
    proxy_ip: &str,
    values: &HashMap<String, String>,
    github: bool,
) -> serde_json::Value {
    let written = planned_files(policy, values, github);
    iron_config::render(policy, proxy_ip, &written, &app_bindings(github))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two sites render one policy, differing only in the sidecar address the DNS answers with.
    #[test]
    fn two_sites_render_configs_that_differ_only_in_the_proxy_address() {
        let policy: NetPolicy = serde_json::from_str("{}").unwrap();
        let mut local = rendered(&policy, "10.90.1.2", &HashMap::new(), false);
        let mut remote = rendered(&policy, "10.90.7.2", &HashMap::new(), false);
        assert_ne!(local, remote);
        local["dns"]["proxy_ip"] = serde_json::Value::Null;
        remote["dns"]["proxy_ip"] = serde_json::Value::Null;
        assert_eq!(local, remote);
    }
}
