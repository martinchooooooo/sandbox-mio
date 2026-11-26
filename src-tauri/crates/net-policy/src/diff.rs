//! The reviewable difference between the accepted policy and a changed workspace file.

use super::schema::{NetPolicy, NetRule, SecretBinding};
use mio_provider::types::{EgressMode, RuleAction};

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

#[cfg(test)]
mod tests {
    use super::super::schema::starter;
    use super::super::test_support::{binding, rule, PODMAN};
    use super::*;

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
}
