//! Renders a validated `.mio/net-policy.json` into an iron-proxy config
//! (`docs/features/podman-egress-boundary/02_dev.md`, Phase C). Pure: the caller writes the value
//! as `config.yaml`, relying on YAML 1.2 being a superset of JSON (Resolved Decision 12).
//!
//! iron-proxy silently ignores an unknown key inside a transform's `config`, so a misspelled key
//! fails open; the tests pin every rendered key against a config captured from a real run.

use serde_json::{json, Value};

use crate::net_policy::{self, EgressMode, NetPolicy, RuleAction};

/// Replaces iron-proxy's default deny list in every config, the bootstrap included, so an allowed
/// hostname resolving to a private address cannot reach the machine, the host or the LAN
/// (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 17). The last entry is
/// iron-proxy's own IPv6 metadata address; the IPv4 one sits inside `169.254.0.0/16`.
pub const BASELINE_DENY_CIDRS: &[&str] = &[
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "::1/128",
    "fc00::/7",
    "fe80::/10",
    "fd00:ec2::254/128",
];

/// Where the sidecar sees the CA directory and the per-binding secret files.
pub const CONTAINER_CERTS_DIR: &str = "/etc/iron-proxy/certs";
pub const CONTAINER_SECRETS_DIR: &str = "/etc/iron-proxy/secrets";

/// The sidecar environment variable holding the management bearer token; iron-proxy refuses to
/// boot a management listener without it (Resolved Decision 13).
pub const MGMT_API_KEY_ENV: &str = "IRON_MANAGEMENT_API_KEY";

/// iron-proxy cannot disable metrics, so the listener is pinned to the sidecar's loopback.
const METRICS_PORT: u16 = 9090;

/// The sidecar-side addresses a rendered config binds or announces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IronListen {
    /// The sidecar's pinned address on the internal network, which iron-proxy's DNS server answers
    /// for every name.
    pub proxy_ip: String,
    /// The tunnel listener (`http_proxy`/`https_proxy`, CONNECT, SOCKS5).
    pub tunnel_port: u16,
    /// The management listener, bound to the sidecar's loopback.
    pub mgmt_port: u16,
}

/// A binding whose value was materialised for the sidecar; bindings without one are not rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFile {
    pub env_var: String,
    /// The file's path inside the sidecar.
    pub path: String,
}

impl SecretFile {
    /// The file for `env_var` under [`CONTAINER_SECRETS_DIR`]; the variable name is validated as
    /// `[A-Z0-9_]`, so it is a safe file name (Resolved Decision 15).
    pub fn new(env_var: &str) -> Self {
        SecretFile {
            env_var: env_var.to_string(),
            path: format!("{CONTAINER_SECRETS_DIR}/{env_var}"),
        }
    }
}

/// The config the sidecar runs with before the first apply: an empty allowlist outside warn mode,
/// so it denies everything (Resolved Decision 18).
pub fn bootstrap(listen: &IronListen) -> Value {
    config(
        listen,
        BASELINE_DENY_CIDRS.iter().map(|c| c.to_string()).collect(),
        vec![allowlist(false, Vec::new())],
    )
}

