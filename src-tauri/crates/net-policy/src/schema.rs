//! The `.mio/net-policy.json` document types, reading and writing the file, the starter policy and
//! the conversion to the provider-neutral rendering input.

use std::path::{Path, PathBuf};

use super::NetPolicyError;
use mio_provider::types::{
    EgressMode, EgressPolicy, EgressRule, EgressTarget, NetGroup, NetProtocol, ProviderCapabilities,
    RuleAction, SecretBindingSpec, ViolationAction,
};

/// File name under the workspace's `.mio` directory.
pub const POLICY_FILE: &str = "net-policy.json";

/// Hosts every agent's toolchain needs, seeded into a new allowlist policy (Resolved Decision 13).
pub const STARTER_BASE_HOSTS: &[&str] = &[
    "registry.npmjs.org",
    "pypi.org",
    "files.pythonhosted.org",
    "github.com",
    "api.github.com",
    "codeload.github.com",
    "uploads.github.com",
    "gist.github.com",
    "*.githubusercontent.com",
    "nodejs.org",
    "mise.jdx.dev",
    "mise-versions.jdx.dev",
];

/// A port list entry: a single port, or an inclusive `"a-b"` range.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PortSpec {
    Single(u16),
    Range(String),
}

impl PortSpec {
    fn bounds(&self) -> Result<(u16, u16), NetPolicyError> {
        match self {
            PortSpec::Single(0) => Err(NetPolicyError("port 0 is not a valid port".to_string())),
            PortSpec::Single(p) => Ok((*p, *p)),
            PortSpec::Range(r) => {
                let bad = || NetPolicyError(format!("invalid port range {r:?} (expected \"a-b\")"));
                let (a, b) = r.split_once('-').ok_or_else(bad)?;
                let a: u16 = a.trim().parse().map_err(|_| bad())?;
                let b: u16 = b.trim().parse().map_err(|_| bad())?;
                if a == 0 || a > b {
                    return Err(bad());
                }
                Ok((a, b))
            }
        }
    }
}

/// One ordered rule. Exactly one of `host`, `cidr` and `group` is set.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetRule {
    pub action: RuleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cidr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<NetGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<NetProtocol>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<PortSpec>>,
}

impl NetRule {
    /// TCP 443 for an engine-enforced policy; no protocol or ports in the sidecar dialect, which
    /// refuses both.
    fn starter_allow(host: &str, caps: ProviderCapabilities) -> Self {
        let sidecar = caps.sidecar_egress;
        NetRule {
            action: RuleAction::Allow,
            host: Some(host.to_string()),
            cidr: None,
            group: None,
            protocol: (!sidecar).then_some(NetProtocol::Tcp),
            ports: (!sidecar).then(|| vec![PortSpec::Single(443)]),
        }
    }

    pub(super) fn target(&self) -> Result<EgressTarget, NetPolicyError> {
        match (&self.host, &self.cidr, &self.group) {
            (Some(h), None, None) => Ok(EgressTarget::Host(h.to_ascii_lowercase())),
            (None, Some(c), None) => Ok(EgressTarget::Cidr(c.clone())),
            (None, None, Some(g)) => Ok(EgressTarget::Group(*g)),
            _ => Err(NetPolicyError(
                "each rule needs exactly one of host, cidr or group".to_string(),
            )),
        }
    }

    pub(super) fn port_ranges(&self) -> Result<Vec<(u16, u16)>, NetPolicyError> {
        self.ports.iter().flatten().map(PortSpec::bounds).collect()
    }
}

pub(super) fn default_headers() -> Vec<String> {
    vec!["authorization".to_string()]
}

/// A stored secret bound to a guest variable and the hosts its value may reach. Holds the secret's
/// id only; values live in the sandbox's own `<app_data_root>/proxy/<sandbox>/values.json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretBinding {
    pub secret_id: String,
    pub env_var: String,
    pub hosts: Vec<String>,
    #[serde(default = "default_headers")]
    pub headers: Vec<String>,
    #[serde(default)]
    pub query: bool,
    #[serde(default)]
    pub body: bool,
}

fn default_version() -> u32 {
    1
}

fn default_true() -> bool {
    true
}

/// The whole `.mio/net-policy.json` document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetPolicy {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub egress: EgressMode,
    /// First match wins, in file order; `egress` is the fallback.
    #[serde(default)]
    pub rules: Vec<NetRule>,
    #[serde(default = "default_true")]
    pub strict_hostnames: bool,
    #[serde(default)]
    pub secrets: Vec<SecretBinding>,
    #[serde(default)]
    pub violation_action: ViolationAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny_message: Option<String>,
    /// Reserved for a future approval mode; refused for every provider
    /// (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive: Option<bool>,
}

impl Default for NetPolicy {
    fn default() -> Self {
        NetPolicy {
            version: 1,
            egress: EgressMode::Open,
            rules: Vec::new(),
            strict_hostnames: true,
            secrets: Vec::new(),
            violation_action: ViolationAction::BlockAndLog,
            deny_message: None,
            interactive: None,
        }
    }
}

