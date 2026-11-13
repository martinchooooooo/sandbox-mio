//! The Podman iron-proxy sidecar: image pin, CA, secret files, and policy apply
//! (`docs/features/podman-egress-boundary/02_dev.md`, Phase D). The sidecar reads `config.yaml`
//! and the secret files at boot, so a restart applies the last written state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::AppHandle;

use super::iron_config::{self, IronListen, SecretFile};
use super::types::ProxyError;
use crate::net_policy::{self, NetPolicy};
use crate::provider::types::{ExecOpts, MachineProvider, ProviderId};
use crate::provider::ProviderRegistry;

/// Pinned to the version Phase A ran, never `latest` or an `-rc.N` tag. The tag must not parse as
/// an integer, or `is_sidecar` mistakes the sidecar for a sandbox. An upgrade re-runs the Phase A
/// spikes first (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 10).
pub const IRON_PROXY_IMAGE_DEFAULT: &str = "docker.io/ironsh/iron-proxy:0.56.0";

/// The management listener's port on the sidecar's loopback.
pub const IRON_MGMT_PORT: u16 = 9092;

/// Caps the sidecar's `k8s-file` log, which is truncated to zero at the cap, so it must hold far
/// more than one poll's worth of audit rows (about 1 KB each; Resolved Decision 4).
pub const SIDECAR_LOG_MAX_SIZE: &str = "4mb";

/// The mount point of the state directory's `config/`, which holds `config.yaml`.
pub const CONTAINER_CONFIG_DIR: &str = "/etc/iron-proxy/config";

/// The sidecar-reported reason for a rejected reload; busybox `wget` hides the 422 body.
const RELOAD_REJECTED_MSG: &str = "management reload rejected invalid config";

pub fn iron_proxy_image() -> String {
    std::env::var("MIO_IRON_PROXY_IMAGE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| IRON_PROXY_IMAGE_DEFAULT.to_string())
}

/// What an apply could not do without failing it: bindings left out because their secret has no
/// stored value, so their placeholder passes through unswapped (Resolved Decision 15).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    /// The `envVar` of each binding without a stored value.
    pub missing_secrets: Vec<String>,
}

fn io_err(path: &Path, e: std::io::Error) -> ProxyError {
    ProxyError::Io { path: path.to_string_lossy().to_string(), error: e.to_string() }
}

/// Writes `bytes` beside `path` and renames over it, so the sidecar never reads a partial file
/// through the directory mount.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ProxyError> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
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

/// Writes one file per binding whose secret has a stored value into `dir`, holding the value's
/// exact bytes: a `file` source sends the content verbatim, so a trailing newline would reach the
/// upstream (Resolved Decision 15). Returns the files written and the variables left out.
pub fn materialise_secret_files(
    dir: &Path,
    policy: &NetPolicy,
    values: &HashMap<String, String>,
) -> Result<(Vec<SecretFile>, Vec<String>), ProxyError> {
    std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let mut files = Vec::new();
    let mut missing = Vec::new();
    for binding in &policy.secrets {
        match values.get(&binding.secret_id) {
            Some(value) => {
                write_atomic(&dir.join(&binding.env_var), value.as_bytes())?;
                files.push(SecretFile::new(&binding.env_var));
            }
            None => missing.push(binding.env_var.clone()),
        }
    }
    Ok((files, missing))
}

/// Deletes secret files no rendered binding names, so an unbound value does not stay on disk.
/// Runs only after a successful reload: the previous pipeline may still read them until then.
fn remove_stale_secret_files(dir: &Path, keep: &[SecretFile]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !keep.iter().any(|f| f.env_var == name) {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                tracing::warn!(target: "proxy", file = %name, error = %e, "failed to remove a stale secret file");
            }
        }
    }
}

fn config_dir(state: &Path) -> PathBuf {
    state.join("config")
}

pub fn secrets_dir(state: &Path) -> PathBuf {
    state.join("secrets")
}

/// Writes the bootstrap config the sidecar boots with (Resolved Decision 18).
pub fn write_bootstrap(state: &Path, listen: &IronListen) -> Result<(), ProxyError> {
    let dir = config_dir(state);
    std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    write_config(&dir.join("config.yaml"), &iron_config::bootstrap(listen))
}

/// JSON in a `.yaml` file: YAML 1.2 is a superset of JSON (Resolved Decision 12).
fn write_config(path: &Path, config: &serde_json::Value) -> Result<(), ProxyError> {
    let body = serde_json::to_vec_pretty(config)
        .map_err(|e| ProxyError::CommandFailed(format!("failed to render config: {e}")))?;
    write_atomic(path, &body)
}

/// Asks the running sidecar to reload `config.yaml`. The bearer token is read from the sidecar's
/// own `IRON_MANAGEMENT_API_KEY`, so it never enters a command line.
async fn reload(provider: &Arc<dyn MachineProvider>, sidecar: &str) -> Result<(), ProxyError> {
    let script = format!(
        "wget -q -O - --header \"Authorization: Bearer ${env}\" --post-data= \
         http://127.0.0.1:{IRON_MGMT_PORT}/v1/reload",
        env = iron_config::MGMT_API_KEY_ENV,
    );
    let out = provider.exec(sidecar, &["sh", "-c", &script], ExecOpts::default()).await?;
    if out.status.success() {
        return Ok(());
    }
    let wget_err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let reason = rejection_reason(provider, sidecar).await;
    Err(ProxyError::CommandFailed(match reason {
        Some(reason) => format!("the proxy rejected the new config: {reason}"),
        None => format!("the proxy reload failed: {wget_err}"),
    }))
}

