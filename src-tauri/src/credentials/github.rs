//! The GitHub token: verifying and saving it on the host, and applying it to a sandbox's `gh`
//! (`docs/features/github-cli-auth/02_dev.md`). The token reaches a sandbox only on stdin.

use std::path::Path;
use std::sync::Arc;

use tauri::AppHandle;

use super::types::{CredentialError, CredentialManifest};
use super::{GITHUB_SERVICE, save_manifest, save_secret};
use crate::provider::types::{ExecOpts, MachineProvider};

/// What a sandbox behind an iron-proxy sidecar holds in place of the token; the sidecar swaps it
/// for the real token on GitHub's hosts (`docs/features/proxy-credential-secrets/02_dev.md`,
/// Resolved Decision 1). Carries no secret.
pub const GITHUB_PLACEHOLDER: &str = "ghp_MioPlaceholder0000000000000000000000";

/// Scopes `gh auth login --with-token` requires of a classic token.
const REQUIRED_SCOPES: [&str; 2] = ["repo", "read:org"];

/// What GitHub said about a token. Both fields stay empty when GitHub could not be asked.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TokenCheck {
    pub login: Option<String>,
    pub missing_scopes: Vec<String>,
}

/// Whether `granted` (a classic token's scope list) satisfies `required`. A broader org scope
/// covers `read:org`, as in gh's own check.
fn scope_satisfied(required: &str, granted: &[&str]) -> bool {
    if granted.contains(&required) {
        return true;
    }
    required == "read:org" && granted.iter().any(|s| matches!(*s, "write:org" | "admin:org"))
}

/// The gh-required scopes absent from an `X-OAuth-Scopes` header. Fine-grained tokens send no
/// header and so get no warning.
pub fn missing_gh_scopes(header: Option<&str>) -> Vec<String> {
    let Some(header) = header else { return Vec::new() };
    let granted: Vec<&str> = header.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    REQUIRED_SCOPES
        .iter()
        .filter(|required| !scope_satisfied(required, &granted))
        .map(|s| s.to_string())
        .collect()
}

/// Asks GitHub who owns `token`. A `401` is an error; any other failure (offline, rate limit)
/// yields an unverified check so a save never depends on the network.
pub async fn verify_token(token: &str) -> Result<TokenCheck, CredentialError> {
    // The light tier must not call GitHub, and its specs save a sentinel token.
    if crate::backends::mock_backend_enabled() {
        return Ok(TokenCheck::default());
    }
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build() {
        Ok(client) => client,
        Err(_) => return Ok(TokenCheck::default()),
    };
    let response = client
        .get("https://api.github.com/user")
        .bearer_auth(token)
        .header("User-Agent", "sandbox-mio")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(e) => {
            tracing::warn!(target: "credentials", error = %e, "GitHub token left unverified");
            return Ok(TokenCheck::default());
        }
    };
    match response.status().as_u16() {
        401 => Err(CredentialError::InvalidValue("GitHub rejected the token".to_string())),
        200 => {
            let scopes = response
                .headers()
                .get("x-oauth-scopes")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let login = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|body| body.get("login").and_then(|v| v.as_str()).map(str::to_string));
            Ok(TokenCheck { login, missing_scopes: missing_gh_scopes(scopes.as_deref()) })
        }
        status => {
            tracing::warn!(target: "credentials", status, "GitHub token left unverified");
            Ok(TokenCheck::default())
        }
    }
}

/// Verifies and stores `token` as the `github` credential, recording the account login and any
/// missing scopes in its manifest. Both the paste form and the login flow end here.
pub async fn save_github_token(
    app: &AppHandle,
    token: &str,
) -> Result<CredentialManifest, CredentialError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(CredentialError::InvalidValue("the value is empty".to_string()));
    }
    let check = verify_token(token).await?;
    let mut manifest = save_secret(app, GITHUB_SERVICE, token)?;
    manifest.account_login = check.login;
    manifest.missing_scopes = check.missing_scopes;
    save_manifest(app, GITHUB_SERVICE, &manifest)?;
    Ok(manifest)
}

/// Exit status of [`APPLY_SCRIPT`] when the sandbox's image has no gh.
const EXIT_NO_GH: i32 = 64;

/// Longest `Failed` reason kept, so one noisy stderr line cannot bloat the state file or the card.
pub(crate) const MAX_REASON_CHARS: usize = 200;

/// Runs with the token on stdin. The hosts file is cleared first so a re-login as another account
/// replaces the old token instead of adding a second account
/// (`docs/features/github-cli-auth/02_dev.md`, Resolved Decision 8).
const APPLY_SCRIPT: &str = "[ -x /usr/local/bin/gh ] || exit 64; rm -f /root/.config/gh/hosts.yml; /usr/local/bin/gh auth login --with-token --hostname github.com --git-protocol https || exit $?; /usr/local/bin/gh auth setup-git";

/// Deletes gh's hosts file. The git helper section `gh auth setup-git` wrote stays and fails closed.
const REMOVE_SCRIPT: &str = "rm -f /root/.config/gh/hosts.yml";