fn policy_path(workspace_path: &str) -> PathBuf {
    mio_files::mio_path(workspace_path, POLICY_FILE)
}

fn parse(path: &Path) -> Result<Option<NetPolicy>, NetPolicyError> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(None),
        Ok(s) => serde_json::from_str(&s).map(Some).map_err(|e| {
            NetPolicyError(format!("failed to parse {}: {e}", path.to_string_lossy()))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(NetPolicyError(format!("failed to read {}: {e}", path.to_string_lossy()))),
    }
}

/// The workspace file as written, or `None` when it is absent or empty (nothing proposed).
pub fn load_if_present(workspace_path: &str) -> Result<Option<NetPolicy>, NetPolicyError> {
    parse(&policy_path(workspace_path))
}

pub fn save(workspace_path: &str, policy: &NetPolicy) -> Result<(), NetPolicyError> {
    Ok(mio_files::write_pretty(&policy_path(workspace_path), policy)?)
}

/// An allowlist policy holding [`STARTER_BASE_HOSTS`] plus `extra_hosts`, deduplicated, in the
/// dialect `caps` validates.
pub fn starter(caps: ProviderCapabilities, extra_hosts: &[String]) -> NetPolicy {
    let mut hosts: Vec<String> = Vec::new();
    for h in STARTER_BASE_HOSTS.iter().map(|h| h.to_string()).chain(extra_hosts.iter().cloned()) {
        let h = h.trim().to_ascii_lowercase();
        if !h.is_empty() && !hosts.contains(&h) {
            hosts.push(h);
        }
    }
    NetPolicy {
        egress: EgressMode::Allowlist,
        rules: hosts.iter().map(|h| NetRule::starter_allow(h, caps)).collect(),
        ..NetPolicy::default()
    }
}

/// The non-secret value a sandbox sees in a binding's variable until the sidecar swaps it
/// (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 16).
pub fn placeholder(env_var: &str) -> String {
    format!("mio-placeholder-{}", env_var.to_ascii_lowercase())
}

/// The provider-neutral rendering input. Call [`super::validate::validate`] first: an invalid rule
/// is an error here too, never silently dropped.
pub fn to_egress(policy: &NetPolicy) -> Result<EgressPolicy, NetPolicyError> {
    let rules = policy
        .rules
        .iter()
        .map(|r| {
            Ok(EgressRule {
                action: r.action,
                target: r.target()?,
                protocol: r.protocol,
                ports: r.port_ranges()?,
            })
        })
        .collect::<Result<Vec<_>, NetPolicyError>>()?;
    Ok(EgressPolicy {
        mode: policy.egress,
        rules,
        strict_hostnames: policy.strict_hostnames,
        secrets: policy
            .secrets
            .iter()
            .map(|b| SecretBindingSpec {
                env_var: b.env_var.clone(),
                hosts: b.hosts.iter().map(|h| h.to_ascii_lowercase()).collect(),
                headers: b.headers.clone(),
                query: b.query,
                body: b.body,
            })
            .collect(),
        violation_action: policy.violation_action,
        deny_message: policy.deny_message.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_support::ENGINE;
    use super::super::validate::validate;
    use super::*;

    #[test]
    fn minimal_file_fills_defaults() {
        let p: NetPolicy = serde_json::from_str("{}").unwrap();
        assert_eq!(p, NetPolicy::default());
        let p: NetPolicy = serde_json::from_str(
            r#"{"egress":"allowlist","rules":[{"action":"allow","host":"github.com","protocol":"tcp","ports":[443,"8000-8100"]}],"secrets":[{"secretId":"gh","envVar":"GH_TOKEN","hosts":["github.com"]}],"violationAction":"block-and-terminate"}"#,
        )
        .unwrap();
        assert_eq!(p.egress, EgressMode::Allowlist);
        assert_eq!(p.secrets[0].headers, ["authorization"]);
        assert_eq!(p.violation_action, ViolationAction::BlockAndTerminate);
        assert_eq!(p.rules[0].port_ranges().unwrap(), [(443, 443), (8000, 8100)]);
        validate(&p, ENGINE).unwrap();
    }

    #[test]
    fn starter_is_allowlist_with_deduplicated_hosts() {
        let p = starter(ENGINE, &["api.anthropic.com".to_string(), "GitHub.com".to_string()]);
        assert_eq!(p.egress, EgressMode::Allowlist);
        assert_eq!(p.rules.len(), STARTER_BASE_HOSTS.len() + 1);
        assert!(p.rules.iter().all(|r| r.protocol == Some(NetProtocol::Tcp)));
        validate(&p, ENGINE).unwrap();
    }

    #[test]
    fn converts_to_the_provider_shape() {
        let p = NetPolicy { secrets: vec![super::super::test_support::binding("api.github.com")], ..starter(ENGINE, &[]) };
        let e = to_egress(&p).unwrap();
        assert_eq!(e.mode, EgressMode::Allowlist);
        assert_eq!(e.rules.len(), p.rules.len());
        assert_eq!(e.secrets[0].env_var, "GH_TOKEN");
        assert_eq!(e.rules[0].ports, [(443, 443)]);
    }
}
