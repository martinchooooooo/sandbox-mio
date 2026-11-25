//! `.mio/net-policy.json`: the shared, provider-neutral outbound policy of one workspace, plus the
//! per-sandbox applied snapshot under the app data root. A plain file module like
//! [`crate::mio_files`], so `backends`, `proxy` and `services` may all use it. Schema:
//! `docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 12.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Runtime};

use crate::domain::types::BackendError;
use crate::mio_files;
use crate::provider::types::{
    EgressPolicy, EgressRule, EgressTarget, ProviderCapabilities, SecretBindingSpec,
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
    "uploads.github.com",
    "gist.github.com",
    "*.githubusercontent.com",
    "nodejs.org",
    "mise.jdx.dev",
    "mise-versions.jdx.dev",
];

/// Variable names a secret binding may not claim: the guest shell, loaders, language runtimes and
/// the sidecar's own `proxy_env` set rely on them, so a placeholder would break tooling or the proxy.
const RESERVED_ENV: &[&str] = &[
    "PATH", "HOME", "USER", "PWD", "SHELL", "TERM", "ENV", "BASH_ENV",
    "LD_PRELOAD", "LD_LIBRARY_PATH", "LD_AUDIT",
    "NODE_OPTIONS", "NODE_PATH", "NODE_EXTRA_CA_CERTS",
    "PYTHONPATH", "PYTHONSTARTUP", "PYTHONHOME", "PERL5OPT", "RUBYOPT", "JAVA_TOOL_OPTIONS",
    "GIT_SSH_COMMAND", "GIT_SSL_CAINFO", "SSL_CERT_FILE", "REQUESTS_CA_BUNDLE", "PIP_CERT", "CURL_CA_BUNDLE",
    "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy",
    "PASEO_PASSWORD",
];

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

/// An exact hostname, or `*.suffix` whose suffix has at least two labels (`*.com` is refused).
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

