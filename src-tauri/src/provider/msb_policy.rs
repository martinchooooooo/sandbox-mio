//! Renders an [`EgressPolicy`] into `msb create` flags. Every flag here was exercised against real
//! `msb` 0.7.12 (`docs/features/microsandbox-network-policy/spike/`); the argv carries variable
//! names only, never a secret value.

use super::types::{
    EgressMode, EgressPolicy, EgressRule, EgressTarget, NetGroup, NetProtocol, RuleAction,
    SecretBindingSpec, ViolationAction,
};

/// The variable the agent `msb` child reads the chained proxy's SOCKS5 password from.
pub const SOCKS_PASSWORD_ENV: &str = "MIO_SOCKS_PASSWORD";

/// The SOCKS5 username the sidecar's htpasswd and the agent's `--socks5-username` share.
pub const SOCKS_USERNAME: &str = "mio";

fn action(a: RuleAction) -> &'static str {
    match a {
        RuleAction::Allow => "allow",
        RuleAction::Deny => "deny",
    }
}

fn group(g: NetGroup) -> &'static str {
    match g {
        NetGroup::Public => "public",
        NetGroup::Private => "private",
        NetGroup::Host => "host",
    }
}

fn protocol(p: NetProtocol) -> &'static str {
    match p {
        NetProtocol::Tcp => "tcp",
        NetProtocol::Udp => "udp",
    }
}

/// One `--net-rule` token per protocol/port-range pair: the token grammar
/// (`<action>@<target>[:<proto>[:<ports>]]`) admits one port range, and ports need a protocol.
fn rule_tokens(rule: &EgressRule) -> Vec<String> {
    let target = match &rule.target {
        EgressTarget::Host(h) => h.clone(),
        EgressTarget::Cidr(c) => c.clone(),
        EgressTarget::Group(g) => group(*g).to_string(),
    };
    let head = format!("{}@{target}", action(rule.action));
    let protocols: Vec<NetProtocol> = match (rule.protocol, rule.ports.is_empty()) {
        (Some(p), _) => vec![p],
        (None, true) => return vec![head],
        (None, false) => vec![NetProtocol::Tcp, NetProtocol::Udp],
    };
    let mut tokens = Vec::new();
    for p in protocols {
        if rule.ports.is_empty() {
            tokens.push(format!("{head}:{}", protocol(p)));
        }
        for (start, end) in &rule.ports {
            let ports = if start == end { start.to_string() } else { format!("{start}-{end}") };
            tokens.push(format!("{head}:{}:{ports}", protocol(p)));
        }
    }
    tokens
}

fn secret_token(binding: &SecretBindingSpec) -> String {
    let mut options = Vec::new();
    if binding.headers.is_empty() {
        options.push("no-headers".to_string());
    } else {
        options.push(format!("headers=[{}]", binding.headers.join(",")));
    }
    if binding.query {
        options.push("query".to_string());
    }
    if binding.body {
        options.push("body".to_string());
    }
    format!("{}:{}@{}", binding.env_var, options.join(","), binding.hosts.join(","))
}

/// The `--script-raw` body launching `command` as the sidecar's init: `env` exported, the command
/// `exec`ed so it replaces the shell, its console captured in the flows mount. Not `--script`,
/// which decodes backslash escapes and breaks single-quoted arguments
/// (`docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 9).
pub fn script_body(env: &[(String, String)], command: &[String]) -> String {
    let mut body = String::from("#!/bin/sh\n");
    for (k, v) in env {
        body.push_str(&format!("export {k}={}\n", super::msb::shell_quote(v)));
    }
    let argv: Vec<String> = command.iter().map(|a| super::msb::shell_quote(a)).collect();
    body.push_str(&format!("exec {} >/flows/mitmdump.log 2>&1\n", argv.join(" ")));
    body
}

