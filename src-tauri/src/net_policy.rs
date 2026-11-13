//! `.mio/net-policy.json`: the shared, provider-neutral outbound policy of one workspace, plus the
//! per-sandbox applied snapshot under the app data root. A plain file module like
//! [`crate::mio_files`], so `backends`, `proxy` and `services` may all use it. Schema:
//! `docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 12.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Runtime};

use crate::backends::types::BackendError;
use crate::mio_files;
use crate::provider::types::{
    ChainedProxy, EgressPolicy, EgressRule, EgressTarget, ProviderCapabilities, SecretBindingSpec,
};

pub use crate::provider::types::{EgressMode, NetGroup, NetProtocol, RuleAction, ViolationAction};

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
    "*.githubusercontent.com",
    "nodejs.org",
    "mise.jdx.dev",
    "mise-versions.jdx.dev",
];

/// Variable names a secret binding may not claim: the guest shell needs the first two, the chained
/// proxy owns the third, and microsandbox reserves the `MSB_` prefix for its placeholders.
const RESERVED_ENV: &[&str] = &["PATH", "HOME", crate::provider::msb_policy::SOCKS_PASSWORD_ENV];

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

    fn target(&self) -> Result<EgressTarget, NetPolicyError> {
        match (&self.host, &self.cidr, &self.group) {
            (Some(h), None, None) => Ok(EgressTarget::Host(h.to_ascii_lowercase())),
            (None, Some(c), None) => Ok(EgressTarget::Cidr(c.clone())),
            (None, None, Some(g)) => Ok(EgressTarget::Group(*g)),
            _ => Err(NetPolicyError(
                "each rule needs exactly one of host, cidr or group".to_string(),
            )),
        }
    }

    fn port_ranges(&self) -> Result<Vec<(u16, u16)>, NetPolicyError> {
        self.ports.iter().flatten().map(PortSpec::bounds).collect()
    }
}

fn default_headers() -> Vec<String> {
    vec!["authorization".to_string()]
}

/// A stored secret bound to a guest variable and the hosts its value may reach. Holds the secret's
/// id only; values live in `<app_data_root>/proxy/secrets.json`.
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

/// The workspace's policy; an absent or empty file is the default open policy.
pub fn load(workspace_path: &str) -> Result<NetPolicy, NetPolicyError> {
    Ok(parse(&policy_path(workspace_path))?.unwrap_or_default())
}