/// Whether an allow rule covers `host` (exactly, or through a `*.suffix` rule). `wildcard_apex`
/// follows the enforcer: iron-proxy's `*.example.com` also matches `example.com`, an engine-enforced
/// dialect's may not (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 6).
fn allowed_by_rule(policy: &NetPolicy, host: &str, wildcard_apex: bool) -> bool {
    policy.rules.iter().any(|r| {
        r.action == RuleAction::Allow
            && r.host.as_deref().is_some_and(|h| {
                let h = h.to_ascii_lowercase();
                match h.strip_prefix("*.") {
                    Some(suffix) => {
                        host.ends_with(&format!(".{suffix}")) || (wildcard_apex && host == suffix)
                    }
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
    // iron-proxy answers a blocked request with its own fixed response. `strictHostnames` stays
    // accepted: the boundary lets no connection out without a hostname, stricter than either value.
    if policy.deny_message.is_some() {
        return Err(NetPolicyError(
            "a custom deny message is not available for Podman sandboxes".to_string(),
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
        if RESERVED_ENV.contains(&var.as_str()) {
            return Err(NetPolicyError(format!(
                "variable name {var:?} is reserved: the sandbox's shell, runtimes or proxy setup depend on it"
            )));
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
                && !allowed_by_rule(policy, &host.to_ascii_lowercase(), caps.sidecar_egress)
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

/// What changed between two policies, as the review dialog lists it. `widens` marks a change that
/// can enlarge what the sandbox reaches or which hosts see a real secret value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyChange {
    #[serde(flatten)]
    pub detail: ChangeDetail,
    pub text: String,
    pub widens: bool,
    /// For a change that exposes a real secret value: which hosts receive which stored value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
}

/// The kind of one [`PolicyChange`], with the data the dialog shows.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ChangeDetail {
    ModeChanged { from: EgressMode, to: EgressMode },
    RuleAdded { rule: NetRule },
    RuleRemoved { rule: NetRule },
    RulesReordered,
    BindingAdded { env_var: String, hosts: Vec<String>, secret_id: String },
    BindingRemoved { env_var: String },
    BindingHostAdded { env_var: String, host: String },
    BindingHostRemoved { env_var: String, host: String },
    BindingSecretIdChanged { env_var: String, secret_id: String, owner: Option<String> },
    BindingPositionWidened { env_var: String },
    BindingPositionNarrowed { env_var: String },
    SettingChanged { setting: String },
}

fn rule_text(rule: &NetRule) -> String {
    let action = match rule.action {
        RuleAction::Allow => "allow",
        RuleAction::Deny => "deny",
    };
    let target = match (&rule.host, &rule.cidr, &rule.group) {
        (Some(h), _, _) => h.clone(),
        (_, Some(c), _) => c.clone(),
        (_, _, Some(g)) => format!("group {g:?}").to_ascii_lowercase(),
        _ => "(no target)".to_string(),
    };
    format!("{action} {target}")
}

fn mode_name(mode: EgressMode) -> &'static str {
    match mode {
        EgressMode::Open => "open",
        EgressMode::Allowlist => "allowlist",
    }
}

fn change(detail: ChangeDetail, text: String, widens: bool) -> PolicyChange {
    PolicyChange { detail, text, widens, risk: None }
}

/// What a binding exposes, named by the secret it resolves to rather than by its own variable: a
/// proposal can bind any stored id under any variable name.
fn exposure(accepted: &NetPolicy, new: &SecretBinding) -> String {
    let same_id = accepted.secrets.iter().any(|b| b.env_var == new.env_var && b.secret_id == new.secret_id);
    if same_id {
        return format!("the real value of ${}", new.env_var);
    }
    match accepted.secrets.iter().find(|b| b.secret_id == new.secret_id) {
        Some(owner) => format!("the real value stored for ${}", owner.env_var),
        None => format!("the real value stored under secret id {:?}, if one exists", new.secret_id),
    }
}

fn with_risk(mut change: PolicyChange, hosts: &[String], exposure: &str) -> PolicyChange {
    if !hosts.is_empty() {
        change.risk = Some(format!("Requests to {} will receive {exposure}", hosts.join(", ")));
    }
    change
}

fn diff_rules(accepted: &NetPolicy, proposed: &NetPolicy, out: &mut Vec<PolicyChange>) {
    let mut remaining = accepted.rules.clone();
    let mut added = Vec::new();
    for rule in &proposed.rules {
        match remaining.iter().position(|r| r == rule) {
            Some(i) => {
                remaining.remove(i);
            }
            None => added.push(rule.clone()),
        }
    }
    let nothing_added = added.is_empty();
    for rule in added {
        let widens = rule.action == RuleAction::Allow;
        out.push(change(ChangeDetail::RuleAdded { rule: rule.clone() }, format!("Rule added: {}", rule_text(&rule)), widens));
    }
    let nothing_removed = remaining.is_empty();
    for rule in remaining {
        // Dropping a deny rule reopens what it blocked.
        let widens = rule.action == RuleAction::Deny;
        out.push(change(ChangeDetail::RuleRemoved { rule: rule.clone() }, format!("Rule removed: {}", rule_text(&rule)), widens));
    }
    if nothing_added && nothing_removed && accepted.rules != proposed.rules {
        out.push(change(ChangeDetail::RulesReordered, "Rules reordered: the first matching rule wins".to_string(), true));
    }
}

fn diff_binding(accepted: &NetPolicy, old: &SecretBinding, new: &SecretBinding, out: &mut Vec<PolicyChange>) {
    let var = &new.env_var;
    let exposes = exposure(accepted, new);
    for host in new.hosts.iter().filter(|h| !old.hosts.contains(h)) {
        out.push(with_risk(
            change(
                ChangeDetail::BindingHostAdded { env_var: var.clone(), host: host.clone() },
                format!("${var} may now be sent to {host}"),
                true,
            ),
            std::slice::from_ref(host),
            &exposes,
        ));
    }
    for host in old.hosts.iter().filter(|h| !new.hosts.contains(h)) {
        out.push(change(
            ChangeDetail::BindingHostRemoved { env_var: var.clone(), host: host.clone() },
            format!("${var} no longer reaches {host}"),
            false,
        ));
    }
    if old.secret_id != new.secret_id {
        let owner = accepted.secrets.iter().find(|b| b.secret_id == new.secret_id).map(|b| b.env_var.clone());
        let whose = match &owner {
            Some(o) => format!("the stored secret of ${o}"),
            None => "a stored secret no accepted variable uses".to_string(),
        };
        out.push(with_risk(
            change(
                ChangeDetail::BindingSecretIdChanged {
                    env_var: var.clone(),
                    secret_id: new.secret_id.clone(),
                    owner,
                },
                format!("${var} now resolves to secret id {:?}, {whose}", new.secret_id),
                true,
            ),
            &new.hosts,
            &exposes,
        ));
    }
    let widened = new.headers.iter().any(|h| !old.headers.contains(h))
        || (new.query && !old.query)
        || (new.body && !old.body);
    let narrowed = old.headers.iter().any(|h| !new.headers.contains(h))
        || (old.query && !new.query)
        || (old.body && !new.body);
    if widened {
        out.push(with_risk(
            change(
                ChangeDetail::BindingPositionWidened { env_var: var.clone() },
                format!("${var} is now also swapped in more places (headers, query or body)"),
                true,
            ),
            &new.hosts,
            &exposes,
        ));
    }
    if narrowed {
        out.push(change(
            ChangeDetail::BindingPositionNarrowed { env_var: var.clone() },
            format!("${var} is swapped in fewer places (headers, query or body)"),
            false,
        ));
    }
}

/// The changes that turn `accepted` into `proposed`, widening ones first.
pub fn diff(accepted: &NetPolicy, proposed: &NetPolicy) -> Vec<PolicyChange> {
    let mut out = Vec::new();
    if accepted.egress != proposed.egress {
        out.push(change(
            ChangeDetail::ModeChanged { from: accepted.egress, to: proposed.egress },
            format!("Mode changed from {} to {}", mode_name(accepted.egress), mode_name(proposed.egress)),
            proposed.egress == EgressMode::Open,
        ));
    }
    diff_rules(accepted, proposed, &mut out);
    for new in &proposed.secrets {
        match accepted.secrets.iter().find(|b| b.env_var == new.env_var) {
            None => out.push(with_risk(
                change(
                    ChangeDetail::BindingAdded {
                        env_var: new.env_var.clone(),
                        hosts: new.hosts.clone(),
                        secret_id: new.secret_id.clone(),
                    },
                    format!(
                        "Secret binding added: ${} (stored secret {:?}) for {}",
                        new.env_var,
                        new.secret_id,
                        new.hosts.join(", ")
                    ),
                    true,
                ),
                &new.hosts,
                &exposure(accepted, new),
            )),
            Some(old) => diff_binding(accepted, old, new, &mut out),
        }
    }
    for old in accepted.secrets.iter().filter(|b| !proposed.secrets.iter().any(|n| n.env_var == b.env_var)) {
        out.push(change(
            ChangeDetail::BindingRemoved { env_var: old.env_var.clone() },
            format!("Secret binding removed: ${}", old.env_var),
            false,
        ));
    }
    let mut setting = |name: &str, differs: bool, widens: bool| {
        if differs {
            out.push(change(
                ChangeDetail::SettingChanged { setting: name.to_string() },
                format!("Setting changed: {name}"),
                widens,
            ));
        }
    };
    setting("strictHostnames", accepted.strict_hostnames != proposed.strict_hostnames, accepted.strict_hostnames);
    setting("violationAction", accepted.violation_action != proposed.violation_action, false);
    setting("denyMessage", accepted.deny_message != proposed.deny_message, false);
    setting("version", accepted.version != proposed.version, false);
    setting("interactive", accepted.interactive != proposed.interactive, false);
    out.sort_by_key(|c| !c.widens);
    out
}

/// The accepted policy of one sandbox: the only policy any sidecar render enforces, kept in the app
/// data root where code inside the sandbox cannot write it
/// (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 1).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPolicy {
    pub policy: NetPolicy,
    /// Counts policy changes; an apply that started from an older revision is refused
    /// (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 10).
    #[serde(default)]
    pub revision: u64,
    /// The binding variables a Podman sandbox received placeholders for at create; kept across
    /// applies, since placeholders exist only from create
    /// (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guest_env: Option<Vec<String>>,
}

/// The binding variables of `policy` that are safe to stamp into a guest as placeholders: valid
/// names only, so an imported file cannot inject an arbitrary variable.
pub fn stampable_vars(policy: &NetPolicy) -> Vec<String> {
    let mut vars: Vec<String> = Vec::new();
    for b in &policy.secrets {
        let ok = valid_env_var(&b.env_var) && !RESERVED_ENV.contains(&b.env_var.as_str());
        if ok && !vars.contains(&b.env_var) {
            vars.push(b.env_var.clone());
        }
    }
    vars
}

/// A binding whose variable the sandbox never received, with the value to export by hand.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnstampedBinding {
    pub env_var: String,
    pub placeholder: String,
}

/// The bindings of `policy` that `applied` did not stamp into the guest at create. Empty for a
/// sandbox that records no create-time variables (one predating them).
pub fn unstamped_bindings(policy: &NetPolicy, applied: &AppliedPolicy) -> Vec<UnstampedBinding> {
    let Some(stamped) = &applied.guest_env else {
        return Vec::new();
    };
    policy
        .secrets
        .iter()
        .filter(|b| !stamped.contains(&b.env_var))
        .map(|b| UnstampedBinding {
            env_var: b.env_var.clone(),
            placeholder: placeholder(&b.env_var),
        })
        .collect()
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

/// The applied snapshot, or `None` for a sandbox created without one (sidecars, sandboxes
/// predating this feature).
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

/// The accepted policy of `sandbox`. One with no snapshot (predating this feature) is an empty
/// allowlist, so its workspace file shows as a proposal
/// (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 9).
pub fn read_accepted<R: Runtime>(app: &AppHandle<R>, sandbox: &str) -> Result<AppliedPolicy, NetPolicyError> {
    Ok(read_applied(app, sandbox)?.unwrap_or_else(|| AppliedPolicy {
        policy: empty_accepted_policy(),
        revision: 0,
        guest_env: None,
    }))
}

/// The accepted policy of a sandbox with no snapshot: an allowlist with no rules.
pub fn empty_accepted_policy() -> NetPolicy {
    NetPolicy { egress: EgressMode::Allowlist, ..NetPolicy::default() }
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

    /// A provider that enforces the policy with its own network stack; none ships today.
    const ENGINE: ProviderCapabilities = ProviderCapabilities {
        remote_targets: false,
        event_stream: false,
        host_bind_mount: false,
        egress_policy: true,
        sidecar_egress: false,
    };

    const PODMAN: ProviderCapabilities =
        ProviderCapabilities { egress_policy: false, sidecar_egress: true, ..ENGINE };

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
    fn rejects_bad_targets_and_ports() {
        let mut p = starter(ENGINE, &[]);
        p.rules[0].host = Some("*.com".to_string());
        assert!(validate(&p, ENGINE).is_err());
        let mut p = starter(ENGINE, &[]);
        p.rules[0].cidr = Some("10.0.0.0/8".to_string());
        assert!(validate(&p, ENGINE).is_err(), "two targets on one rule");
        let mut p = starter(ENGINE, &[]);
        p.rules[0].ports = Some(vec![PortSpec::Range("90-80".to_string())]);
        assert!(validate(&p, ENGINE).is_err());
        let mut p = starter(ENGINE, &[]);
        p.rules[0].host = None;
        p.rules[0].cidr = Some("10.0.0.0/33".to_string());
        assert!(validate(&p, ENGINE).is_err());
    }

    #[test]
    fn rejects_bad_secret_bindings() {
        let base = starter(ENGINE, &[]);
        let with = |b: SecretBinding| NetPolicy { secrets: vec![b], ..base.clone() };
        validate(&with(binding("api.github.com")), ENGINE).unwrap();
        assert!(validate(&with(binding("example.com")), ENGINE).is_err(), "not allowed by a rule");
        assert!(validate(&with(binding("*.github.com")), ENGINE).is_err(), "wildcard secret host");
        for var in ["PATH", "HOME", "lower", "1ABC"] {
            let b = SecretBinding { env_var: var.to_string(), ..binding("github.com") };
            assert!(validate(&with(b), ENGINE).is_err(), "{var} must be refused");
        }
        let twice = NetPolicy {
            secrets: vec![binding("github.com"), binding("api.github.com")],
            ..base.clone()
        };
        assert!(validate(&twice, ENGINE).is_err());
        let open = NetPolicy { secrets: vec![binding("example.com")], ..NetPolicy::default() };
        validate(&open, ENGINE).unwrap();
    }

    #[test]
    fn interactive_is_refused_for_every_provider() {
        let p = NetPolicy { interactive: Some(true), ..NetPolicy::default() };
        assert!(validate(&p, ENGINE).is_err());
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
            validate(&p, ENGINE).unwrap_or_else(|e| panic!("{what} is valid for an engine-enforced provider: {e}"));
        }
        for action in [ViolationAction::Block, ViolationAction::BlockAndTerminate] {
            let p = NetPolicy { violation_action: action, ..NetPolicy::default() };
            assert!(validate(&p, PODMAN).is_err(), "{action:?} must be refused for Podman");
        }
        let p = NetPolicy { deny_message: Some("blocked by IT".to_string()), ..NetPolicy::default() };
        assert!(validate(&p, PODMAN).is_err(), "a deny message must be refused for Podman");
        validate(&p, ENGINE).unwrap();
        let p = NetPolicy { strict_hostnames: false, ..NetPolicy::default() };
        validate(&p, PODMAN).unwrap();
    }

    #[test]
    fn a_wildcard_covers_its_apex_only_for_the_sidecar() {
        let p = NetPolicy {
            egress: EgressMode::Allowlist,
            rules: vec![NetRule { host: Some("*.example.com".to_string()), ..rule(RuleAction::Allow) }],
            secrets: vec![binding("example.com")],
            ..NetPolicy::default()
        };
        validate(&p, PODMAN).unwrap();
        assert!(validate(&p, ENGINE).is_err(), "an engine-enforced suffix rule does not cover the apex");
    }

    fn widening(accepted: &NetPolicy, proposed: &NetPolicy) -> Vec<bool> {
        diff(accepted, proposed).iter().map(|c| c.widens).collect()
    }

    #[test]
    fn identical_policies_have_no_diff() {
        let p = NetPolicy { secrets: vec![binding("github.com")], ..starter(PODMAN, &[]) };
        assert!(diff(&p, &p.clone()).is_empty());
    }

    #[test]
    fn a_binding_host_added_in_open_mode_widens() {
        let old = NetPolicy { secrets: vec![binding("api.example.com")], ..NetPolicy::default() };
        let mut new = old.clone();
        new.secrets[0].hosts.push("evil.example.net".to_string());
        let changes = diff(&old, &new);
        assert_eq!(changes.len(), 1);
        assert!(changes[0].widens);
        assert!(matches!(&changes[0].detail, ChangeDetail::BindingHostAdded { host, .. } if host == "evil.example.net"));
        assert_eq!(widening(&new, &old), [false], "removing the host narrows");
    }

    #[test]
    fn a_secret_id_change_widens() {
        let old = NetPolicy { secrets: vec![binding("api.example.com")], ..NetPolicy::default() };
        let mut new = old.clone();
        new.secrets[0].secret_id = "other-sandbox-key".to_string();
        assert_eq!(widening(&old, &new), [true]);
    }

    fn bound(var: &str, id: &str, host: &str) -> SecretBinding {
        SecretBinding { secret_id: id.to_string(), env_var: var.to_string(), ..binding(host) }
    }

    #[test]
    fn a_proposal_names_the_secret_it_actually_exposes() {
        let old = NetPolicy { secrets: vec![bound("NPR_KEY", "sb-npr_key", "api.example.com")], ..NetPolicy::default() };
        let mut new = old.clone();
        new.secrets.push(bound("EXFIL", "sb-npr_key", "evil.net"));
        let changes = diff(&old, &new);
        assert_eq!(changes.len(), 1);
        let risk = changes[0].risk.as_deref().unwrap();
        assert_eq!(risk, "Requests to evil.net will receive the real value stored for $NPR_KEY");

        let mut host_added = old.clone();
        host_added.secrets[0].hosts.push("evil.net".to_string());
        let risk = diff(&old, &host_added)[0].risk.clone().unwrap();
        assert_eq!(risk, "Requests to evil.net will receive the real value of $NPR_KEY");

        let mut unknown = old.clone();
        unknown.secrets.push(bound("OTHER", "sb-other", "evil.net"));
        let risk = diff(&old, &unknown)[0].risk.clone().unwrap();
        assert!(risk.contains("secret id \"sb-other\""), "{risk}");
    }

    #[test]
    fn a_secret_id_change_names_the_new_id_and_its_owner() {
        let mut old = NetPolicy {
            secrets: vec![bound("A_KEY", "sb-a_key", "a.example.com"), bound("B_KEY", "sb-b_key", "b.example.com")],
            ..NetPolicy::default()
        };
        let mut new = old.clone();
        new.secrets[0].secret_id = "sb-b_key".to_string();
        let changes = diff(&old, &new);
        assert_eq!(changes.len(), 1);
        assert!(changes[0].widens);
        assert!(matches!(
            &changes[0].detail,
            ChangeDetail::BindingSecretIdChanged { secret_id, owner, .. }
                if secret_id == "sb-b_key" && owner.as_deref() == Some("B_KEY")
        ));
        assert!(changes[0].text.contains("sb-b_key") && changes[0].text.contains("$B_KEY"), "{}", changes[0].text);
        assert!(changes[0].risk.as_deref().unwrap().contains("stored for $B_KEY"));
        old.secrets[0].secret_id = "sb-b_key".to_string();
        new.secrets[0].secret_id = "sb-fresh".to_string();
        let changes = diff(&old, &new);
        assert!(matches!(&changes[0].detail, ChangeDetail::BindingSecretIdChanged { owner: None, .. }));
    }

    #[test]
    fn loader_and_runtime_variables_are_reserved() {
        let base = NetPolicy::default();
        for var in ["LD_PRELOAD", "NODE_OPTIONS", "PYTHONPATH", "BASH_ENV", "HTTPS_PROXY", "SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS", "SHELL", "USER"] {
            let b = SecretBinding { env_var: var.to_string(), ..binding("example.com") };
            let p = NetPolicy { secrets: vec![b.clone()], ..base.clone() };
            let err = validate(&p, PODMAN).unwrap_err();
            assert!(err.0.contains("reserved"), "{var}: {}", err.0);
            assert!(stampable_vars(&p).is_empty(), "{var} must not be stamped");
        }
    }

    #[test]
    fn rule_widening_depends_on_the_action() {
        let old = NetPolicy::default();
        let allow = NetRule { host: Some("example.com".to_string()), ..rule(RuleAction::Allow) };
        let deny = NetRule { cidr: Some("10.0.0.0/8".to_string()), ..rule(RuleAction::Deny) };
        let with = |r: &NetRule| NetPolicy { rules: vec![r.clone()], ..NetPolicy::default() };
        assert_eq!(widening(&old, &with(&allow)), [true]);
        assert_eq!(widening(&old, &with(&deny)), [false]);
        assert_eq!(widening(&with(&deny), &old), [true], "dropping a deny reopens it");
    }

    #[test]
    fn widening_changes_sort_first_and_reorders_widen() {
        let a = NetRule { host: Some("a.example.com".to_string()), ..rule(RuleAction::Allow) };
        let b = NetRule { cidr: Some("10.0.0.0/8".to_string()), ..rule(RuleAction::Deny) };
        let old = NetPolicy { rules: vec![a.clone(), b.clone()], ..NetPolicy::default() };
        let new = NetPolicy { rules: vec![b, a], ..NetPolicy::default() };
        let changes = diff(&old, &new);
        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0].detail, ChangeDetail::RulesReordered) && changes[0].widens);
        let mut mixed = NetPolicy { egress: EgressMode::Allowlist, ..NetPolicy::default() };
        mixed.secrets.push(binding("x.example.com"));
        let widths = widening(&old, &mixed);
        assert!(widths.windows(2).all(|w| w[0] >= w[1]), "widening first: {widths:?}");
    }

    #[test]
    fn stampable_vars_skip_invalid_names() {
        let mut p = NetPolicy::default();
        for var in ["GOOD_ONE", "PATH", "bad name", "GOOD_ONE"] {
            p.secrets.push(SecretBinding { env_var: var.to_string(), ..binding("example.com") });
        }
        assert_eq!(stampable_vars(&p), ["GOOD_ONE"]);
    }

    #[test]
    fn converts_to_the_provider_shape() {
        let p = NetPolicy { secrets: vec![binding("api.github.com")], ..starter(ENGINE, &[]) };
        let e = to_egress(&p).unwrap();
        assert_eq!(e.mode, EgressMode::Allowlist);
        assert_eq!(e.rules.len(), p.rules.len());
        assert_eq!(e.secrets[0].env_var, "GH_TOKEN");
        assert_eq!(e.rules[0].ports, [(443, 443)]);
    }
}