/// What the last apply to one sandbox did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ApplyOutcome {
    /// gh holds the credential captured at `captured_at_ms`. `swapped` is true when gh holds the
    /// placeholder that a sidecar swaps, false when it holds the real token; state files from
    /// before the sidecar swap load as false.
    Applied {
        captured_at_ms: u64,
        #[serde(default)]
        swapped: bool,
    },
    /// The sandbox's image predates gh; recreating it over its workspace fixes that.
    NeedsImage,
    /// gh or the sandbox refused; `reason` is gh's first stderr line.
    Failed { reason: String, captured_at_ms: u64 },
}

/// Per-sandbox GitHub state, kept in the app-data root and never in the workspace, which code
/// inside the sandbox can write (`docs/features/github-cli-auth/02_dev.md`, Resolved Decision 10).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GithubSandboxState {
    pub opted_out: bool,
    pub outcome: Option<ApplyOutcome>,
}

/// The state at `path`; a missing file is the default (opted in, nothing applied).
pub fn load_state(path: &Path) -> Result<GithubSandboxState, CredentialError> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| CredentialError::Parse(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(GithubSandboxState::default()),
        Err(e) => Err(CredentialError::Io(e.to_string())),
    }
}

pub fn save_state(path: &Path, state: &GithubSandboxState) -> Result<(), CredentialError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CredentialError::Io(e.to_string()))?;
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| CredentialError::Parse(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| CredentialError::Io(e.to_string()))
}

/// The first non-empty stderr line, capped.
fn first_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("gh refused the token");
    line.chars().take(MAX_REASON_CHARS).collect()
}

/// Logs gh into `name` with `token`, and wires git to gh. `captured_at_ms` is the credential's
/// manifest stamp, recorded so the card can tell a current apply from an older one. `token` is the
/// real token or the placeholder; `swapped` records which one the caller passed.
pub async fn apply(
    provider: &Arc<dyn MachineProvider>,
    name: &str,
    token: &str,
    captured_at_ms: u64,
    swapped: bool,
) -> ApplyOutcome {
    let out = match provider.exec_piped(name, &["sh", "-c", APPLY_SCRIPT], token.as_bytes()).await {
        Ok(out) => out,
        Err(e) => {
            return ApplyOutcome::Failed { reason: e.to_string().chars().take(MAX_REASON_CHARS).collect(), captured_at_ms }
        }
    };
    match out.status.code() {
        Some(0) => ApplyOutcome::Applied { captured_at_ms, swapped },
        Some(EXIT_NO_GH) => ApplyOutcome::NeedsImage,
        _ => ApplyOutcome::Failed { reason: first_line(&out.stderr), captured_at_ms },
    }
}

/// Removes the token gh holds in `name`.
pub async fn remove(provider: &Arc<dyn MachineProvider>, name: &str) -> Result<(), CredentialError> {
    let out = provider.exec(name, &["sh", "-c", REMOVE_SCRIPT], ExecOpts::default()).await?;
    if out.status.success() {
        Ok(())
    } else {
        Err(CredentialError::ExecFailed(first_line(&out.stderr)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_placeholder_has_the_shape_of_a_classic_token() {
        assert!(GITHUB_PLACEHOLDER.starts_with("ghp_"));
        assert_eq!(GITHUB_PLACEHOLDER.len(), 40);
    }

    #[test]
    fn the_first_stderr_line_is_kept_and_capped() {
        assert_eq!(first_line(b"
 error validating token: HTTP 401
more"), "error validating token: HTTP 401");
        assert_eq!(first_line(b""), "gh refused the token");
        assert_eq!(first_line("x".repeat(500).as_bytes()).chars().count(), MAX_REASON_CHARS);
    }

    #[test]
    fn an_applied_outcome_without_swapped_loads_as_plaintext() {
        let json = r#"{"kind":"applied","capturedAtMs":3}"#;
        let outcome: ApplyOutcome = serde_json::from_str(json).expect("parses");
        assert_eq!(outcome, ApplyOutcome::Applied { captured_at_ms: 3, swapped: false });
    }

    #[test]
    fn a_missing_state_file_is_opted_in_with_no_outcome() {
        let path = std::env::temp_dir().join(format!("mio-gh-state-missing-{}.json", std::process::id()));
        assert_eq!(load_state(&path).expect("loads"), GithubSandboxState::default());
    }

    #[test]
    fn state_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("mio-gh-state-{}", std::process::id()));
        let path = dir.join("github").join("box.json");
        let state = GithubSandboxState {
            opted_out: true,
            outcome: Some(ApplyOutcome::Failed { reason: "no".into(), captured_at_ms: 7 }),
        };
        save_state(&path, &state).expect("saves");
        assert_eq!(load_state(&path).expect("loads"), state);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_scope_header_means_no_warning() {
        assert!(missing_gh_scopes(None).is_empty());
    }

    #[test]
    fn a_classic_token_without_read_org_is_flagged() {
        assert_eq!(missing_gh_scopes(Some("repo, gist")), ["read:org"]);
    }

    #[test]
    fn a_token_with_every_required_scope_is_clean() {
        assert!(missing_gh_scopes(Some("gist, read:org, repo")).is_empty());
    }

    #[test]
    fn a_broader_org_scope_covers_read_org() {
        assert!(missing_gh_scopes(Some("repo, admin:org")).is_empty());
    }

    #[test]
    fn an_empty_header_misses_everything() {
        assert_eq!(missing_gh_scopes(Some("")), ["repo", "read:org"]);
    }
}
