//! Renders a validated `.mio/net-policy.json` into an iron-proxy config
//! (`docs/features/podman-egress-boundary/02_dev.md`, Phase C). Pure: the caller writes the value
//! as `config.yaml`, relying on YAML 1.2 being a superset of JSON (Resolved Decision 12).
//!
//! iron-proxy silently ignores an unknown key inside a transform's `config`, so a misspelled key
//! fails open; the tests pin every rendered key against a config captured from a real run.

use serde_json::{json, Value};

use crate::credentials::github::GITHUB_PLACEHOLDER;
use crate::net_policy::{self, EgressMode, NetPolicy, RuleAction};

/// Replaces iron-proxy's default deny list in every config, so an allowed host resolving to a
/// private address cannot reach the machine, host or LAN (Resolved Decision 17). The last entry is
/// iron-proxy's IPv6 metadata address; the IPv4 one is inside `169.254.0.0/16`.
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

pub const CONTAINER_CERTS_DIR: &str = "/etc/iron-proxy/certs";
pub const CONTAINER_SECRETS_DIR: &str = "/etc/iron-proxy/secrets";

/// The GitHub binding's secret file. Lowercase and hyphenated, so it can never equal a workspace
/// binding's `[A-Z0-9_]` file name (`docs/features/proxy-credential-secrets/02_dev.md`, Resolved
/// Decision 6).
pub const GITHUB_BINDING_FILE: &str = "mio-github";

/// Hosts the GitHub placeholder is swapped for (`docs/features/proxy-credential-secrets/02_dev.md`,
/// Resolved Decision 3).
pub const GITHUB_BINDING_HOSTS: &[&str] = &[
    "github.com",
    "api.github.com",
    "uploads.github.com",
    "gist.github.com",
    "raw.githubusercontent.com",
];

/// A secret binding the app owns, rendered beside the workspace policy's bindings but never stored
/// in `.mio/net-policy.json`, which code inside the sandbox can write.
pub struct AppBinding {
    /// The file under [`CONTAINER_SECRETS_DIR`] holding the value.
    pub file: String,
    pub placeholder: String,
    pub hosts: Vec<String>,
    pub headers: Vec<String>,
}

/// The GitHub token binding: `Authorization` headers to GitHub's hosts only.
pub fn github_app_binding() -> AppBinding {
    AppBinding {
        file: GITHUB_BINDING_FILE.to_string(),
        placeholder: GITHUB_PLACEHOLDER.to_string(),
        hosts: GITHUB_BINDING_HOSTS.iter().map(|h| h.to_string()).collect(),
        headers: vec!["Authorization".to_string()],
    }
}

/// The sidecar environment variable holding the management bearer token; iron-proxy refuses to
/// boot a management listener without it (Resolved Decision 13).
pub const MGMT_API_KEY_ENV: &str = "IRON_MANAGEMENT_API_KEY";

pub const IRON_MGMT_PORT: u16 = 9092;

/// iron-proxy cannot disable metrics, so the listener is pinned to the sidecar's loopback.
const METRICS_PORT: u16 = 9090;

/// The config the sidecar runs with before the first apply: an empty allowlist outside warn mode,
/// so it denies everything (Resolved Decision 18).
pub fn bootstrap(proxy_ip: &str) -> Value {
    config(
        proxy_ip,
        BASELINE_DENY_CIDRS.iter().map(|c| c.to_string()).collect(),
        vec![allowlist(false, Vec::new())],
    )
}

/// The config enforcing `policy`, which [`net_policy::validate`] has accepted in the sidecar
/// dialect. `with_value` names the bindings whose secret has a stored value; any other binding is
/// left out, so its placeholder passes through unswapped (Resolved Decision 15). `app_bindings` are
/// appended after the workspace bindings.
pub fn render(
    policy: &NetPolicy,
    proxy_ip: &str,
    with_value: &[String],
    app_bindings: &[AppBinding],
) -> Value {
    let mut domains: Vec<String> = Vec::new();
    let mut deny = BASELINE_DENY_CIDRS.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    for rule in &policy.rules {
        match (rule.action, &rule.host, &rule.cidr) {
            (RuleAction::Allow, Some(host), _) => {
                push_unique(&mut domains, host.to_ascii_lowercase())
            }
            (RuleAction::Deny, _, Some(cidr)) => push_unique(&mut deny, with_prefix(cidr)),
            _ => {}
        }
    }
    let mut transforms = vec![allowlist(policy.egress == EgressMode::Open, domains)];
    let mut secrets: Vec<Value> = policy
        .secrets
        .iter()
        .filter(|b| with_value.contains(&b.env_var))
        .map(|b| {
            let rules: Vec<Value> =
                b.hosts.iter().map(|h| json!({ "host": h.to_ascii_lowercase() })).collect();
            // `env_var` is validated as `[A-Z0-9_]`, a safe file name (Resolved Decision 15).
            let path = format!("{CONTAINER_SECRETS_DIR}/{}", b.env_var);
            json!({
                "source": { "type": "file", "path": path },
                "replace": {
                    "proxy_value": net_policy::placeholder(&b.env_var),
                    "match_headers": b.headers.iter().map(|h| canonical_header(h)).collect::<Vec<_>>(),
                    "match_query": b.query,
                    "match_body": b.body,
                },
                "rules": rules,
            })
        })
        .collect();
    secrets.extend(app_bindings.iter().map(|b| {
        let rules: Vec<Value> = b.hosts.iter().map(|h| json!({ "host": h })).collect();
        json!({
            "source": { "type": "file", "path": format!("{CONTAINER_SECRETS_DIR}/{}", b.file) },
            "replace": {
                "proxy_value": b.placeholder,
                "match_headers": b.headers,
                "match_query": false,
                "match_body": false,
            },
            "rules": rules,
        })
    }));
    // Omitted rather than rendered empty: an empty `secrets` transform was never run in Phase A.
    if !secrets.is_empty() {
        transforms.push(json!({ "name": "secrets", "config": { "secrets": secrets } }));
    }
    config(proxy_ip, deny, transforms)
}