/// The config enforcing `policy`, which [`net_policy::validate`] has accepted in the sidecar
/// dialect. `secret_files` lists the bindings whose value exists; any other binding is left out,
/// so its placeholder passes through unswapped (Resolved Decision 15).
pub fn render(policy: &NetPolicy, listen: &IronListen, secret_files: &[SecretFile]) -> Value {
    let mut domains: Vec<String> = Vec::new();
    let mut deny = BASELINE_DENY_CIDRS.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    for rule in &policy.rules {
        match (rule.action, &rule.host, &rule.cidr) {
            (RuleAction::Allow, Some(host), _) => {
                push_unique(&mut domains, host.to_ascii_lowercase())
            }
            (RuleAction::Deny, _, Some(cidr)) => push_unique(&mut deny, cidr.clone()),
            _ => {}
        }
    }
    let mut transforms = vec![allowlist(policy.egress == EgressMode::Open, domains)];
    let secrets: Vec<Value> = policy
        .secrets
        .iter()
        .filter_map(|b| {
            let file = secret_files.iter().find(|f| f.env_var == b.env_var)?;
            let rules: Vec<Value> =
                b.hosts.iter().map(|h| json!({ "host": h.to_ascii_lowercase() })).collect();
            Some(json!({
                "source": { "type": "file", "path": file.path },
                "replace": {
                    "proxy_value": net_policy::placeholder(&b.env_var),
                    "match_headers": b.headers.iter().map(|h| canonical_header(h)).collect::<Vec<_>>(),
                    "match_query": b.query,
                    "match_body": b.body,
                },
                "rules": rules,
            }))
        })
        .collect();
    // Omitted rather than rendered empty: an empty `secrets` transform was never run in Phase A.
    if !secrets.is_empty() {
        transforms.push(json!({ "name": "secrets", "config": { "secrets": secrets } }));
    }
    config(listen, deny, transforms)
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

fn allowlist(warn: bool, domains: Vec<String>) -> Value {
    json!({ "name": "allowlist", "config": { "warn": warn, "domains": domains } })
}

/// Go's canonical header form (`authorization` → `Authorization`), the form Phase A ran with.
fn canonical_header(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

fn config(listen: &IronListen, deny: Vec<String>, transforms: Vec<Value>) -> Value {
    json!({
        "dns": { "listen": ":53", "proxy_ip": listen.proxy_ip },
        "proxy": {
            "http_listen": ":80",
            "https_listen": ":443",
            "tunnel_listen": format!(":{}", listen.tunnel_port),
            "upstream_deny_cidrs": deny,
        },
        "tls": {
            "ca_cert": format!("{CONTAINER_CERTS_DIR}/ca.pem"),
            "ca_key": format!("{CONTAINER_CERTS_DIR}/ca-key.pem"),
        },
        "transforms": transforms,
        "metrics": { "listen": format!("127.0.0.1:{METRICS_PORT}") },
        "management": {
            "listen": format!("127.0.0.1:{}", listen.mgmt_port),
            "api_key_env": MGMT_API_KEY_ENV,
        },
        "log": { "level": "info" },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::net_policy::{NetRule, SecretBinding};

    /// The config iron-proxy 0.56.0 ran in Phase A, copied from
    /// `docs/features/podman-egress-boundary/spike/fixtures/config.yaml`.
    const CAPTURED: &str = include_str!("fixtures/iron-config-0.56.0.yaml");

    /// Rendered keys the captured config did not exercise, with the Phase A evidence for each
    /// (`docs/features/podman-egress-boundary/spike/README.md`): 5b found `api_key_env` in the
    /// boot error, 6a read `match_body` from the 0.56.0 source.
    const UNCAPTURED_KEYS: &[&str] =
        &["management.api_key_env", "transforms[secrets].config.secrets[].replace.match_body"];

    fn listen() -> IronListen {
        IronListen { proxy_ip: "10.90.7.2".to_string(), tunnel_port: 8080, mgmt_port: 9092 }
    }

    fn allow(host: &str) -> NetRule {
        NetRule {
            action: RuleAction::Allow,
            host: Some(host.to_string()),
            cidr: None,
            group: None,
            protocol: None,
            ports: None,
        }
    }

    fn deny_cidr(cidr: &str) -> NetRule {
        NetRule {
            action: RuleAction::Deny,
            host: None,
            cidr: Some(cidr.to_string()),
            ..allow("x.y")
        }
    }

    fn binding(var: &str, hosts: &[&str]) -> SecretBinding {
        SecretBinding {
            secret_id: var.to_ascii_lowercase(),
            env_var: var.to_string(),
            hosts: hosts.iter().map(|h| h.to_string()).collect(),
            headers: vec!["authorization".to_string(), "x-api-key".to_string()],
            query: true,
            body: false,
        }
    }

    fn policy() -> NetPolicy {
        NetPolicy {
            egress: EgressMode::Allowlist,
            rules: vec![
                allow("API.example.com"),
                allow("*.githubusercontent.com"),
                deny_cidr("203.0.113.0/24"),
                deny_cidr("10.0.0.0/8"),
            ],
            secrets: vec![
                binding("OPENAI_API_KEY", &["API.example.com"]),
                binding("NO_VALUE", &["api.example.com"]),
            ],
            ..NetPolicy::default()
        }
    }

    /// Every key path of a JSON value; an array element with a `name` is labelled by it, so the
    /// two transforms' `config` keys stay apart.
    fn key_paths(value: &Value, prefix: &str, out: &mut BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                    out.insert(path.clone());
                    key_paths(v, &path, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    let label = item["name"].as_str().unwrap_or("");
                    key_paths(item, &format!("{prefix}[{label}]"), out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn every_rendered_key_was_run_against_iron_proxy() {
        let captured: Value = serde_json::from_str(CAPTURED).unwrap();
        let mut known = BTreeSet::new();
        key_paths(&captured, "", &mut known);
        known.extend(UNCAPTURED_KEYS.iter().map(|k| k.to_string()));
        let open = NetPolicy { egress: EgressMode::Open, ..policy() };
        let files = [SecretFile::new("OPENAI_API_KEY")];
        for config in [
            render(&policy(), &listen(), &files),
            render(&open, &listen(), &files),
            bootstrap(&listen()),
        ] {
            let mut rendered = BTreeSet::new();
            key_paths(&config, "", &mut rendered);
            let unknown: Vec<_> = rendered.difference(&known).collect();
            assert!(unknown.is_empty(), "keys never run against iron-proxy: {unknown:?}");
        }
    }
}
