//! Refuses a policy the provider cannot enforce exactly as written: host, CIDR and port syntax, the
//! sidecar dialect, and the secret-binding rules.

use super::schema::NetPolicy;
use super::NetPolicyError;
use crate::provider::types::{
    EgressMode, EgressTarget, NetProtocol, ProviderCapabilities, RuleAction, ViolationAction,
};

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

pub(super) fn valid_env_var(name: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::super::schema::{starter, NetRule, PortSpec, SecretBinding};
    use super::super::test_support::{binding, rule, ENGINE, PODMAN};
    use super::*;
    use crate::provider::types::NetGroup;

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
    fn stampable_vars_skip_invalid_names() {
        let mut p = NetPolicy::default();
        for var in ["GOOD_ONE", "PATH", "bad name", "GOOD_ONE"] {
            p.secrets.push(SecretBinding { env_var: var.to_string(), ..binding("example.com") });
        }
        assert_eq!(stampable_vars(&p), ["GOOD_ONE"]);
    }
}
