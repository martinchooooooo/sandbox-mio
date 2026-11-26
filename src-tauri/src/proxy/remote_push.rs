//! Driving the remote sidecar: the shell scripts it runs, the secret-file pushes over stdin, and the
//! config swap with its reload.

use std::path::Path;

use super::iron::CONTAINER_CONFIG_DIR;
use super::sidecar::reload;
use super::types::{ProxyError, SidecarSite};
use super::{io_err, iron_config};
use crate::provider::types::ExecOpts;

/// Whether `name` is safe to splice into the `sh -c` scripts below: secret file names come from a
/// validated environment variable name or the app's own constant, never from the sandbox.
fn safe_file_name(name: &str) -> Result<(), ProxyError> {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
        Ok(())
    } else {
        Err(ProxyError::CommandFailed(format!("unsafe secret file name {name:?}")))
    }
}

/// The remote secrets directory's path for one file.
fn remote_secret_path(name: &str) -> String {
    format!("{}/{name}", iron_config::CONTAINER_SECRETS_DIR)
}

/// The shell script that writes stdin to `name` in the sidecar's secrets directory through a
/// rename, so iron-proxy never reads a partial file.
fn push_script(name: &str) -> String {
    let target = remote_secret_path(name);
    format!("cat > {target}.tmp && mv {target}.tmp {target}")
}

/// The shell script that deletes every file in the secrets directory not named in `keep`.
fn stale_script(keep: &[String]) -> String {
    format!(
        "cd {dir} && for f in $(ls -A); do case \" {keep} \" in *\" $f \"*) ;; *) rm -f -- \"$f\" ;; esac; done",
        dir = iron_config::CONTAINER_SECRETS_DIR,
        keep = keep.join(" "),
    )
}

/// The shell script that swaps the copied-in `config.next.yaml` over `config.yaml`, keeping the
/// running config as `config.prev.yaml`; [`swap_back_script`] undoes it
/// (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 14).
fn swap_script() -> String {
    let dir = CONTAINER_CONFIG_DIR;
    format!("cd {dir} && mv -f config.yaml config.prev.yaml && mv -f config.next.yaml config.yaml")
}

fn swap_back_script() -> String {
    format!("cd {CONTAINER_CONFIG_DIR} && mv -f config.prev.yaml config.yaml")
}

/// Runs `script` in the remote sidecar and turns a non-zero exit into an error.
async fn exec_script(site: &SidecarSite, sidecar: &str, script: &str) -> Result<(), ProxyError> {
    let out = site.provider.exec(sidecar, &["sh", "-c", script], ExecOpts::default()).await?;
    if out.status.success() {
        return Ok(());
    }
    Err(ProxyError::CommandFailed(String::from_utf8_lossy(&out.stderr).trim().to_string()))
}

/// Writes each of `names` from the laptop's `secrets` directory into the remote sidecar's tmpfs. The
/// value travels on stdin only, never argv, the container environment, logs or IPC.
pub(super) async fn push_secret_files(
    site: &SidecarSite,
    sidecar: &str,
    secrets: &Path,
    names: &[String],
) -> Result<(), ProxyError> {
    for name in names {
        safe_file_name(name)?;
        let path = secrets.join(name);
        let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
        let script = push_script(name);
        let out = site.provider.exec_piped(sidecar, &["sh", "-c", &script], &bytes).await?;
        if !out.status.success() {
            return Err(ProxyError::CommandFailed(format!(
                "could not push a secret file to the remote proxy: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    }
    Ok(())
}

/// Deletes the remote secret files not in `keep`. Runs only after a successful reload, for the reason
/// `secret_files::remove_stale_secret_files` does.
pub(super) async fn remove_stale_remote_files(
    site: &SidecarSite,
    sidecar: &str,
    keep: &[String],
) -> Result<(), ProxyError> {
    for name in keep {
        safe_file_name(name)?;
    }
    exec_script(site, sidecar, &stale_script(keep)).await
}

/// The names of the files in the laptop's `secrets` directory.
pub(super) fn staged_secret_names(secrets: &Path) -> Vec<String> {
    std::fs::read_dir(secrets)
        .map(|entries| {
            entries.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()
        })
        .unwrap_or_default()
}

/// The remote half of an apply: pushes `names`, copies the rendered config in as `config.next.yaml`,
/// swaps it over `config.yaml` with one exec and reloads. A rejected reload swaps the old config back;
/// the sidecar is never restarted as a fallback.
pub(super) async fn push_swap_reload(
    site: &SidecarSite,
    sidecar: &str,
    secrets: &Path,
    names: &[String],
    rendered_config: &Path,
) -> Result<(), ProxyError> {
    push_secret_files(site, sidecar, secrets, names).await?;
    let next = format!("{CONTAINER_CONFIG_DIR}/config.next.yaml");
    site.provider.copy_in(sidecar, &rendered_config.to_string_lossy(), &next).await?;
    exec_script(site, sidecar, &swap_script()).await?;
    if let Err(e) = reload(&site.provider, sidecar).await {
        if let Err(back) = exec_script(site, sidecar, &swap_back_script()).await {
            tracing::error!(target: "proxy", %sidecar, error = %back, "failed to swap the previous remote config back");
        }
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_scripts_write_through_a_rename_and_swap_in_one_command() {
        assert_eq!(
            push_script("A_KEY"),
            "cat > /etc/iron-proxy/secrets/A_KEY.tmp && mv /etc/iron-proxy/secrets/A_KEY.tmp /etc/iron-proxy/secrets/A_KEY"
        );
        let swap = swap_script();
        assert!(swap.contains("mv -f config.yaml config.prev.yaml && mv -f config.next.yaml config.yaml"));
        assert!(swap_back_script().ends_with("mv -f config.prev.yaml config.yaml"));
        let stale = stale_script(&["A_KEY".to_string(), "mio-github".to_string()]);
        assert!(stale.contains("\" A_KEY mio-github \""), "{stale}");
    }

    /// File names reach `sh -c`, so anything outside the environment-variable alphabet is refused.
    #[test]
    fn unsafe_secret_file_names_are_refused() {
        assert!(safe_file_name("OPENAI_API_KEY").is_ok());
        assert!(safe_file_name("mio-github").is_ok());
        for bad in ["", "a b", "a;rm", "../x", "a$b", "a'b"] {
            assert!(safe_file_name(bad).is_err(), "{bad}");
        }
    }
}