/// The `error` of the newest rejected-reload line in the sidecar log.
async fn rejection_reason(provider: &Arc<dyn MachineProvider>, sidecar: &str) -> Option<String> {
    let lines = provider.sidecar_logs(sidecar, None, Some(50)).await.ok()?;
    lines.iter().rev().filter(|l| l.contains(RELOAD_REJECTED_MSG)).find_map(|line| {
        let json = &line[line.find('{')?..];
        let record: serde_json::Value = serde_json::from_str(json).ok()?;
        record["error"].as_str().map(str::to_string)
    })
}

async fn is_running(
    provider: &Arc<dyn MachineProvider>,
    sidecar: &str,
) -> Result<bool, ProxyError> {
    Ok(provider.list_instances().await?.iter().any(|s| s.name == sidecar && s.status == "running"))
}

/// Renders the workspace's `.mio/net-policy.json` into the sidecar's config and makes the sidecar
/// run it: reload when running, start when stopped (Resolved Decision 13). A rejected reload
/// restores `config.prev.yaml` and never restarts, so the file on disk always matches the running
/// pipeline. `applied.json` is rewritten only once the sidecar runs the new policy.
pub async fn apply_policy(
    app: &AppHandle,
    registry: &Arc<ProviderRegistry>,
    sandbox: &str,
    workspace_path: &str,
) -> Result<ApplyReport, ProxyError> {
    let state = super::state_dir(app, sandbox)?;
    let meta = super::read_meta(app, sandbox).filter(|m| m["engine"] == super::ENGINE_IRON_PROXY);
    let Some(meta) = meta else {
        return Err(ProxyError::CommandFailed(format!(
            "{sandbox:?} has no iron-proxy sidecar; recreate the sandbox to get the network boundary"
        )));
    };
    let proxy_ip = meta["proxyIp"].as_str().unwrap_or_default().to_string();
    let provider = registry.local(ProviderId::Podman);
    let policy_err = |e: net_policy::NetPolicyError| ProxyError::CommandFailed(e.0);
    let policy = net_policy::load(workspace_path).map_err(policy_err)?;
    net_policy::validate(&policy, provider.capabilities()).map_err(policy_err)?;

    let secrets = secrets_dir(&state);
    let (files, missing_secrets) =
        materialise_secret_files(&secrets, &policy, &super::load_secrets(app)?)?;
    let listen = IronListen { proxy_ip, tunnel_port: super::PROXY_PORT, mgmt_port: IRON_MGMT_PORT };
    let dir = config_dir(&state);
    let current = dir.join("config.yaml");
    let prev = dir.join("config.prev.yaml");
    std::fs::copy(&current, &prev).map_err(|e| io_err(&current, e))?;
    write_config(&current, &iron_config::render(&policy, &listen, &files))?;

    let sidecar = super::proxy_container_name(sandbox);
    let outcome = if is_running(&provider, &sidecar).await? {
        reload(&provider, &sidecar).await
    } else {
        provider.start(&sidecar).await.map_err(ProxyError::from)
    };
    if let Err(e) = outcome {
        if let Err(restore) = std::fs::rename(&prev, &current) {
            tracing::error!(target: "proxy", %sandbox, error = %restore, "failed to restore the previous proxy config");
        }
        return Err(e);
    }
    remove_stale_secret_files(&secrets, &files);
    let applied = net_policy::AppliedPolicy { policy, chained_proxy: false };
    net_policy::write_applied(app, sandbox, &applied).map_err(policy_err)?;
    if !missing_secrets.is_empty() {
        tracing::warn!(target: "proxy", %sandbox, ?missing_secrets, "bindings without a stored secret were not applied");
    }
    Ok(ApplyReport { missing_secrets })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trailing newline in a secret file would be sent upstream inside the credential.
    #[test]
    fn secret_files_hold_exact_bytes_and_skip_missing_values() {
        let dir = std::env::temp_dir().join(format!("mio-iron-test-{}", uuid::Uuid::new_v4()));
        let policy: NetPolicy = serde_json::from_str(
            r#"{"secrets":[{"secretId":"a","envVar":"A_KEY","hosts":["a.example.com"]},
                           {"secretId":"b","envVar":"B_KEY","hosts":["b.example.com"]}]}"#,
        )
        .unwrap();
        let values = HashMap::from([("a".to_string(), "sk-123".to_string())]);
        let (files, missing) = materialise_secret_files(&dir, &policy, &values).unwrap();
        assert_eq!(std::fs::read(dir.join("A_KEY")).unwrap(), b"sk-123");
        assert_eq!(files, [SecretFile::new("A_KEY")]);
        assert_eq!(missing, ["B_KEY"]);
        assert!(!dir.join("B_KEY").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