pub fn save(workspace_path: &str, policy: &NetPolicy) -> Result<(), NetPolicyError> {
    Ok(mio_files::write_pretty(&policy_path(workspace_path), policy)?)
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn valid_exact_host(host: &str) -> bool {
    host.len() <= 253 && host.contains('.') && host.split('.').all(valid_label)
}

/// An exact hostname, or `*.suffix` whose suffix has at least two labels (microsandbox rejects
/// `*.com`).
fn validate_host(host: &str) -> Result<(), NetPolicyError> {
    let ok = match host.strip_prefix("*.") {
        Some(suffix) => valid_exact_host(suffix),
        None => valid_exact_host(host),
    };
    if ok {
        Ok(())
    } else {
        Err(NetPolicyError(format!("invalid host {host:?}")))
    }
}

fn validate_cidr(cidr: &str) -> Result<(), NetPolicyError> {
    let bad = || NetPolicyError(format!("invalid CIDR {cidr:?}"));
    let (ip, prefix) = cidr.split_once('/').unwrap_or((cidr, ""));
    let ip: std::net::IpAddr = ip.parse().map_err(|_| bad())?;
    if prefix.is_empty() {
        return Ok(());
    }
    let prefix: u8 = prefix.parse().map_err(|_| bad())?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    if prefix > max {
        return Err(bad());
    }
    Ok(())
}

fn valid_env_var(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase() || c == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Whether an allow rule covers `host` (exactly, or through a `*.suffix` rule).
fn allowed_by_rule(policy: &NetPolicy, host: &str) -> bool {
    policy.rules.iter().any(|r| {
        r.action == RuleAction::Allow
            && r.host.as_deref().is_some_and(|h| {
                let h = h.to_ascii_lowercase();
                match h.strip_prefix("*.") {
                    Some(suffix) => host.ends_with(&format!(".{suffix}")),
                    None => h == host,
                }
            })
    })
}

/// The sidecar dialect: what iron-proxy can enforce
/// (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 6).
fn validate_sidecar_dialect(policy: &NetPolicy) -> Result<(), NetPolicyError> {
    if policy.violation_action != ViolationAction::BlockAndLog {
        return Err(NetPolicyError(
            "violation actions are not available for Podman sandboxes".to_string(),
        ));
    }
    for rule in &policy.rules {
        let what = match (rule.action, rule.target()?) {
            (RuleAction::Allow, EgressTarget::Host(_))
            | (RuleAction::Deny, EgressTarget::Cidr(_)) => None,
            (RuleAction::Allow, EgressTarget::Cidr(c)) => Some(format!(
                "allow rule {c:?}: Podman sandboxes allow hostnames only, not CIDR ranges"
            )),
            (RuleAction::Deny, EgressTarget::Host(h)) => Some(format!(
                "deny rule {h:?}: Podman sandboxes deny CIDR ranges only, not hostnames"
            )),
            (_, EgressTarget::Group(_)) => {
                Some("group rules are not available for Podman sandboxes".to_string())
            }
        };
        if let Some(what) = what {
            return Err(NetPolicyError(what));
        }
        if rule.protocol == Some(NetProtocol::Udp) {
            return Err(NetPolicyError("Podman sandboxes cannot allow or deny UDP".to_string()));
        }
        if rule.ports.is_some() {
            return Err(NetPolicyError(
                "port lists are not available for Podman sandboxes".to_string(),
            ));
        }
    }
    Ok(())
}

/// Refuses a policy the provider cannot enforce exactly as written.
pub fn validate(policy: &NetPolicy, caps: ProviderCapabilities) -> Result<(), NetPolicyError> {
    if policy.version != 1 {
        return Err(NetPolicyError(format!("unsupported policy version {}", policy.version)));
    }
    if policy.interactive == Some(true) {
        return Err(NetPolicyError("interactive approval is not available".to_string()));
    }
    for rule in &policy.rules {
        match rule.target()? {
            EgressTarget::Host(h) => validate_host(&h)?,
            EgressTarget::Cidr(c) => validate_cidr(&c)?,
            EgressTarget::Group(_) => {}
        }
        rule.port_ranges()?;
    }
    if caps.sidecar_egress {
        validate_sidecar_dialect(policy)?;
    }
    let mut seen = std::collections::HashSet::new();
    for binding in &policy.secrets {
        if binding.secret_id.trim().is_empty() {
            return Err(NetPolicyError("a secret binding has no secret id".to_string()));
        }
        let var = &binding.env_var;
        if !valid_env_var(var) {
            return Err(NetPolicyError(format!(
                "invalid variable name {var:?} (use A-Z, 0-9 and _, not starting with a digit)"
            )));
        }
        if RESERVED_ENV.contains(&var.as_str()) || var.starts_with("MSB_") {
            return Err(NetPolicyError(format!("variable name {var:?} is reserved")));
        }
        if !seen.insert(var.clone()) {
            return Err(NetPolicyError(format!("variable name {var:?} is bound twice")));
        }
        if binding.hosts.is_empty() {
            return Err(NetPolicyError(format!("secret binding {var:?} lists no host")));
        }
        for host in &binding.hosts {
            if host.starts_with("*.") || !valid_exact_host(host) {
                return Err(NetPolicyError(format!(
                    "secret binding {var:?}: {host:?} must be an exact hostname"
                )));
            }
            if policy.egress == EgressMode::Allowlist
                && !allowed_by_rule(policy, &host.to_ascii_lowercase())
            {
                return Err(NetPolicyError(format!(
                    "secret binding {var:?}: {host:?} is not allowed by any rule"
                )));
            }
        }
    }
    Ok(())
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

/// The provider-neutral rendering input. Call [`validate`] first: an invalid rule is an error here
/// too, never silently dropped.
pub fn to_egress(
    policy: &NetPolicy,
    proxy: Option<ChainedProxy>,
) -> Result<EgressPolicy, NetPolicyError> {
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
        proxy,
    })
}

/// What a sandbox was actually created with: the policy plus whether a chained proxy was attached.
/// Drives the start environment and drift detection (Resolved Decision 18).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPolicy {
    pub policy: NetPolicy,
    #[serde(default)]
    pub chained_proxy: bool,
}