/// A bare address as a single-host range: `validate` accepts one, but iron-proxy parses every
/// `upstream_deny_cidrs` entry as a CIDR and refuses the whole config on a missing prefix.
fn with_prefix(cidr: &str) -> String {
    if cidr.contains('/') {
        return cidr.to_string();
    }
    let max = if cidr.contains(':') { 128 } else { 32 };
    format!("{cidr}/{max}")
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

fn config(proxy_ip: &str, deny: Vec<String>, transforms: Vec<Value>) -> Value {
    json!({
        "dns": { "listen": ":53", "proxy_ip": proxy_ip },
        "proxy": {
            "http_listen": ":80",
            "https_listen": ":443",
            "tunnel_listen": format!(":{}", super::PROXY_PORT),
            "upstream_deny_cidrs": deny,
        },
        "tls": {
            "ca_cert": format!("{CONTAINER_CERTS_DIR}/ca.pem"),
            "ca_key": format!("{CONTAINER_CERTS_DIR}/ca-key.pem"),
        },
        "transforms": transforms,
        "metrics": { "listen": format!("127.0.0.1:{METRICS_PORT}") },
        "management": {
            "listen": format!("127.0.0.1:{IRON_MGMT_PORT}"),
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
        let with_value = ["OPENAI_API_KEY".to_string()];
        for config in [
            render(&policy(), "10.90.7.2", &with_value, &[]),
            render(&policy(), "10.90.7.2", &with_value, &[github_app_binding()]),
            render(&open, "10.90.7.2", &with_value, &[]),
            bootstrap("10.90.7.2"),
        ] {
            let mut rendered = BTreeSet::new();
            key_paths(&config, "", &mut rendered);
            let unknown: Vec<_> = rendered.difference(&known).collect();
            assert!(unknown.is_empty(), "keys never run against iron-proxy: {unknown:?}");
        }
    }

    #[test]
    fn a_bare_deny_address_gets_a_single_host_prefix() {
        let p = NetPolicy {
            rules: vec![deny_cidr("203.0.113.5"), deny_cidr("2001:db8::1"), deny_cidr("198.51.100.0/24")],
            ..NetPolicy::default()
        };
        let config = render(&p, "10.90.7.2", &[], &[]);
        let deny: Vec<&str> = config["proxy"]["upstream_deny_cidrs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(deny.ends_with(&["203.0.113.5/32", "2001:db8::1/128", "198.51.100.0/24"]), "{deny:?}");
    }

    #[test]
    fn the_github_entry_swaps_authorization_only_for_github_api_and_content_hosts() {
        let config = render(&NetPolicy::default(), "10.90.7.2", &[], &[github_app_binding()]);
        let secrets = config["transforms"][1]["config"]["secrets"].as_array().unwrap();
        assert_eq!(secrets.len(), 1);
        let entry = &secrets[0];
        let hosts: Vec<&str> =
            entry["rules"].as_array().unwrap().iter().map(|r| r["host"].as_str().unwrap()).collect();
        // raw.githubusercontent.com is deliberately in scope: mise sends its token there
        // (`proxy-credential-secrets/02_dev.md`, Resolved Decision 15).
        assert_eq!(
            hosts,
            [
                "github.com",
                "api.github.com",
                "uploads.github.com",
                "gist.github.com",
                "raw.githubusercontent.com"
            ]
        );
        assert_eq!(entry["replace"]["match_headers"], json!(["Authorization"]));
        assert_eq!(entry["replace"]["match_query"], json!(false));
        assert_eq!(entry["replace"]["match_body"], json!(false));
        assert_eq!(entry["replace"]["proxy_value"], GITHUB_PLACEHOLDER);
        assert_eq!(entry["source"]["path"], "/etc/iron-proxy/secrets/mio-github");
    }

    #[test]
    fn the_secrets_transform_is_omitted_without_any_binding() {
        let config = render(&NetPolicy::default(), "10.90.7.2", &[], &[]);
        assert_eq!(config["transforms"].as_array().unwrap().len(), 1);
    }
}