/// The `msb create` flags for `policy`. An open policy with nothing else set renders nothing,
/// which is microsandbox's own `allow@public` default
/// (`docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 3).
pub fn render(policy: &EgressPolicy) -> Vec<String> {
    let mut argv = Vec::new();
    let has_rules = !policy.rules.is_empty();
    let allowlist = policy.mode == EgressMode::Allowlist;
    if allowlist || has_rules {
        argv.push("--net-default-egress".to_string());
        argv.push("deny".to_string());
    }
    for rule in &policy.rules {
        for token in rule_tokens(rule) {
            argv.push("--net-rule".to_string());
            argv.push(token);
        }
    }
    if !allowlist && has_rules {
        // Reproduces microsandbox's implicit default after the user's rules (Resolved Decision 15).
        argv.push("--net-rule".to_string());
        argv.push("allow@public".to_string());
    }
    // Never `allow@dns`: it opens the resolver to every name, while domain-allow rules already
    // resolve exactly the allowed names (spike/01_egress-baseline.md, "0.7.12 Differences").
    let intercept =
        allowlist || has_rules || !policy.secrets.is_empty() || policy.proxy.is_some();
    if intercept {
        argv.push("--tls-intercept".to_string());
        if !policy.strict_hostnames {
            argv.push("--net-strict=false".to_string());
        }
    }
    for binding in &policy.secrets {
        argv.push("--secret".to_string());
        argv.push(secret_token(binding));
    }
    if !policy.secrets.is_empty() && policy.violation_action != ViolationAction::BlockAndLog {
        argv.push("--secret-violation-action".to_string());
        argv.push(
            match policy.violation_action {
                ViolationAction::Block => "block",
                ViolationAction::BlockAndLog => "block-and-log",
                ViolationAction::BlockAndTerminate => "block-and-terminate",
            }
            .to_string(),
        );
    }
    if let Some(proxy) = &policy.proxy {
        argv.push("--proxy".to_string());
        argv.push(format!("socks5://127.0.0.1:{}", proxy.host_port));
        argv.push("--socks5-username".to_string());
        argv.push(proxy.username.clone());
        argv.push("--socks5-password-env".to_string());
        argv.push(proxy.password_env.clone());
        argv.push("--tls-upstream-ca-cert".to_string());
        argv.push(proxy.upstream_ca_path.clone());
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::types::ChainedProxy;

    fn open() -> EgressPolicy {
        EgressPolicy {
            mode: EgressMode::Open,
            rules: Vec::new(),
            strict_hostnames: true,
            secrets: Vec::new(),
            violation_action: ViolationAction::BlockAndLog,
            deny_message: None,
            proxy: None,
        }
    }

    fn host_rule(action: RuleAction, host: &str) -> EgressRule {
        EgressRule {
            action,
            target: EgressTarget::Host(host.to_string()),
            protocol: Some(NetProtocol::Tcp),
            ports: vec![(443, 443)],
        }
    }

    fn binding() -> SecretBindingSpec {
        SecretBindingSpec {
            env_var: "GH_TOKEN".to_string(),
            hosts: vec!["api.github.com".to_string(), "github.com".to_string()],
            headers: vec!["authorization".to_string()],
            query: false,
            body: false,
        }
    }

    #[test]
    fn script_body_quotes_every_argument_and_exports_env() {
        let body = script_body(
            &[("MIO_FLOWS_PATH".to_string(), "/flows/flows.ndjson".to_string())],
            &["mitmdump".to_string(), "it's".to_string()],
        );
        assert_eq!(
            body,
            "#!/bin/sh\nexport MIO_FLOWS_PATH='/flows/flows.ndjson'\nexec 'mitmdump' 'it'\\''s' >/flows/mitmdump.log 2>&1\n"
        );
    }

    #[test]
    fn plain_open_policy_renders_nothing() {
        assert!(render(&open()).is_empty());
    }

    #[test]
    fn allowlist_denies_by_default_and_intercepts() {
        let policy = EgressPolicy {
            mode: EgressMode::Allowlist,
            rules: vec![host_rule(RuleAction::Allow, "registry.npmjs.org")],
            ..open()
        };
        assert_eq!(
            render(&policy),
            [
                "--net-default-egress",
                "deny",
                "--net-rule",
                "allow@registry.npmjs.org:tcp:443",
                "--tls-intercept",
            ]
        );
    }

    #[test]
    fn open_policy_with_rules_ends_on_allow_public() {
        let policy = EgressPolicy { rules: vec![host_rule(RuleAction::Deny, "example.com")], ..open() };
        assert_eq!(
            render(&policy),
            [
                "--net-default-egress",
                "deny",
                "--net-rule",
                "deny@example.com:tcp:443",
                "--net-rule",
                "allow@public",
                "--tls-intercept",
            ]
        );
    }

    #[test]
    fn never_renders_allow_dns() {
        let policy = EgressPolicy {
            mode: EgressMode::Allowlist,
            rules: vec![host_rule(RuleAction::Allow, "github.com")],
            ..open()
        };
        assert!(render(&policy).iter().all(|t| !t.contains("dns")));
    }

    #[test]
    fn rule_targets_protocols_and_port_ranges() {
        let rules = [
            EgressRule {
                action: RuleAction::Allow,
                target: EgressTarget::Cidr("10.0.0.0/8".to_string()),
                protocol: None,
                ports: Vec::new(),
            },
            EgressRule {
                action: RuleAction::Allow,
                target: EgressTarget::Group(NetGroup::Private),
                protocol: Some(NetProtocol::Udp),
                ports: Vec::new(),
            },
            EgressRule {
                action: RuleAction::Allow,
                target: EgressTarget::Host("*.githubusercontent.com".to_string()),
                protocol: None,
                ports: vec![(8000, 8100)],
            },
        ];
        let tokens: Vec<String> = rules.iter().flat_map(rule_tokens).collect();
        assert_eq!(
            tokens,
            [
                "allow@10.0.0.0/8",
                "allow@private:udp",
                "allow@*.githubusercontent.com:tcp:8000-8100",
                "allow@*.githubusercontent.com:udp:8000-8100",
            ]
        );
    }

    #[test]
    fn strict_off_is_rendered_only_when_disabled() {
        let policy = EgressPolicy { mode: EgressMode::Allowlist, strict_hostnames: false, ..open() };
        assert!(render(&policy).contains(&"--net-strict=false".to_string()));
        let policy = EgressPolicy { mode: EgressMode::Allowlist, ..open() };
        assert!(!render(&policy).iter().any(|t| t.starts_with("--net-strict")));
    }

    #[test]
    fn secret_bindings_render_names_and_options() {
        let mut full = binding();
        full.headers = vec!["authorization".to_string(), "x-api-key".to_string()];
        full.query = true;
        full.body = true;
        let none = SecretBindingSpec { headers: Vec::new(), ..binding() };
        assert_eq!(secret_token(&binding()), "GH_TOKEN:headers=[authorization]@api.github.com,github.com");
        assert_eq!(
            secret_token(&full),
            "GH_TOKEN:headers=[authorization,x-api-key],query,body@api.github.com,github.com"
        );
        assert_eq!(secret_token(&none), "GH_TOKEN:no-headers@api.github.com,github.com");
    }

    #[test]
    fn secrets_turn_on_interception_and_non_default_violation_action() {
        let policy = EgressPolicy {
            secrets: vec![binding()],
            violation_action: ViolationAction::BlockAndTerminate,
            ..open()
        };
        let argv = render(&policy);
        assert!(argv.contains(&"--tls-intercept".to_string()));
        assert!(!argv.contains(&"--net-default-egress".to_string()));
        assert!(argv.windows(2).any(|w| w == ["--secret-violation-action", "block-and-terminate"]));

        let policy = EgressPolicy { secrets: vec![binding()], ..open() };
        assert!(!render(&policy).contains(&"--secret-violation-action".to_string()));
    }

    #[test]
    fn chained_proxy_renders_socks_flags() {
        let policy = EgressPolicy {
            proxy: Some(ChainedProxy {
                host_port: 50123,
                username: SOCKS_USERNAME.to_string(),
                password_env: SOCKS_PASSWORD_ENV.to_string(),
                upstream_ca_path: "C:/data/proxy/s/certs/mitmproxy-ca-cert.pem".to_string(),
            }),
            ..open()
        };
        assert_eq!(
            render(&policy),
            [
                "--tls-intercept",
                "--proxy",
                "socks5://127.0.0.1:50123",
                "--socks5-username",
                "mio",
                "--socks5-password-env",
                "MIO_SOCKS_PASSWORD",
                "--tls-upstream-ca-cert",
                "C:/data/proxy/s/certs/mitmproxy-ca-cert.pem",
            ]
        );
    }

    /// The spec carries no value at all, so the only way one could leak is a future field; this
    /// pins that no rendered token resembles a `NAME=value` assignment.
    #[test]
    fn rendered_argv_never_assigns_a_value() {
        let policy = EgressPolicy {
            mode: EgressMode::Allowlist,
            rules: vec![host_rule(RuleAction::Allow, "api.github.com")],
            secrets: vec![binding()],
            ..open()
        };
        assert!(render(&policy).iter().all(|t| !t.contains('=') || t.contains("headers=[")));
    }
}