fn applied_path<R: Runtime>(app: &AppHandle<R>, sandbox: &str) -> Result<PathBuf, NetPolicyError> {
    let root = crate::paths::app_data_root(app)
        .map_err(|e| NetPolicyError(format!("app data directory unavailable: {e}")))?;
    Ok(root.join("net-policy").join(sandbox).join("applied.json"))
}

pub fn write_applied<R: Runtime>(
    app: &AppHandle<R>,
    sandbox: &str,
    applied: &AppliedPolicy,
) -> Result<(), NetPolicyError> {
    Ok(mio_files::write_pretty(&applied_path(app, sandbox)?, applied)?)
}

/// The applied snapshot, or `None` for a sandbox created without one (sidecars, Podman sandboxes
/// without a proxy, sandboxes predating this feature).
pub fn read_applied<R: Runtime>(
    app: &AppHandle<R>,
    sandbox: &str,
) -> Result<Option<AppliedPolicy>, NetPolicyError> {
    let path = applied_path(app, sandbox)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).map(Some).map_err(|e| {
            NetPolicyError(format!("failed to parse {}: {e}", path.to_string_lossy()))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(NetPolicyError(format!("failed to read {}: {e}", path.to_string_lossy()))),
    }
}

/// Best-effort: a leftover snapshot only matters if a sandbox of the same name is created again,
/// and that create overwrites it.
pub fn remove_applied<R: Runtime>(app: &AppHandle<R>, sandbox: &str) {
    if let Ok(path) = applied_path(app, sandbox) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MSB: ProviderCapabilities = ProviderCapabilities {
        remote_targets: false,
        event_stream: false,
        host_bind_mount: false,
        egress_policy: true,
        sidecar_egress: false,
    };

    const PODMAN: ProviderCapabilities =
        ProviderCapabilities { egress_policy: false, sidecar_egress: true, ..MSB };

    fn binding(host: &str) -> SecretBinding {
        SecretBinding {
            secret_id: "gh".to_string(),
            env_var: "GH_TOKEN".to_string(),
            hosts: vec![host.to_string()],
            headers: default_headers(),
            query: false,
            body: false,
        }
    }

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
        validate(&p, MSB).unwrap();
    }

    #[test]
    fn starter_is_allowlist_with_deduplicated_hosts() {
        let p = starter(MSB, &["api.anthropic.com".to_string(), "GitHub.com".to_string()]);
        assert_eq!(p.egress, EgressMode::Allowlist);
        assert_eq!(p.rules.len(), STARTER_BASE_HOSTS.len() + 1);
        assert!(p.rules.iter().all(|r| r.protocol == Some(NetProtocol::Tcp)));
        validate(&p, MSB).unwrap();
    }

    #[test]
    fn rejects_bad_targets_and_ports() {
        let mut p = starter(MSB, &[]);
        p.rules[0].host = Some("*.com".to_string());
        assert!(validate(&p, MSB).is_err());
        let mut p = starter(MSB, &[]);
        p.rules[0].cidr = Some("10.0.0.0/8".to_string());
        assert!(validate(&p, MSB).is_err(), "two targets on one rule");
        let mut p = starter(MSB, &[]);
        p.rules[0].ports = Some(vec![PortSpec::Range("90-80".to_string())]);
        assert!(validate(&p, MSB).is_err());
        let mut p = starter(MSB, &[]);
        p.rules[0].host = None;
        p.rules[0].cidr = Some("10.0.0.0/33".to_string());
        assert!(validate(&p, MSB).is_err());
    }

    #[test]
    fn rejects_bad_secret_bindings() {
        let base = starter(MSB, &[]);
        let with = |b: SecretBinding| NetPolicy { secrets: vec![b], ..base.clone() };
        validate(&with(binding("api.github.com")), MSB).unwrap();
        assert!(validate(&with(binding("example.com")), MSB).is_err(), "not allowed by a rule");
        assert!(validate(&with(binding("*.github.com")), MSB).is_err(), "wildcard secret host");
        for var in ["PATH", "MIO_SOCKS_PASSWORD", "MSB_X", "lower", "1ABC"] {
            let b = SecretBinding { env_var: var.to_string(), ..binding("github.com") };
            assert!(validate(&with(b), MSB).is_err(), "{var} must be refused");
        }
        let twice = NetPolicy {
            secrets: vec![binding("github.com"), binding("api.github.com")],
            ..base.clone()
        };
        assert!(validate(&twice, MSB).is_err());
        let open = NetPolicy { secrets: vec![binding("example.com")], ..NetPolicy::default() };
        validate(&open, MSB).unwrap();
    }

    #[test]
    fn interactive_is_refused_for_every_provider() {
        let p = NetPolicy { interactive: Some(true), ..NetPolicy::default() };
        assert!(validate(&p, MSB).is_err());
        assert!(validate(&p, PODMAN).is_err());
        let p = NetPolicy { interactive: Some(false), ..NetPolicy::default() };
        validate(&p, PODMAN).unwrap();
    }

    fn rule(action: RuleAction) -> NetRule {
        NetRule { action, host: None, cidr: None, group: None, protocol: None, ports: None }
    }

    #[test]
    fn podman_dialect_rejects_what_the_sidecar_cannot_enforce() {
        let host = |action| NetRule { host: Some("example.com".to_string()), ..rule(action) };
        let cidr = |action| NetRule { cidr: Some("10.0.0.0/8".to_string()), ..rule(action) };
        let refused = [
            ("allow cidr", cidr(RuleAction::Allow)),
            ("deny host", host(RuleAction::Deny)),
            ("group", NetRule { group: Some(NetGroup::Public), ..rule(RuleAction::Allow) }),
            ("udp", NetRule { protocol: Some(NetProtocol::Udp), ..host(RuleAction::Allow) }),
            (
                "ports",
                NetRule { ports: Some(vec![PortSpec::Single(443)]), ..host(RuleAction::Allow) },
            ),
            ("empty ports", NetRule { ports: Some(vec![]), ..host(RuleAction::Allow) }),
        ];
        for (what, r) in refused {
            let p = NetPolicy { rules: vec![r.clone()], ..NetPolicy::default() };
            assert!(validate(&p, PODMAN).is_err(), "{what} must be refused for Podman");
            validate(&p, MSB).unwrap_or_else(|e| panic!("{what} is valid for microsandbox: {e}"));
        }
        for action in [ViolationAction::Block, ViolationAction::BlockAndTerminate] {
            let p = NetPolicy { violation_action: action, ..NetPolicy::default() };
            assert!(validate(&p, PODMAN).is_err(), "{action:?} must be refused for Podman");
        }
    }

    #[test]
    fn converts_to_the_provider_shape() {
        let p = NetPolicy { secrets: vec![binding("api.github.com")], ..starter(MSB, &[]) };
        let e = to_egress(&p, None).unwrap();
        assert_eq!(e.mode, EgressMode::Allowlist);
        assert_eq!(e.rules.len(), p.rules.len());
        assert_eq!(e.secrets[0].env_var, "GH_TOKEN");
        assert_eq!(e.rules[0].ports, [(443, 443)]);
    }
}
