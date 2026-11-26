//! One-time split of the app-global legacy secret file into per-sandbox value stores.

use std::collections::HashMap;

use tauri::AppHandle;

use super::io_err;
use super::store::ProxyStore;
use super::types::ProxyError;

/// Owner of each legacy value (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 9).
/// `users` pairs each live sandbox with the secret ids its accepted policy names.
fn split_legacy_secrets(
    legacy: &HashMap<String, String>,
    users: &[(String, Vec<String>)],
) -> HashMap<String, HashMap<String, String>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (id, value) in legacy {
        // The longest prefix wins, so `a` never claims `a-b-key`; a prefixed id never falls through
        // to the sole-user rule.
        let prefix_owner = users
            .iter()
            .filter(|(name, _)| id.starts_with(&format!("{name}-")))
            .max_by_key(|(name, _)| name.len())
            .map(|(name, _)| name);
        let owner = prefix_owner.or_else(|| {
            let mut named_by = users.iter().filter(|(_, ids)| ids.contains(id)).map(|(name, _)| name);
            named_by.next().filter(|_| named_by.next().is_none())
        });
        if let Some(owner) = owner {
            out.entry(owner.clone()).or_default().insert(id.clone(), value.clone());
        }
    }
    out
}

/// Moves the app-global `proxy/secrets.json` into the value stores of the `live` sandboxes, then
/// deletes it. A failure leaves the file for the next start.
pub fn migrate_legacy_secrets(app: &AppHandle, store: &ProxyStore, live: &[String]) {
    if let Err(e) = migrate_legacy_secrets_inner(app, store, live) {
        tracing::warn!(target: "proxy", error = %e, "legacy secret migration failed; will retry at next start");
    }
}

fn migrate_legacy_secrets_inner(app: &AppHandle, store: &ProxyStore, live: &[String]) -> Result<(), ProxyError> {
    let legacy_path = store.legacy_secrets_path()?;
    let Some(value) = crate::mio_files::read_value(&legacy_path) else {
        return Ok(());
    };
    let legacy: HashMap<String, String> = serde_json::from_value(value)
        .map_err(|e| ProxyError::CommandFailed(format!("failed to parse {}: {e}", legacy_path.to_string_lossy())))?;
    // A sandbox without a snapshot still takes its prefixed values; a snapshot of a destroyed one
    // takes nothing.
    let users: Vec<(String, Vec<String>)> = live
        .iter()
        .map(|name| {
            let ids = match crate::net_policy::applied::read_applied(app, name) {
                Ok(Some(applied)) => applied.policy.secrets.into_iter().map(|b| b.secret_id).collect(),
                _ => Vec::new(),
            };
            (name.clone(), ids)
        })
        .collect();
    for (sandbox, values) in split_legacy_secrets(&legacy, &users) {
        let mut current = store.load_secrets(&sandbox)?;
        for (id, value) in values {
            current.entry(id).or_insert(value);
        }
        store.save_secrets(&sandbox, &current)?;
    }
    std::fs::remove_file(&legacy_path).map_err(|e| io_err(&legacy_path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_values_split_by_prefix_or_sole_user() {
        let legacy: HashMap<String, String> = ["a-key", "a-b-key", "shared", "solo", "orphan", "a-borrowed"]
            .iter()
            .map(|id| (id.to_string(), format!("v-{id}")))
            .collect();
        let users = vec![
            ("a".to_string(), vec!["a-key".to_string(), "shared".to_string()]),
            (
                "a-b".to_string(),
                vec!["a-b-key".to_string(), "shared".to_string(), "solo".to_string(), "a-borrowed".to_string()],
            ),
            ("bare".to_string(), Vec::new()),
        ];
        let out = split_legacy_secrets(&legacy, &users);
        let has = |sb: &str, id: &str| out.get(sb).is_some_and(|m| m.contains_key(id));
        assert!(has("a", "a-key") && !has("a-b", "a-key"));
        assert!(has("a-b", "a-b-key") && !has("a", "a-b-key"), "longest sandbox name owns the prefix");
        assert!(!has("a", "shared") && !has("a-b", "shared"), "two users, no prefix: dropped");
        assert!(has("a-b", "solo"));
        assert!(!has("a-b", "a-borrowed") && has("a", "a-borrowed"), "a prefixed id never goes to a naming sandbox");
        assert!(!out.values().any(|m| m.contains_key("orphan")));
    }

    #[test]
    fn a_sandbox_without_a_snapshot_keeps_its_prefixed_values_only() {
        let legacy: HashMap<String, String> =
            [("bare-key", "1"), ("gone-key", "2"), ("loose", "3")].map(|(k, v)| (k.to_string(), v.to_string())).into();
        let users = vec![("bare".to_string(), Vec::new())];
        let out = split_legacy_secrets(&legacy, &users);
        assert_eq!(out.get("bare").map(HashMap::len), Some(1));
        assert!(out["bare"].contains_key("bare-key"));
        assert_eq!(out.len(), 1, "a destroyed sandbox's value and an unnamed one go nowhere");
    }
}
